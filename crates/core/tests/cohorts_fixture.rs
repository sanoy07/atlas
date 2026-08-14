//! B8: directory co-change cohorts.

use atlas_core::compute_directory_cohorts;
use atlas_ir::{Commit, EvidenceClass};
use atlas_storage::Store;
use chrono::{DateTime, Utc};

fn store() -> Store {
    Store::open(":memory:").unwrap()
}

fn commit(s: &Store, repo: &str, hash: &str, ts: i64, files: &[&str]) {
    let c = Commit {
        hash: hash.into(),
        short_hash: hash[..7.min(hash.len())].into(),
        message: format!("c {}", hash),
        author_name: "T".into(),
        author_email: "t@x".into(),
        timestamp: DateTime::<Utc>::from_timestamp(ts, 0).unwrap(),
        files_changed: files.iter().map(|f| f.to_string()).collect(),
        parents: vec![],
    };
    s.insert_commit(&c, repo).unwrap();
}

#[test]
fn obvious_cochange_pair_forms_cohort() {
    let s = store();
    // A and B co-change twice; C alone.
    commit(
        &s,
        "/r",
        "c1aaaaa",
        100,
        &["src/modules/a/x.ts", "src/modules/b/y.ts"],
    );
    commit(
        &s,
        "/r",
        "c2bbbbb",
        200,
        &["src/modules/a/x.ts", "src/modules/b/z.ts"],
    );
    commit(&s, "/r", "c3ccccc", 300, &["src/modules/c/w.ts"]);

    let r = compute_directory_cohorts("src/modules", Some(2), "/r", &s).unwrap();
    assert!(r.directories.contains(&"a".into()));
    assert!(r.directories.contains(&"b".into()));
    assert!(r.directories.contains(&"c".into()));

    let ab = r
        .pairs
        .iter()
        .find(|p| p.directory_a == "a" && p.directory_b == "b")
        .expect("a×b pair");
    assert_eq!(ab.cochange_commit_count, 2);
    assert_eq!(ab.evidence_class, EvidenceClass::Deterministic);

    assert_eq!(r.cohorts.len(), 1);
    assert_eq!(r.cohorts[0].members, vec!["a".to_string(), "b".to_string()]);
    assert!(r.singletons.contains(&"c".to_string()));
}

#[test]
fn threshold_filters_pairs_from_cohort_graph() {
    let s = store();
    commit(
        &s,
        "/r",
        "c1aaaaa",
        100,
        &["src/modules/a/x.ts", "src/modules/b/y.ts"],
    );
    // only one co-change — below default threshold 2
    let r = compute_directory_cohorts("src/modules", Some(2), "/r", &s).unwrap();
    assert_eq!(r.pairs[0].cochange_commit_count, 1);
    assert!(r.cohorts.is_empty());
    assert_eq!(r.singletons.len(), 2);
}

#[test]
fn unrelated_directories_not_paired() {
    let s = store();
    commit(&s, "/r", "c1aaaaa", 100, &["src/modules/a/x.ts"]);
    commit(&s, "/r", "c2bbbbb", 200, &["src/modules/b/y.ts"]);
    let r = compute_directory_cohorts("src/modules", Some(1), "/r", &s).unwrap();
    assert!(r.pairs.is_empty());
}

#[test]
fn pairs_sorted_by_count_desc_then_names() {
    let s = store();
    for i in 0..3 {
        commit(
            &s,
            "/r",
            &format!("ab{:05}", i),
            100 + i,
            &["src/modules/a/x.ts", "src/modules/b/y.ts"],
        );
    }
    commit(
        &s,
        "/r",
        "ac00001",
        200,
        &["src/modules/a/x.ts", "src/modules/c/z.ts"],
    );
    let r = compute_directory_cohorts("src/modules", Some(1), "/r", &s).unwrap();
    assert_eq!(r.pairs[0].directory_a, "a");
    assert_eq!(r.pairs[0].directory_b, "b");
    assert_eq!(r.pairs[0].cochange_commit_count, 3);
}

#[test]
fn repo_isolation() {
    let s = store();
    commit(
        &s,
        "/r/a",
        "c1aaaaa",
        100,
        &["src/modules/x/f.ts", "src/modules/y/g.ts"],
    );
    commit(
        &s,
        "/r/a",
        "c2bbbbb",
        200,
        &["src/modules/x/f.ts", "src/modules/y/g.ts"],
    );
    commit(
        &s,
        "/r/b",
        "c3ccccc",
        300,
        &["src/modules/x/f.ts", "src/modules/z/h.ts"],
    );

    let ra = compute_directory_cohorts("src/modules", Some(2), "/r/a", &s).unwrap();
    assert_eq!(ra.cohorts.len(), 1);
    assert_eq!(ra.cohorts[0].members, vec!["x".to_string(), "y".to_string()]);

    let rb = compute_directory_cohorts("src/modules", Some(1), "/r/b", &s).unwrap();
    assert!(rb.pairs.iter().any(|p| p.directory_b == "z" || p.directory_a == "z"));
    assert!(!rb.pairs.iter().any(|p| p.directory_a == "y" || p.directory_b == "y"));
}

#[test]
fn singleton_listed_not_discarded() {
    let s = store();
    commit(&s, "/r", "c1aaaaa", 100, &["src/modules/lonely/a.ts"]);
    let r = compute_directory_cohorts("src/modules", Some(2), "/r", &s).unwrap();
    assert_eq!(r.singletons, vec!["lonely".to_string()]);
    assert!(r.cohorts.is_empty());
}
