//! B3: `atlas show` — verifies subject resolution + linked-record hooks.
//!
//! Tests that each subject kind returns the correct concrete row and
//! that every emitted link `token` round-trips back into `show`.

use atlas_core::{show, ShowOptions, ShowSubjectKind};
use atlas_ir::{
    Commit, Issue, PullRequest, ShowLink, ShowRecord, ShowSubject, StructuralEdge,
    StructuralEdgeKind, StructuralEvidence,
};
use atlas_storage::Store;
use chrono::{DateTime, Utc};

fn store() -> Store {
    Store::open(":memory:").unwrap()
}

fn commit(store: &Store, repo: &str, hash: &str, msg: &str, ts: i64, files: &[&str]) {
    let c = Commit {
        hash:          hash.into(),
        short_hash:    hash[..7.min(hash.len())].into(),
        message:       msg.into(),
        author_name:   "Alice".into(),
        author_email:  "alice@example.com".into(),
        timestamp:     DateTime::<Utc>::from_timestamp(ts, 0).unwrap(),
        files_changed: files.iter().map(|f| f.to_string()).collect(),
        parents:       vec![],
    };
    store.insert_commit(&c, repo).unwrap();
}

fn commit_with_parents(store: &Store, repo: &str, hash: &str, msg: &str, ts: i64,
                       files: &[&str], parents: &[&str]) {
    let c = Commit {
        hash:          hash.into(),
        short_hash:    hash[..7.min(hash.len())].into(),
        message:       msg.into(),
        author_name:   "Alice".into(),
        author_email:  "alice@example.com".into(),
        timestamp:     DateTime::<Utc>::from_timestamp(ts, 0).unwrap(),
        files_changed: files.iter().map(|f| f.to_string()).collect(),
        parents:       parents.iter().map(|s| s.to_string()).collect(),
    };
    store.insert_commit(&c, repo).unwrap();
}

fn pr(store: &Store, repo: &str, number: i64, title: &str, state: &str, merge_sha: Option<&str>) {
    let pr = PullRequest {
        number,
        title:            title.into(),
        state:            state.into(),
        body:             Some("body text".into()),
        author:           "bob".into(),
        merge_commit_sha: merge_sha.map(|s| s.to_string()),
        created_at:       Some(DateTime::from_timestamp(1_700_000_000, 0).unwrap()),
        merged_at:        Some(DateTime::from_timestamp(1_700_000_100, 0).unwrap()),
    };
    store.insert_pull_request(&pr, repo).unwrap();
}

fn issue(store: &Store, repo: &str, number: i64, title: &str, state: &str) {
    let is = Issue {
        number,
        title:      title.into(),
        state:      state.into(),
        body:       Some("issue body".into()),
        author:     "eve".into(),
        created_at: Some(DateTime::from_timestamp(1_700_000_000, 0).unwrap()),
    };
    store.insert_issue(&is, repo).unwrap();
}

fn edge(store: &Store, repo: &str, source: &str, target: &str, kind: StructuralEdgeKind) {
    let e = StructuralEdge {
        source_file:   source.into(),
        source_symbol: None,
        target_file:   target.into(),
        target_symbol: None,
        kind,
        evidence:      StructuralEvidence {
            source_file: source.into(),
            line:        Some(1),
            snippet:     "e".into(),
            extractor:   "test".into(),
        },
    };
    store.insert_structural_edge(&e, repo).unwrap();
}

