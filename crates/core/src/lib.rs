use anyhow::Result;
use atlas_connectors::Connector;
use atlas_git::{GitHubIssueConnector, GitHubPrConnector, GitRepo};
use atlas_parser::{gh_json, git_log};
use atlas_storage::Store;
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
}
