use anyhow::Result;
use atlas_connectors::Connector;
use atlas_git::{GitHubIssueConnector, GitHubPrConnector, GitRepo};
use atlas_ir::{
    CommitSummary, ContextDocument, CouplingEntry, CoverageMap, CoverageStatus,
    EvidenceSummary, FileIdentity, FileSignificance, IssueSummary, PrSummary,
    RelatedHistory,
};
use atlas_parser::{gh_json, git_log};
use atlas_storage::{CommitRow, HotFileRow, Store};
use tracing::info;

pub fn ingest_git(repo_path: &str, store: &Store) -> Result<usize> {
    let connector = GitRepo::open(repo_path)?;
    let payload   = connector.fetch_raw()?;
    let commits   = git_log::parse(&payload.data)?;
    let count     = commits.len();

    info!(
        "connector={} capability={} parsed={} commits",
        connector.name(),
        connector.capability().name,
        count,
    );

    for commit in &commits {
        store.insert_commit(commit, repo_path)?;
    }

    Ok(count)
}

/// Ingest GitHub data from raw JSON strings (testable without the `gh` binary).
pub fn ingest_github_from_json(
    prs_json:   &str,
    issues_json: &str,
    repo_path:  &str,
    store:      &Store,
) -> Result<usize> {
    let prs    = gh_json::parse_prs(prs_json)?;
    let links  = gh_json::parse_pr_issue_links(prs_json)?;
    let issues = gh_json::parse_issues(issues_json)?;

    let pr_count = prs.len();

    info!("parsed={} PRs {} issues {} links", pr_count, issues.len(), links.len());

    for pr in &prs {
        store.insert_pull_request(pr, repo_path)?;
    }
    for (pr_number, issue_number) in &links {
        store.link_pr_to_issue(*pr_number, *issue_number, repo_path)?;
    }
    for issue in &issues {
        store.insert_issue(issue, repo_path)?;
    }

    Ok(pr_count)
}

pub fn ingest_github(repo_path: &str, store: &Store) -> Result<usize> {
    let pr_conn    = GitHubPrConnector::new(repo_path);
    let issue_conn = GitHubIssueConnector::new(repo_path);

    let prs_json    = pr_conn.fetch_raw()?.data;
    let issues_json = issue_conn.fetch_raw()?.data;

    ingest_github_from_json(&prs_json, &issues_json, repo_path, store)
}

/// Assemble a `ContextDocument` for `file` in `repo_path` from all available sources.
///
/// This is the context engine: it composes every query primitive into a single
/// typed document that can be rendered to CLI, serialized to JSON, or fed to an LLM.
pub fn build_context(file: &str, repo_path: &str, store: &Store) -> Result<ContextDocument> {
    let first_commit = store.first_seen(file, repo_path)?;
    let last_commit  = store.last_seen(file, repo_path)?;
    let touch_count  = store.touch_count(file, repo_path)?;

    // Newest-first for recent activity display.
    let all_commits = store.commits_for_file(file, repo_path)?;

    // PRs whose merge commit touched this file, plus issue linkage.
    let pr_rows    = store.prs_for_file(file, repo_path)?;
    let issue_rows = store.issues_for_file(file, repo_path)?;

    let mut prs = Vec::with_capacity(pr_rows.len());
    for pr in &pr_rows {
        let linked = store.issue_numbers_for_pr(pr.number, repo_path)?;
        prs.push(PrSummary {
            number:           pr.number,
            title:            pr.title.clone(),
            state:            pr.state.clone(),
            merge_commit_sha: pr.merge_commit_sha.clone(),
            linked_issues:    linked,
        });
    }
    let issues: Vec<IssueSummary> = issue_rows.iter().map(|i| IssueSummary {
        number: i.number,
        title:  i.title.clone(),
        state:  i.state.clone(),
    }).collect();

    // Co-changes — partitioned into documentary (docs, markdown) and source coupling.
    let co_changes = store.co_changes_for_file(file, repo_path, 1)?;
    let (doc_changes, src_changes): (Vec<_>, Vec<_>) = co_changes
        .into_iter()
        .partition(|e| is_documentary(&e.file_path));

    let coupling: Vec<CouplingEntry> = src_changes.iter().map(|c| CouplingEntry {
        file_path:    c.file_path.clone(),
        change_count: c.change_count,
    }).collect();
    let documentary: Vec<CouplingEntry> = doc_changes.iter().map(|c| CouplingEntry {
        file_path:    c.file_path.clone(),
        change_count: c.change_count,
    }).collect();

    // Significance: rank among all files in the repo.
    let hot_all     = store.hot_files(repo_path, 9999)?;
    let significance = compute_significance(file, touch_count, &hot_all);

    // Coverage map — what sources are present?
    let repo_commits = store.commit_count(repo_path)?;
    let repo_prs     = store.pr_count(repo_path)?;
    let repo_issues  = store.issue_count(repo_path)?;
    let doc_status   = if documentary.is_empty() {
        CoverageStatus::NotIngested
    } else {
        CoverageStatus::CoChangeOnly
    };

    let co_count = coupling.len() + documentary.len();
    let evidence = EvidenceSummary {
        commits:     all_commits.len(),
        prs:         prs.len(),
        issues:      issues.len(),
        co_changes:  co_count,
        total_facts: all_commits.len() + prs.len() + issues.len() + co_count,
    };

    Ok(ContextDocument {
        subject: file.to_string(),
        identity: FileIdentity {
            first_commit: first_commit.map(row_to_summary),
            last_commit:  last_commit.map(row_to_summary),
            touch_count,
        },
        recent_activity: all_commits.into_iter().map(row_to_summary).collect(),
        related_history: RelatedHistory { pull_requests: prs, issues },
        coupling,
        documentary,
        significance,
        evidence,
        coverage: CoverageMap {
            git_history:   if repo_commits > 0 { CoverageStatus::Available } else { CoverageStatus::NotIngested },
            github_prs:    if repo_prs     > 0 { CoverageStatus::Available } else { CoverageStatus::NotIngested },
            github_issues: if repo_issues  > 0 { CoverageStatus::Available } else { CoverageStatus::NotIngested },
            documentation: doc_status,
            working_tree:  CoverageStatus::NotIngested,
        },
    })
}