fn tokens(r: &ShowRecord) -> Vec<String> {
    r.links.iter().map(|l: &ShowLink| l.token.clone()).collect()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn commit_by_full_hash_resolves_with_parents_and_files() {
    let s = store();
    commit(&s, "r", "0000aaaa1111bbbb2222cccc3333dddd4444eeee",
        "root", 1, &["src/a.ts"]);
    commit_with_parents(&s, "r", "0000aaaa1111bbbb2222cccc3333dddd4444ffff",
        "second", 2, &["src/b.ts"],
        &["0000aaaa1111bbbb2222cccc3333dddd4444eeee"]);

    let r = show("0000aaaa1111bbbb2222cccc3333dddd4444ffff", "r", &s, ShowOptions::default()).unwrap();
    match &r.subject {
        ShowSubject::Commit(c) => {
            assert_eq!(c.short_hash, "0000aaa");
            assert_eq!(c.message, "second");
        }
        _ => panic!("expected commit subject"),
    }
    // Parent section present with 1 row.
    let parents = r.sections.iter().find(|s| s.title == "PARENTS").expect("parents section");
    assert_eq!(parents.rows.len(), 1);
    assert_eq!(parents.rows[0].display, "0000aaaa1111bbbb2222cccc3333dddd4444eeee");
    // Changed files section present.
    let files = r.sections.iter().find(|s| s.title == "CHANGED FILES").expect("files section");
    assert_eq!(files.rows.len(), 1);
    assert_eq!(files.rows[0].display, "src/b.ts");
    // Both link tokens should surface at top-level.
    let toks = tokens(&r);
    assert!(toks.iter().any(|t| t == "0000aaaa1111bbbb2222cccc3333dddd4444eeee"),
        "parent commit must be a link token");
    assert!(toks.iter().any(|t| t == "src/b.ts"),
        "changed file must be a link token");
}

#[test]
fn commit_by_short_prefix_resolves_when_unique() {
    let s = store();
    commit(&s, "r", "abcd0000111122223333444455556666aaaabbbb", "unique", 1, &[]);
    let r = show("abcd000", "r", &s, ShowOptions::default()).unwrap();
    match r.subject {
        ShowSubject::Commit(c) => assert!(c.hash.starts_with("abcd000")),
        _ => panic!("expected commit"),
    }
}

#[test]
fn commit_prefix_ambiguity_errors_with_candidates() {
    let s = store();
    commit(&s, "r", "abcd0000111122223333444455556666aaaabbbb", "one", 1, &[]);
    commit(&s, "r", "abcd0000ffffeeeeddddccccbbbbaaaa99998888", "two", 2, &[]);
    let err = show("abcd0000", "r", &s, ShowOptions::default()).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("ambiguous"), "expected ambiguity error, got: {}", msg);
    assert!(msg.contains("abcd0000"), "error should mention the input prefix");
}

#[test]
fn commit_shows_linked_pr_via_merge_commit_sha() {
    let s = store();
    let sha = "aaaa1111bbbb2222cccc3333dddd4444eeee5555";
    commit(&s, "r", sha, "merge commit", 1, &["src/x.ts"]);
    pr(&s, "r", 42, "add feature", "MERGED", Some(sha));

    let r = show(sha, "r", &s, ShowOptions::default()).unwrap();
    let prs = r.sections.iter().find(|sec| sec.title == "LINKED PULL REQUESTS").unwrap();
    assert_eq!(prs.rows.len(), 1);
    let link = prs.rows[0].link.as_ref().expect("PR row must carry a link");
    assert_eq!(link.token, "pr#42");
    assert_eq!(link.kind,  "pr");
}

/// C4-B: commit message `(#N)` resolves to an ingested PR even when merge_commit_sha is missing.
#[test]
fn commit_shows_linked_pr_via_message_ref() {
    let s = store();
    let sha = "bbbb2222cccc3333dddd4444eeee5555ffff6666";
    commit(&s, "r", sha, "feat(orders): quoteOrder (#134)", 2, &["src/order.ts"]);
    // PR exists but merge_commit_sha does not match this commit (common incomplete GitHub data).
    pr(&s, "r", 134, "quote order", "MERGED", None);

    let r = show(sha, "r", &s, ShowOptions::default()).unwrap();
    let prs = r.sections.iter().find(|sec| sec.title == "LINKED PULL REQUESTS").unwrap();
    assert_eq!(prs.rows.len(), 1);
    let link = prs.rows[0].link.as_ref().expect("PR from message ref");
    assert_eq!(link.token, "pr#134");
    assert!(
        prs.rows[0].display.contains("commit_message"),
        "expected via commit_message provenance, got: {}",
        prs.rows[0].display
    );
}

#[test]
fn commit_shows_linked_issue_via_message_ref() {
    let s = store();
    let sha = "cccc3333dddd4444eeee5555ffff6666aaaa7777";
    commit(&s, "r", sha, "fix: timeout (#19)", 3, &["src/x.ts"]);
    issue(&s, "r", 19, "Configure Redis Timeout", "CLOSED");

    let r = show(sha, "r", &s, ShowOptions::default()).unwrap();
    let issues = r
        .sections
        .iter()
        .find(|sec| sec.title == "LINKED ISSUES (from commit message)")
        .expect("message-ref issue section");
    assert_eq!(issues.rows.len(), 1);
    assert_eq!(
        issues.rows[0].link.as_ref().unwrap().token,
        "issue#19"
    );
}

