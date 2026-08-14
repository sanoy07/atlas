//! B4: `atlas authors` — verifies (author_name, author_email) aggregation
//! over `commits + commit_files` with the repo-isolation invariant, the
//! DISTINCT-commit denominator, identity-scoped (rename-safe) queries,
//! and the historical-path redirect.
//!
//! One of the ten tests below (`authors_do_not_leak_across_repositories`)
//! is a load-bearing regression that must not weaken: any change to the
//! author-aggregation SQL that drops the `commits.repo_path` filter will
//! fail it immediately.

use atlas_core::{compute_authors, AuthorsSubjectKind};
use atlas_ir::{AuthorScope, Commit};
use atlas_storage::Store;
use chrono::{DateTime, Utc};

fn store() -> Store { Store::open(":memory:").unwrap() }

/// Insert a commit with a specific author and file set into `repo`.
fn commit(
    store: &Store,
    repo:  &str,
    hash:  &str,
    ts:    i64,
    name:  &str,
    email: &str,
    files: &[&str],
) {
    let c = Commit {
        hash:          hash.into(),
        short_hash:    hash[..7.min(hash.len())].into(),
        message:       format!("commit {}", hash),
        author_name:   name.into(),
        author_email:  email.into(),
        timestamp:     DateTime::<Utc>::from_timestamp(ts, 0).unwrap(),
        files_changed: files.iter().map(|f| f.to_string()).collect(),
        parents:       vec![],
    };
    store.insert_commit(&c, repo).unwrap();
}

/// Create a materialised FileIdentity chain: `old_path` renamed to
/// `new_path` at `rename_hash`.  Both path observations belong to the
/// same identity, and both are linked to their respective touching
/// commits via `populate_identity_commits`.
fn identity_chain(
    store:       &Store,
    repo:        &str,
    old_path:    &str,
    new_path:    &str,
    intro_hash:  &str,
    rename_hash: &str,
) -> i64 {
    let id = store.insert_file_identity(repo).unwrap();
    store
        .insert_path_observation(id, old_path, intro_hash, Some(rename_hash), repo)
        .unwrap();
    store
        .insert_path_observation(id, new_path, rename_hash, None, repo)
        .unwrap();
    store.populate_identity_commits(repo).unwrap();
    id
}

// ── 1. baseline ─────────────────────────────────────────────────────────────

#[test]
fn single_author_on_a_single_file() {
    let s = store();
    commit(&s, "/repo", "aaa1111", 100, "Alice", "alice@x.com", &["src/main.rs"]);

    let r = compute_authors("src/main.rs", AuthorsSubjectKind::Auto, "/repo", &s).unwrap();

    assert_eq!(r.authors.len(), 1);
    assert_eq!(r.authors[0].author_name,  "Alice");
    assert_eq!(r.authors[0].author_email, "alice@x.com");
    assert_eq!(r.authors[0].commit_count, 1);
    assert_eq!(r.total_commits, 1);
    assert_eq!(r.total_authors, 1);
    // Auto with no subtree rows falls through to ExactFile scope.
    assert_eq!(r.scope, AuthorScope::ExactFile);
}

// ── 2. deterministic no-merge on differing email ────────────────────────────

#[test]
fn same_author_different_email_produces_two_rows() {
    let s = store();
    commit(&s, "/repo", "aaa1111", 100, "Alice", "alice@work.com",     &["src/x.rs"]);
    commit(&s, "/repo", "bbb2222", 200, "Alice", "alice@personal.com", &["src/x.rs"]);

    let r = compute_authors("src/x.rs", AuthorsSubjectKind::Auto, "/repo", &s).unwrap();

    assert_eq!(r.authors.len(), 2, "differing emails must not be merged");
    assert_eq!(r.total_authors, 2);
    // Both authors named "Alice"; distinguish by email.
    let emails: std::collections::HashSet<_> =
        r.authors.iter().map(|a| a.author_email.as_str()).collect();
    assert!(emails.contains("alice@work.com"));
    assert!(emails.contains("alice@personal.com"));
}

// ── 3. DISTINCT-commit denominator (a commit touching N files ≠ N) ─────────

#[test]
fn commits_touching_many_files_count_once() {
    let s = store();
    // One commit touching 5 files under the subtree.
    commit(
        &s, "/repo", "aaa1111", 100, "Alice", "alice@x.com",
        &["src/mod/a.rs", "src/mod/b.rs", "src/mod/c.rs", "src/mod/d.rs", "src/mod/e.rs"],
    );

    let r = compute_authors("src/mod", AuthorsSubjectKind::Auto, "/repo", &s).unwrap();

    assert_eq!(r.scope, AuthorScope::Prefix);
    assert_eq!(r.authors.len(), 1);
    assert_eq!(r.authors[0].commit_count, 1, "one commit touching 5 files must count as 1");
    assert_eq!(r.total_commits, 1);
}

// ── 4. directory subtree aggregates across multiple authors ────────────────