fn row_to_summary(c: CommitRow) -> CommitSummary {
    CommitSummary {
        short_hash: c.short_hash,
        message:    c.message,
        author:     c.author_name,
        timestamp:  c.timestamp,
    }
}

// Document discovery uses a path-based heuristic.
//
// This is discovery evidence, not document ontology.
// A path matching these patterns is treated as a document candidate
// for context assembly under CoverageStatus::CoChangeOnly.
//
// Future sources may include: nested READMEs, RFC directories, exported
// Notion documents, PDFs, office documents, browser captures, and chat
// attachments.  Do not treat this heuristic as authoritative classification.
fn is_documentary(path: &str) -> bool {
    path.starts_with("docs/")
        || path.ends_with(".md")
        || path.ends_with(".rst")
        || path.ends_with(".adoc")
}

fn compute_significance(file: &str, touch_count: i64, hot: &[HotFileRow]) -> Option<FileSignificance> {
    if touch_count == 0 {
        return None;
    }
    let total = hot.len();
    let rank  = hot.iter().position(|r| r.file_path == file)
                   .map(|i| i + 1)
                   .unwrap_or(total + 1);
    Some(FileSignificance { rank, total_files: total, touch_count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_storage::Store;
    use std::process::Command;
    use tempfile::TempDir;

    // ── Fixture git repo builder ────────────────────────────────────────────

    struct FixtureRepo {
        _dir:    TempDir,
        pub path:   String,
        pub hash_a: String,
        pub hash_b: String,
        #[allow(dead_code)]
        pub hash_c: String,
    }

    fn create_fixture_repo() -> FixtureRepo {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().to_str().unwrap().to_string();

        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(&p)
                .output()
                .expect("git");
            assert!(
                status.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&status.stderr)
            );
        };

        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "test@test.com"]);
        git(&["config", "user.name", "Test"]);

        let commit_dated = |path: &str, msg: &str, date: &str| {
            let out = Command::new("git")
                .args(["commit", "-m", msg])
                .current_dir(path)
                .env("GIT_AUTHOR_DATE",    date)
                .env("GIT_COMMITTER_DATE", date)
                .output()
                .expect("git commit");
            assert!(out.status.success(), "commit failed: {}", String::from_utf8_lossy(&out.stderr));
        };

        // Commit A — creates auth.ts (2024-01-01)
        std::fs::write(format!("{p}/auth.ts"), "export {}").unwrap();
        git(&["add", "auth.ts"]);
        commit_dated(&p, "Add authentication module", "2024-01-01T10:00:00+0000");
        let hash_a = head_hash(&p);

        // Commit B — modifies auth.ts, creates user.ts (2024-01-02)
        std::fs::write(format!("{p}/auth.ts"), "export function auth() {}").unwrap();
        std::fs::write(format!("{p}/user.ts"), "export {}").unwrap();
        git(&["add", "auth.ts", "user.ts"]);
        commit_dated(&p, "Add user model, extend auth", "2024-01-02T10:00:00+0000");
        let hash_b = head_hash(&p);

        // Commit C — modifies user.ts (2024-01-03)
        std::fs::write(format!("{p}/user.ts"), "export function getUser() {}").unwrap();
        git(&["add", "user.ts"]);
        commit_dated(&p, "Add getUser function", "2024-01-03T10:00:00+0000");
        let hash_c = head_hash(&p);

        FixtureRepo { _dir: dir, path: p, hash_a, hash_b, hash_c }
    }

    fn head_hash(repo: &str) -> String {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .expect("rev-parse");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn fixture_prs_json(merge_commit_sha: &str) -> String {
        format!(r#"[{{
            "number": 12,
            "title": "Add user authentication",
            "state": "MERGED",
            "body": "Closes #10",
            "author": {{"login": "alice"}},
            "mergeCommit": {{"oid": "{merge_commit_sha}"}},
            "closingIssuesReferences": [{{"number": 10}}]
        }}]"#)
    }

    const FIXTURE_ISSUES_JSON: &str = r#"[{
        "number": 10,
        "title": "Add user authentication",
        "state": "CLOSED",
        "body": null,
        "author": {"login": "alice"}
    }]"#;

    // ── Tests ───────────────────────────────────────────────────────────────

    #[test]
    fn ingest_git_extracts_three_commits() {
        let fixture = create_fixture_repo();
        let store = Store::open(":memory:").unwrap();

        let count = ingest_git(&fixture.path, &store).unwrap();
        assert_eq!(count, 3, "expected 3 commits");
    }

    #[test]
    fn ingest_git_correct_files_per_commit() {
        let fixture = create_fixture_repo();
        let store = Store::open(":memory:").unwrap();
        ingest_git(&fixture.path, &store).unwrap();

        // auth.ts is touched by commits A and B
        let auth = store.commits_for_file("auth.ts", &fixture.path).unwrap();
        assert_eq!(auth.len(), 2, "auth.ts should have exactly 2 commits");

        // user.ts is touched by commits B and C
        let user = store.commits_for_file("user.ts", &fixture.path).unwrap();
        assert_eq!(user.len(), 2, "user.ts should have exactly 2 commits");
    }

    #[test]
    fn ingest_git_twice_no_duplication() {
        let fixture = create_fixture_repo();
        let store = Store::open(":memory:").unwrap();
        ingest_git(&fixture.path, &store).unwrap();
        ingest_git(&fixture.path, &store).unwrap();
        assert_eq!(store.commit_count(&fixture.path).unwrap(), 3);
    }

    #[test]
    fn ingest_github_from_json_full_scenario() {
        let fixture = create_fixture_repo();
        let store = Store::open(":memory:").unwrap();

        // Ingest git so the PR→commit→file chain can be resolved.
        ingest_git(&fixture.path, &store).unwrap();

        let prs_json = fixture_prs_json(&fixture.hash_b);
        ingest_github_from_json(&prs_json, FIXTURE_ISSUES_JSON, &fixture.path, &store).unwrap();

        // PR #12 should appear for auth.ts (touched by commit B = merge commit)
        let auth_prs = store.prs_for_file("auth.ts", &fixture.path).unwrap();
        assert_eq!(auth_prs.len(), 1);
        assert_eq!(auth_prs[0].number, 12);

        // Issue #10 should be reachable from auth.ts via PR #12
        let auth_issues = store.issues_for_file("auth.ts", &fixture.path).unwrap();
        assert_eq!(auth_issues.len(), 1);
        assert_eq!(auth_issues[0].number, 10);
    }

    #[test]
    fn ingest_github_twice_no_duplication() {
        let fixture = create_fixture_repo();
        let store = Store::open(":memory:").unwrap();
        ingest_git(&fixture.path, &store).unwrap();

        let prs_json = fixture_prs_json(&fixture.hash_b);
        ingest_github_from_json(&prs_json, FIXTURE_ISSUES_JSON, &fixture.path, &store).unwrap();
        ingest_github_from_json(&prs_json, FIXTURE_ISSUES_JSON, &fixture.path, &store).unwrap();

        let auth_prs = store.prs_for_file("auth.ts", &fixture.path).unwrap();
        assert_eq!(auth_prs.len(), 1);
        let auth_issues = store.issues_for_file("auth.ts", &fixture.path).unwrap();
        assert_eq!(auth_issues.len(), 1);
    }

    #[test]
    fn malformed_github_json_returns_error_not_panic() {
        let store = Store::open(":memory:").unwrap();
        let result = ingest_github_from_json("{bad json}", "[]", ".", &store);
        assert!(result.is_err());
    }

    #[test]
    fn commit_hashes_survive_pipeline() {
        let fixture = create_fixture_repo();
        let store = Store::open(":memory:").unwrap();
        ingest_git(&fixture.path, &store).unwrap();

        // Both hash_a and hash_b should appear for auth.ts
        let auth_commits = store.commits_for_file("auth.ts", &fixture.path).unwrap();
        let short_hashes: Vec<String> = auth_commits.iter().map(|c| c.short_hash.clone()).collect();

        let expected_short_a = &fixture.hash_a[..7];
        let expected_short_b = &fixture.hash_b[..7];

        assert!(
            short_hashes.iter().any(|h| h == expected_short_a),
            "commit A short hash {} not found in {:?}", expected_short_a, short_hashes
        );
        assert!(
            short_hashes.iter().any(|h| h == expected_short_b),
            "commit B short hash {} not found in {:?}", expected_short_b, short_hashes
        );
    }

    #[test]
    fn build_context_identity_and_coverage() {
        let fixture = create_fixture_repo();
        let store   = Store::open(":memory:").unwrap();
        ingest_git(&fixture.path, &store).unwrap();

        let doc = build_context("auth.ts", &fixture.path, &store).unwrap();

        assert_eq!(doc.identity.touch_count, 2);
        assert_eq!(doc.recent_activity.len(), 2);
        assert_eq!(
            doc.identity.first_commit.as_ref().unwrap().short_hash,
            &fixture.hash_a[..7]
        );
        assert_eq!(doc.coverage.git_history, atlas_ir::CoverageStatus::Available);
        assert_eq!(doc.coverage.github_prs,  atlas_ir::CoverageStatus::NotIngested);
        assert_eq!(doc.evidence.commits, 2);
        assert_eq!(doc.evidence.prs,     0);
        assert_eq!(doc.evidence.issues,  0);
    }

    #[test]
    fn build_context_coupling_and_github() {
        let fixture = create_fixture_repo();
        let store   = Store::open(":memory:").unwrap();
        ingest_git(&fixture.path, &store).unwrap();

        let prs_json = fixture_prs_json(&fixture.hash_b);
        ingest_github_from_json(&prs_json, FIXTURE_ISSUES_JSON, &fixture.path, &store).unwrap();

        let doc = build_context("auth.ts", &fixture.path, &store).unwrap();

        // user.ts changed in the same commit as auth.ts (commit B)
        assert!(
            doc.coupling.iter().any(|e| e.file_path == "user.ts"),
            "user.ts missing from coupling: {:?}", doc.coupling
        );

        assert_eq!(doc.coverage.github_prs,    atlas_ir::CoverageStatus::Available);
        assert_eq!(doc.coverage.github_issues, atlas_ir::CoverageStatus::Available);
        assert_eq!(doc.related_history.pull_requests.len(), 1);
        assert_eq!(doc.related_history.pull_requests[0].number, 12);
        assert_eq!(doc.related_history.pull_requests[0].linked_issues, vec![10]);
        assert_eq!(doc.related_history.issues.len(), 1);
        assert_eq!(doc.related_history.issues[0].number, 10);
        assert!(doc.evidence.total_facts > 0);
    }

    #[test]
    fn build_context_unknown_file_returns_empty_not_error() {
        let fixture = create_fixture_repo();
        let store   = Store::open(":memory:").unwrap();
        ingest_git(&fixture.path, &store).unwrap();

        let doc = build_context("nonexistent.ts", &fixture.path, &store).unwrap();
        assert_eq!(doc.identity.touch_count, 0);
        assert!(doc.recent_activity.is_empty());
        assert!(doc.significance.is_none());
    }
}