#[test]
fn pr_shows_linked_issues_via_pr_issues() {
    let s = store();
    pr(&s, "r", 10, "closes stuff", "MERGED", None);
    issue(&s, "r", 5, "The Bug", "CLOSED");
    s.link_pr_to_issue(10, 5, "r").unwrap();

    let r = show("pr#10", "r", &s, ShowOptions::default()).unwrap();
    match &r.subject {
        ShowSubject::Pr(p) => assert_eq!(p.number, 10),
        _ => panic!("expected pr subject"),
    }
    let issues = r.sections.iter().find(|sec| sec.title == "LINKED ISSUES").unwrap();
    assert_eq!(issues.rows.len(), 1);
    let link = issues.rows[0].link.as_ref().unwrap();
    assert_eq!(link.token, "issue#5");
    assert_eq!(link.kind,  "issue");
}

#[test]
fn issue_shows_reverse_pr_links() {
    let s = store();
    pr(&s, "r", 99, "fixes it", "MERGED", None);
    issue(&s, "r", 55, "Some issue", "OPEN");
    s.link_pr_to_issue(99, 55, "r").unwrap();

    let r = show("issue#55", "r", &s, ShowOptions::default()).unwrap();
    let closes = r.sections.iter().find(|sec| sec.title == "CLOSING PULL REQUESTS").unwrap();
    assert_eq!(closes.rows.len(), 1);
    assert_eq!(closes.rows[0].link.as_ref().unwrap().token, "pr#99");
}

#[test]
fn file_shows_structural_edges_both_directions() {
    let s = store();
    let repo = "r";
    commit(&s, repo, "aaaa1111bbbb2222cccc3333dddd4444eeee5555",
        "add", 1, &["src/svc.ts", "src/model.ts", "src/consumer.ts"]);
    edge(&s, repo, "src/svc.ts",      "src/model.ts",   StructuralEdgeKind::Imports);
    edge(&s, repo, "src/consumer.ts", "src/svc.ts",     StructuralEdgeKind::CallsStatic);

    let r = show("src/svc.ts", repo, &s, ShowOptions::default()).unwrap();
    let out = r.sections.iter().find(|sec| sec.title == "STRUCTURAL EDGES (outgoing)").unwrap();
    assert_eq!(out.rows.len(), 1);
    assert!(out.rows[0].display.contains("src/model.ts"));
    let in_ = r.sections.iter().find(|sec| sec.title == "STRUCTURAL EDGES (incoming)").unwrap();
    assert_eq!(in_.rows.len(), 1);
    assert!(in_.rows[0].display.contains("src/consumer.ts"));

    let toks = tokens(&r);
    assert!(toks.iter().any(|t| t == "src/model.ts"));
    assert!(toks.iter().any(|t| t == "src/consumer.ts"));
}

#[test]
fn file_historical_path_produces_redirect_note() {
    let s = store();
    let repo = "r";
    let id = s.insert_file_identity(repo).unwrap();
    s.insert_path_observation(id, "old/svc.ts", "commit_a_00000000000000000000000000", None, repo).unwrap();
    s.supersede_path_observation(id, "old/svc.ts", "commit_b_00000000000000000000000000", repo).unwrap();
    s.insert_path_observation(id, "new/svc.ts", "commit_b_00000000000000000000000000", None, repo).unwrap();

    let r = show("old/svc.ts", repo, &s, ShowOptions::default()).unwrap();
    let rd = r.redirect_note.as_ref().expect("historical redirect note expected");
    assert_eq!(rd.original_subject, "old/svc.ts");
    assert_eq!(rd.current_path,     "new/svc.ts");
    match &r.subject {
        ShowSubject::File(f) => {
            assert_eq!(f.relative_path, "new/svc.ts");
            assert_eq!(f.identity_id, Some(id));
        }
        _ => panic!("expected file subject"),
    }
    // Lineage section shows both observations.
    let lineage = r.sections.iter().find(|sec| sec.title == "IDENTITY LINEAGE").unwrap();
    assert_eq!(lineage.rows.len(), 2);
}