#[test]
fn directory_prefix_aggregates_subtree() {
    let s = store();
    commit(&s, "/repo", "a11", 100, "Alice", "alice@x.com", &["src/mod/foo.rs"]);
    commit(&s, "/repo", "b22", 200, "Bob",   "bob@y.com",   &["src/mod/bar.rs"]);
    commit(&s, "/repo", "c33", 300, "Bob",   "bob@y.com",   &["src/mod/baz.rs"]);
    commit(&s, "/repo", "d44", 400, "Eve",   "eve@z.com",   &["src/mod/qux.rs"]);
    // Sibling directory — must NOT count under `src/mod`.
    commit(&s, "/repo", "e55", 500, "Zoe",   "zoe@w.com",   &["src/other/thing.rs"]);

    let r = compute_authors("src/mod", AuthorsSubjectKind::Auto, "/repo", &s).unwrap();

    assert_eq!(r.scope, AuthorScope::Prefix);
    assert_eq!(r.total_commits, 4);
    assert_eq!(r.total_authors, 3);
    // Sort: commits DESC, then name ASC.  Bob=2, Alice=1, Eve=1.
    assert_eq!(r.authors[0].author_name, "Bob");
    assert_eq!(r.authors[0].commit_count, 2);
    assert_eq!(r.authors[1].author_name, "Alice");
    assert_eq!(r.authors[2].author_name, "Eve");
}

// ── 5. file with identity chain uses identity-scoped aggregation ───────────

#[test]
fn file_with_identity_chain_uses_identity_scope() {
    let s = store();
    // Pre-rename: Alice creates and edits the file at its old path.
    commit(&s, "/repo", "intro", 100, "Alice", "alice@x.com", &["src/old_name.rs"]);
    commit(&s, "/repo", "edit1", 200, "Alice", "alice@x.com", &["src/old_name.rs"]);
    // Rename commit (touches new path only per git log --name-status).
    commit(&s, "/repo", "renam", 300, "Bob",   "bob@y.com",   &["src/new_name.rs"]);
    // Post-rename: Eve edits at the new path.
    commit(&s, "/repo", "edit2", 400, "Eve",   "eve@z.com",   &["src/new_name.rs"]);

    identity_chain(&s, "/repo", "src/old_name.rs", "src/new_name.rs", "intro", "renam");

    // Query the *current* path — auto detects the identity chain.
    let r = compute_authors("src/new_name.rs", AuthorsSubjectKind::Auto, "/repo", &s).unwrap();

    assert_eq!(r.scope, AuthorScope::Identity, "file with an identity chain must use Identity scope");
    // All three authors must appear even though Alice never touched the current path.
    assert_eq!(r.total_authors, 3);
    let names: std::collections::HashSet<_> =
        r.authors.iter().map(|a| a.author_name.as_str()).collect();
    assert!(names.contains("Alice"), "pre-rename author must be preserved");
    assert!(names.contains("Bob"));
    assert!(names.contains("Eve"));
    // Alice's aggregate covers the two commits at the old path.
    let alice = r.authors.iter().find(|a| a.author_name == "Alice").unwrap();
    assert_eq!(alice.commit_count, 2);
}

// ── 6. historical path produces redirect note ──────────────────────────────

#[test]
fn historical_path_produces_redirect_note() {
    let s = store();
    commit(&s, "/repo", "intro", 100, "Alice", "alice@x.com", &["src/old_name.rs"]);
    commit(&s, "/repo", "renam", 200, "Bob",   "bob@y.com",   &["src/new_name.rs"]);
    identity_chain(&s, "/repo", "src/old_name.rs", "src/new_name.rs", "intro", "renam");

    // Query the OLD path — must redirect.
    let r = compute_authors("src/old_name.rs", AuthorsSubjectKind::Auto, "/repo", &s).unwrap();

    let rn = r.redirect_note.expect("historical path must populate redirect_note");
    assert_eq!(rn.original_subject, "src/old_name.rs");
    assert_eq!(rn.current_path,     "src/new_name.rs");
    // The authors from the identity chain must still be reachable.
    assert_eq!(r.scope, AuthorScope::Identity);
    assert!(r.authors.iter().any(|a| a.author_name == "Alice"));
    assert!(r.authors.iter().any(|a| a.author_name == "Bob"));
}

// ── 7. first / last touch timestamps ───────────────────────────────────────

#[test]
fn first_and_last_touch_timestamps_are_min_and_max() {
    let s = store();
    commit(&s, "/repo", "c1", 100, "Alice", "alice@x.com", &["src/x.rs"]);
    commit(&s, "/repo", "c2", 550, "Alice", "alice@x.com", &["src/x.rs"]);
    commit(&s, "/repo", "c3", 300, "Alice", "alice@x.com", &["src/x.rs"]);

    let r = compute_authors("src/x.rs", AuthorsSubjectKind::Auto, "/repo", &s).unwrap();
    let alice = &r.authors[0];
    assert_eq!(alice.first_touch, 100, "must be MIN(timestamp)");
    assert_eq!(alice.last_touch,  550, "must be MAX(timestamp)");
    assert_eq!(alice.commit_count, 3);
}

// ── 8. deterministic sort: commits DESC, then name ASC ─────────────────────

