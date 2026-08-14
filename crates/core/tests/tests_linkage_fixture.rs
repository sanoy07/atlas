//! B6: test ↔ module linkage under explicit path rules.

use atlas_core::compute_test_module_links;
use atlas_ir::{Commit, EvidenceClass, TestLinkageKind};
use atlas_storage::Store;
use chrono::{DateTime, Utc};

fn store() -> Store {
    Store::open(":memory:").unwrap()
}

fn register(s: &Store, repo: &str, path: &str, seed: u32) {
    let hash = format!("{:016x}{:08x}", seed as u64, path.len() as u64);
    let c = Commit {
        hash: hash.clone(),
        short_hash: hash[..7].to_string(),
        message: "seed".into(),
        author_name: "T".into(),
        author_email: "t@x".into(),
        timestamp: DateTime::<Utc>::from_timestamp(1, 0).unwrap(),
        files_changed: vec![path.to_string()],
        parents: vec![],
    };
    s.insert_commit(&c, repo).unwrap();
}

#[test]
fn direct_test_under_module() {
    let s = store();
    register(&s, "/r", "src/modules/core/services/x.ts", 1);
    register(
        &s,
        "/r",
        "src/modules/core/services/__tests__/x.test.ts",
        2,
    );

    let r = compute_test_module_links("src/modules", None, "/r", &s).unwrap();
    assert_eq!(r.total_links, 1);
    assert_eq!(r.links[0].module_name, "core");
    assert_eq!(r.links[0].linkage_kind, TestLinkageKind::DirectPathPrefix);
    assert_eq!(r.links[0].evidence_class, EvidenceClass::Deterministic);
}

#[test]
fn conventional_tests_dir_is_derived() {
    let s = store();
    register(&s, "/r", "src/modules/identity/auth.ts", 1);
    register(&s, "/r", "tests/identity/test-auth-user.ts", 2);

    let r = compute_test_module_links("src/modules", None, "/r", &s).unwrap();
    assert_eq!(r.total_links, 1);
    assert_eq!(r.links[0].module_name, "identity");
    assert_eq!(r.links[0].linkage_kind, TestLinkageKind::ConventionalTestsDir);
    assert_eq!(r.links[0].evidence_class, EvidenceClass::Derived);
}

#[test]
fn unrelated_test_is_unlinked() {
    let s = store();
    register(&s, "/r", "src/modules/core/a.ts", 1);
    register(&s, "/r", "tests/rbac/test-permission-queries.ts", 2);
    // Path heuristic: top-level `test/` directory (not under a known module).
    register(&s, "/r", "test/helpers/request-context.test.ts", 3);

    let r = compute_test_module_links("src/modules", None, "/r", &s).unwrap();
    assert!(r.links.is_empty());
    assert!(r.unlinked_tests.iter().any(|t| t.contains("rbac")));
    assert!(r.unlinked_tests.iter().any(|t| t.contains("request-context")));
}

#[test]
fn multiple_modules_sorted_deterministically() {
    let s = store();
    register(&s, "/r", "src/modules/b/x.ts", 1);
    register(&s, "/r", "src/modules/a/x.ts", 2);
    register(&s, "/r", "tests/b/t.ts", 3);
    register(&s, "/r", "tests/a/t.ts", 4);

    let r = compute_test_module_links("src/modules", None, "/r", &s).unwrap();
    assert_eq!(r.total_links, 2);
    assert_eq!(r.links[0].module_name, "a");
    assert_eq!(r.links[1].module_name, "b");
}

#[test]
fn path_filter_restricts_test_set() {
    let s = store();
    register(&s, "/r", "src/modules/core/x.ts", 1);
    register(&s, "/r", "tests/core/a.ts", 2);
    register(&s, "/r", "tests/core/nested/b.ts", 3);
    register(&s, "/r", "tests/other/c.ts", 4);

    let r = compute_test_module_links("src/modules", Some("tests/core"), "/r", &s).unwrap();
    assert_eq!(r.total_links, 2);
    assert!(r.links.iter().all(|l| l.test_path.starts_with("tests/core")));
}

#[test]
fn tests_dir_requires_existing_module() {
    let s = store();
    // tests/ghost exists but no src/modules/ghost
    register(&s, "/r", "src/modules/real/x.ts", 1);
    register(&s, "/r", "tests/ghost/t.ts", 2);

    let r = compute_test_module_links("src/modules", None, "/r", &s).unwrap();
    assert!(r.links.is_empty());
    assert_eq!(r.unlinked_tests, vec!["tests/ghost/t.ts".to_string()]);
}