#[test]
fn identity_subject_shows_full_lineage_and_commit_membership() {
    let s = store();
    let repo = "r";
    let id = s.insert_file_identity(repo).unwrap();
    s.insert_path_observation(id, "a.ts", "aaaa1111bbbb2222cccc3333dddd4444eeee5555", None, repo).unwrap();
    // Register a commit + attach to identity.
    commit(&s, repo, "aaaa1111bbbb2222cccc3333dddd4444eeee5555",
        "create a", 1, &["a.ts"]);
    s.insert_file_identity_commit(id, "aaaa1111bbbb2222cccc3333dddd4444eeee5555", repo).unwrap();

    let r = show(&format!("id:{}", id), repo, &s, ShowOptions::default()).unwrap();
    match &r.subject {
        ShowSubject::Identity(i) => {
            assert_eq!(i.identity_id, id);
            assert_eq!(i.path_history_count, 1);
            assert_eq!(i.commit_count, 1);
            assert_eq!(i.current_path, Some("a.ts".to_string()));
        }
        _ => panic!("expected identity"),
    }
    let commits = r.sections.iter().find(|sec| sec.title == "COMMITS").unwrap();
    assert_eq!(commits.rows.len(), 1);
    assert!(commits.rows[0].link.is_some());
}

#[test]
fn document_shows_body_truncated_by_default() {
    let s = store();
    let repo = "r";
    let long_body = "a".repeat(5_000);
    s.insert_document("docs/big.md", "doc", "Big", &long_body, repo).unwrap();

    let r = show("doc:docs/big.md", repo, &s, ShowOptions::default()).unwrap();
    match &r.subject {
        ShowSubject::Document(d) => {
            assert_eq!(d.file_path, "docs/big.md");
            assert_eq!(d.body_bytes, 5_000);
            assert!(d.body_excerpt.len() < d.body_bytes,
                "body must be truncated when not --full");
        }
        _ => panic!("expected document"),
    }

    // --full disables truncation.
    let mut opts = ShowOptions::default();
    opts.full = true;
    let r = show("doc:docs/big.md", repo, &s, opts).unwrap();
    match r.subject {
        ShowSubject::Document(d) => assert_eq!(d.body_excerpt.len(), d.body_bytes),
        _ => panic!("expected document"),
    }
}