#[test]
fn sort_order_is_commits_desc_then_name_asc() {
    let s = store();
    // Two authors tie at 1 commit each — name must break the tie.
    commit(&s, "/repo", "a1", 100, "Charlie", "c@x.com", &["src/y.rs"]);
    commit(&s, "/repo", "b1", 200, "Alice",   "a@x.com", &["src/y.rs"]);
    // Bob has 2 commits and must come first.
    commit(&s, "/repo", "c1", 300, "Bob",     "b@x.com", &["src/y.rs"]);
    commit(&s, "/repo", "d1", 400, "Bob",     "b@x.com", &["src/y.rs"]);

    let r = compute_authors("src/y.rs", AuthorsSubjectKind::Auto, "/repo", &s).unwrap();
    assert_eq!(r.authors[0].author_name, "Bob");
    assert_eq!(r.authors[0].commit_count, 2);
    // Tie-break: Alice before Charlie.
    assert_eq!(r.authors[1].author_name, "Alice");
    assert_eq!(r.authors[2].author_name, "Charlie");
}

// ── 9. LOAD-BEARING: repo isolation ────────────────────────────────────────
//
// The user's carry-over note: "Any aggregation over commits/commit_files
// must preserve the same repo-isolation invariant."  This test enforces
// that invariant end-to-end — same DB, same file path, different repos,
// disjoint authors.

#[test]
fn authors_do_not_leak_across_repositories() {
    let s = store();
    // Both repos have a commit at the same path `src/shared.rs`.  If any
    // SQL in the authors path forgets to filter by `commits.repo_path`,
    // the query for repo A will surface the author from repo B.
    commit(&s, "/repo/a", "a-1111", 100, "Alice-A", "a@a.com", &["src/shared.rs"]);
    commit(&s, "/repo/b", "b-2222", 200, "Bob-B",   "b@b.com", &["src/shared.rs"]);

    let ra = compute_authors("src/shared.rs", AuthorsSubjectKind::Auto, "/repo/a", &s).unwrap();
    assert_eq!(ra.authors.len(), 1, "repo A must not see repo B authors");
    assert_eq!(ra.authors[0].author_name, "Alice-A");

    let rb = compute_authors("src/shared.rs", AuthorsSubjectKind::Auto, "/repo/b", &s).unwrap();
    assert_eq!(rb.authors.len(), 1, "repo B must not see repo A authors");
    assert_eq!(rb.authors[0].author_name, "Bob-B");

    // And the directory prefix path is subject to the same invariant.
    let ra_dir = compute_authors("src", AuthorsSubjectKind::Auto, "/repo/a", &s).unwrap();
    assert_eq!(ra_dir.authors.len(), 1);
    assert_eq!(ra_dir.authors[0].author_name, "Alice-A");
}

// ── 10. exact-file scope does not over-match sibling prefixes ──────────────
//
// `src/foo.rs` must not match `src/foo.rs.bak`.  Prevents the classic
// `LIKE 'path%'` over-match bug for exact-file scope.

#[test]
fn exact_file_scope_does_not_over_match_prefix_siblings() {
    let s = store();
    commit(&s, "/repo", "a11", 100, "Alice", "a@x.com", &["src/foo.rs"]);
    commit(&s, "/repo", "b22", 200, "Bob",   "b@y.com", &["src/foo.rs.bak"]);

    // No identity chain — auto with no subtree rows lands on ExactFile.
    let r = compute_authors("src/foo.rs", AuthorsSubjectKind::Auto, "/repo", &s).unwrap();
    assert_eq!(r.scope, AuthorScope::ExactFile);
    assert_eq!(r.authors.len(), 1, "exact-file scope must not match sibling `.bak`");
    assert_eq!(r.authors[0].author_name, "Alice");
}

// ── 11. repo root subject aggregates every commit ──────────────────────────

#[test]
fn repo_root_subject_aggregates_every_commit_in_the_repo() {
    let s = store();
    commit(&s, "/repo", "a11", 100, "Alice", "a@x.com", &["src/foo.rs"]);
    commit(&s, "/repo", "b22", 200, "Bob",   "b@y.com", &["docs/README.md"]);
    commit(&s, "/repo", "c33", 300, "Eve",   "e@z.com", &["Cargo.toml"]);

    let r = compute_authors(".", AuthorsSubjectKind::Auto, "/repo", &s).unwrap();
    assert_eq!(r.scope, AuthorScope::Prefix);
    assert_eq!(r.total_commits, 3);
    assert_eq!(r.total_authors, 3);
}

// ── 12. no commits observed → empty report, correct scope, no panic ────────

#[test]
fn subject_with_no_touching_commits_returns_empty_report() {
    let s = store();
    commit(&s, "/repo", "a11", 100, "Alice", "a@x.com", &["src/foo.rs"]);

    let r = compute_authors("nonexistent/path.rs", AuthorsSubjectKind::Auto, "/repo", &s).unwrap();
    assert_eq!(r.authors.len(), 0);
    assert_eq!(r.total_commits, 0);
    assert_eq!(r.total_authors, 0);
    // Auto with no identity and no subtree rows falls through to ExactFile.
    assert_eq!(r.scope, AuthorScope::ExactFile);
}