#[test]
fn config_artifact_shows_raw_content_and_sha() {
    let s = store();
    let repo = "r";
    s.insert_configuration_artifact(repo, "package.json", "package_json",
        r#"{"name":"x"}"#, "deadbeef").unwrap();

    let r = show("config:package.json", repo, &s, ShowOptions::default()).unwrap();
    match &r.subject {
        ShowSubject::ConfigArtifact(c) => {
            assert_eq!(c.file_path, "package.json");
            assert_eq!(c.artifact_kind, "package_json");
            assert_eq!(c.sha256, "deadbeef");
            assert!(c.body_excerpt.contains("\"name\":\"x\""));
        }
        _ => panic!("expected config artifact"),
    }
}

#[test]
fn ingest_run_by_id_shows_stages_and_git_head() {
    let s = store();
    let repo = "r";
    let id = s.start_ingest_run(repo, "0.1.0",
        Some("aaaa1111bbbb2222cccc3333dddd4444eeee5555"),
        Some("main"), "head_only").unwrap();
    let stages = r#"[{"stage":"git history","status":"ok","detail":"1 commits"}]"#;
    s.finish_ingest_run(id, "ok", stages, "[]").unwrap();

    let r = show(&format!("run:{}", id), repo, &s, ShowOptions::default()).unwrap();
    match &r.subject {
        ShowSubject::IngestRun(run) => {
            assert_eq!(run.id, id);
            assert_eq!(run.exit_status, "ok");
            assert_eq!(run.git_head.as_deref(), Some("aaaa1111bbbb2222cccc3333dddd4444eeee5555"));
        }
        _ => panic!("expected ingest run"),
    }
    // GIT HEAD section carries a commit link.
    let head = r.sections.iter().find(|sec| sec.title == "GIT HEAD AT INGEST").unwrap();
    assert_eq!(head.rows.len(), 1);
    assert_eq!(head.rows[0].link.as_ref().unwrap().kind, "commit");
    // STAGES section present with parsed row.
    let st = r.sections.iter().find(|sec| sec.title == "STAGES").unwrap();
    assert_eq!(st.rows.len(), 1);
    assert!(st.rows[0].display.contains("git history"));
}

#[test]
fn ingest_run_latest_alias_resolves() {
    let s = store();
    let repo = "r";
    let id = s.start_ingest_run(repo, "0.1.0", None, None, "head_only").unwrap();
    s.finish_ingest_run(id, "ok", "[]", "[]").unwrap();

    let r = show("run:latest", repo, &s, ShowOptions::default()).unwrap();
    match r.subject {
        ShowSubject::IngestRun(run) => assert_eq!(run.id, id),
        _ => panic!("expected ingest run"),
    }
}

// ── Auto-fallback vs. explicit-kind failure ───────────────────────────────
//
// Split into two unambiguous tests (was one muddled `unwrap_err → then
// unwrap_or_else` test that contradicted itself).

#[test]
fn auto_unknown_path_returns_empty_file_subject() {
    // Documented behaviour: `atlas show <arbitrary string>` in Auto mode
    // falls through to file lookup and returns an empty file record.
    // Intentional — a bare path is a valid file query.
    let s = store();
    let r = show("gibberish-not-a-thing", "r", &s, ShowOptions::default())
        .expect("Auto mode falls back to file lookup without erroring");
    match r.subject {
        ShowSubject::File(f) => {
            assert_eq!(f.relative_path, "gibberish-not-a-thing");
            assert!(f.identity_id.is_none());
            assert!(f.analysis_status.is_none());
        }
        _ => panic!("expected file fallback for unresolved subject"),
    }
}

#[test]
fn explicit_kind_commit_errors_when_hash_not_found() {
    let s = store();
    let mut opts = ShowOptions::default();
    opts.kind = ShowSubjectKind::Commit;
    let err = show("deadbeef", "r", &s, opts).unwrap_err();
    assert!(format!("{}", err).contains("no commit"),
        "explicit --kind commit must error on missing hash");
}

#[test]
fn explicit_kind_pr_errors_when_number_not_found() {
    let s = store();
    let mut opts = ShowOptions::default();
    opts.kind = ShowSubjectKind::Pr;
    let err = show("999", "r", &s, opts).unwrap_err();
    assert!(format!("{}", err).contains("PR #999"), "got: {}", err);
}

#[test]
fn explicit_kind_issue_errors_when_number_not_found() {
    let s = store();
    let mut opts = ShowOptions::default();
    opts.kind = ShowSubjectKind::Issue;
    let err = show("42", "r", &s, opts).unwrap_err();
    assert!(format!("{}", err).contains("issue #42"), "got: {}", err);
}

#[test]
fn explicit_kind_identity_errors_when_id_not_found() {
    let s = store();
    let mut opts = ShowOptions::default();
    opts.kind = ShowSubjectKind::Identity;
    let err = show("999", "r", &s, opts).unwrap_err();
    assert!(format!("{}", err).contains("identity 999"), "got: {}", err);
}

#[test]
fn explicit_kind_config_errors_when_artifact_not_found() {
    let s = store();
    let mut opts = ShowOptions::default();
    opts.kind = ShowSubjectKind::Config;
    let err = show("nope.json", "r", &s, opts).unwrap_err();
    assert!(format!("{}", err).contains("configuration artifact"), "got: {}", err);
}

#[test]
fn explicit_kind_document_errors_when_document_not_found() {
    let s = store();
    let mut opts = ShowOptions::default();
    opts.kind = ShowSubjectKind::Document;
    let err = show("nope.md", "r", &s, opts).unwrap_err();
    assert!(format!("{}", err).contains("document"), "got: {}", err);
}

#[test]
fn explicit_kind_run_errors_when_run_not_found() {
    let s = store();
    let mut opts = ShowOptions::default();
    opts.kind = ShowSubjectKind::Run;
    let err = show("999", "r", &s, opts).unwrap_err();
    assert!(format!("{}", err).contains("ingest run 999"), "got: {}", err);
}

// ── Repo-isolation regressions (B3 repair items 1 & 2) ────────────────────

#[test]
fn run_by_id_does_not_leak_across_repositories() {
    // Two repos share one DB.  A run id belonging to repo A must not be
    // returned when the caller queries under repo B — even when the id is
    // a globally valid rowid.
    let s = store();
    let run_a = s.start_ingest_run("repo/a", "0.1.0", None, None, "head_only").unwrap();
    s.finish_ingest_run(run_a, "ok", "[]", "[]").unwrap();
    let run_b = s.start_ingest_run("repo/b", "0.1.0", None, None, "head_only").unwrap();
    s.finish_ingest_run(run_b, "ok", "[]", "[]").unwrap();

    // Show run_a's id under repo/b — MUST fail.
    let err = show(&format!("run:{}", run_a), "repo/b", &s, ShowOptions::default()).unwrap_err();
    assert!(format!("{}", err).contains("not found in this repository"),
        "cross-repo run lookup must be rejected; got: {}", err);

    // Show run_a under its own repo — MUST succeed.
    let r = show(&format!("run:{}", run_a), "repo/a", &s, ShowOptions::default()).unwrap();
    match r.subject { ShowSubject::IngestRun(_) => (), _ => panic!("expected run subject") }
}

#[test]
fn commit_changed_files_does_not_leak_across_repositories() {
    // Same commit hash under two repos.  `commits.hash` is PK, so INSERT
    // OR IGNORE means only the first insert (repo/a) owns the row.  The
    // isolation invariant then becomes: `atlas show <hash>` under repo/b
    // must NOT return repo/a's Commit subject, and repo/a's `CHANGED FILES`
    // must NOT contain files from any repo/b insert.
    let s = store();
    let sha = "aaaa1111bbbb2222cccc3333dddd4444eeee5555";
    commit(&s, "repo/a", sha, "in a", 1, &["a/file.ts"]);
    commit(&s, "repo/b", sha, "in b", 2, &["b/file.ts"]);

    // Under repo/b, auto-detection MUST NOT surface repo/a's commit.
    // (Auto-mode falls through to the file lookup — that's documented
    // and covered by `auto_unknown_path_returns_empty_file_subject`.)
    let r = show(sha, "repo/b", &s, ShowOptions::default())
        .expect("auto-fallback does not error");
    assert!(!matches!(r.subject, ShowSubject::Commit(_)),
        "repo/a's commit must NOT leak into repo/b's `atlas show`");

    // Under repo/a, the commit resolves and files are repo-scoped —
    // repo/b's file must not appear.
    let r = show(sha, "repo/a", &s, ShowOptions::default()).unwrap();
    let files = r.sections.iter().find(|s| s.title == "CHANGED FILES").unwrap();
    assert!(files.rows.iter().any(|row| row.display == "a/file.ts"),
        "repo/a's own file must appear");
    assert!(!files.rows.iter().any(|row| row.display == "b/file.ts"),
        "repo/b's file must NOT appear under repo/a's commit view");
}

// ── PR/issue --full behaviour (B3 repair item 3) ──────────────────────────

#[test]
fn pr_body_truncated_by_default_and_full_when_requested() {
    let s = store();
    let repo = "r";
    // Insert a PR with a body longer than the default excerpt.
    let long_body = "P".repeat(5_000);
    let pr_row = PullRequest {
        number:           1,
        title:            "big".into(),
        state:            "MERGED".into(),
        body:             Some(long_body.clone()),
        author:           "eve".into(),
        merge_commit_sha: None,
        created_at:       None,
        merged_at:        None,
    };
    s.insert_pull_request(&pr_row, repo).unwrap();

    // Default: truncated.
    let r = show("pr#1", repo, &s, ShowOptions::default()).unwrap();
    match r.subject {
        ShowSubject::Pr(p) => {
            assert!(p.body_excerpt.len() < long_body.len(),
                "default must truncate PR body; got {} chars", p.body_excerpt.len());
        }
        _ => panic!("expected PR"),
    }

    // --full: complete body preserved.
    let mut opts = ShowOptions::default();
    opts.full = true;
    let r = show("pr#1", repo, &s, opts).unwrap();
    match r.subject {
        ShowSubject::Pr(p) => {
            assert_eq!(p.body_excerpt, long_body,
                "--full must return the entire PR body");
        }
        _ => panic!("expected PR"),
    }
}

#[test]
fn issue_body_truncated_by_default_and_full_when_requested() {
    let s = store();
    let repo = "r";
    let long_body = "I".repeat(5_000);
    let is = Issue {
        number:     7,
        title:      "big issue".into(),
        state:      "OPEN".into(),
        body:       Some(long_body.clone()),
        author:     "alice".into(),
        created_at: DateTime::<Utc>::from_timestamp(1_700_000_000, 0),
    };
    s.insert_issue(&is, repo).unwrap();

    let r = show("issue#7", repo, &s, ShowOptions::default()).unwrap();
    match r.subject {
        ShowSubject::Issue(i) => {
            assert!(i.body_excerpt.len() < long_body.len(),
                "default must truncate issue body");
        }
        _ => panic!("expected issue"),
    }

    let mut opts = ShowOptions::default();
    opts.full = true;
    let r = show("issue#7", repo, &s, opts).unwrap();
    match r.subject {
        ShowSubject::Issue(i) => {
            assert_eq!(i.body_excerpt, long_body, "--full must return full issue body");
        }
        _ => panic!("expected issue"),
    }
}

// ── Issue metadata is populated from the real column set (B3 repair 4) ────

#[test]
fn issue_subject_populates_author_and_created_at() {
    let s = store();
    let repo = "r";
    let is = Issue {
        number:     42,
        title:      "The Bug".into(),
        state:      "CLOSED".into(),
        body:       Some("what happened".into()),
        author:     "octocat".into(),
        created_at: DateTime::<Utc>::from_timestamp(1_700_000_000, 0),
    };
    s.insert_issue(&is, repo).unwrap();

    let r = show("issue#42", repo, &s, ShowOptions::default()).unwrap();
    match r.subject {
        ShowSubject::Issue(i) => {
            assert_eq!(i.author, "octocat",
                "author must come from the issues row — not silently blank");
            assert_eq!(i.created_at, Some(1_700_000_000),
                "created_at must come from the issues row — not silently None");
        }
        _ => panic!("expected issue"),
    }
}

// ── Section provenance (B3 repair item 5) ─────────────────────────────────

#[test]
fn linked_issues_section_on_pr_is_marked_derived() {
    // The section joins `pr_issues` (for numbers) with `issues` (for titles).
    // Two tables → per ShowSectionKind's contract, kind must be Derived.
    let s = store();
    let repo = "r";
    pr(&s, repo, 100, "T", "MERGED", None);
    issue(&s, repo, 200, "I", "OPEN");
    s.link_pr_to_issue(100, 200, repo).unwrap();

    let r = show("pr#100", repo, &s, ShowOptions::default()).unwrap();
    let sec = r.sections.iter().find(|s| s.title == "LINKED ISSUES").expect("section");
    assert_eq!(sec.kind, atlas_ir::ShowSectionKind::Derived,
        "LINKED ISSUES combines pr_issues + issues — must be Derived");
    assert!(sec.provenance_table.contains("pr_issues"));
    assert!(sec.provenance_table.contains("issues"),
        "provenance_table must name both tables when the section joins them");
}

#[test]
fn every_link_token_round_trips_for_commit_subject() {
    let s = store();
    let repo = "r";
    let sha = "aaaa1111bbbb2222cccc3333dddd4444eeee5555";
    commit(&s, repo, sha, "with pr", 1, &["src/a.ts"]);
    pr(&s, repo, 7, "T", "MERGED", Some(sha));

    let r = show(sha, repo, &s, ShowOptions::default()).unwrap();
    for link in &r.links {
        let re_resolved = show(&link.token, repo, &s, ShowOptions::default())
            .unwrap_or_else(|e| panic!("token `{}` failed to round-trip: {}", link.token, e));
        // The re-resolved subject kind should match the link.kind.
        let observed = match re_resolved.subject {
            ShowSubject::Commit(_)         => "commit",
            ShowSubject::Pr(_)             => "pr",
            ShowSubject::Issue(_)          => "issue",
            ShowSubject::File(_)           => "file",
            ShowSubject::Identity(_)       => "identity",
            ShowSubject::Document(_)       => "document",
            ShowSubject::ConfigArtifact(_) => "config",
            ShowSubject::IngestRun(_)      => "run",
        };
        assert_eq!(observed, link.kind,
            "token `{}` labelled as `{}` but resolved to `{}`",
            link.token, link.kind, observed);
    }
}
