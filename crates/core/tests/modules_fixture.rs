//! B5: `atlas modules` — module inventory from path structure + evidence counts.

use atlas_core::compute_modules;
use atlas_ir::{Commit, StructuralEdge, StructuralEdgeKind, StructuralEvidence};
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
        timestamp: DateTime::<Utc>::from_timestamp(100 + seed as i64, 0).unwrap(),
        files_changed: vec![path.to_string()],
        parents: vec![],
    };
    s.insert_commit(&c, repo).unwrap();
}

fn edge(s: &Store, repo: &str, src: &str, tgt: &str) {
    s.insert_structural_edge(
        &StructuralEdge {
            source_file: src.into(),
            source_symbol: None,
            target_file: tgt.into(),
            target_symbol: None,
            kind: StructuralEdgeKind::Imports,
            evidence: StructuralEvidence {
                source_file: src.into(),
                line: Some(1),
                snippet: "import".into(),
                extractor: "test".into(),
            },
        },
        repo,
    )
    .unwrap();
}

#[test]
fn empty_repo_has_no_modules() {
    let s = store();
    let r = compute_modules("src/modules", "/repo", &s).unwrap();
    assert_eq!(r.total_modules, 0);
    assert!(r.modules.is_empty());
}

#[test]
fn discovers_immediate_child_modules_alphabetically() {
    let s = store();
    register(&s, "/repo", "src/modules/zebra/a.ts", 1);
    register(&s, "/repo", "src/modules/alpha/b.ts", 2);
    register(&s, "/repo", "src/modules/alpha/c.ts", 3);
    // Nested deeper — not a module of src/modules.
    register(&s, "/repo", "src/modules/alpha/services/x.ts", 4);

    let r = compute_modules("src/modules", "/repo", &s).unwrap();
    assert_eq!(r.total_modules, 2);
    assert_eq!(r.modules[0].name, "alpha");
    assert_eq!(r.modules[1].name, "zebra");
    assert_eq!(r.modules[0].file_count, 3);
    assert_eq!(r.modules[0].subdirectories, vec!["services".to_string()]);
}

#[test]
fn commit_and_edge_counts_are_deterministic() {
    let s = store();
    register(&s, "/repo", "src/modules/m/a.ts", 1);
    register(&s, "/repo", "src/modules/m/b.ts", 2);
    register(&s, "/repo", "src/modules/n/c.ts", 3);
    edge(&s, "/repo", "src/modules/m/a.ts", "src/modules/n/c.ts");
    edge(&s, "/repo", "src/modules/n/c.ts", "src/modules/m/b.ts");

    let r = compute_modules("src/modules", "/repo", &s).unwrap();
    let m = r.modules.iter().find(|x| x.name == "m").unwrap();
    assert_eq!(m.observed_commit_count, 2);
    assert_eq!(m.outgoing_edge_count, 1);
    assert_eq!(m.incoming_edge_count, 1);
}

#[test]
fn test_association_derived_from_in_module_and_tests_dir() {
    let s = store();
    register(&s, "/repo", "src/modules/core/svc.ts", 1);
    register(
        &s,
        "/repo",
        "src/modules/core/services/__tests__/svc.test.ts",
        2,
    );
    register(&s, "/repo", "src/modules/id/svc.ts", 3);
    register(&s, "/repo", "tests/id/test-auth.ts", 4);
    register(&s, "/repo", "src/modules/bare/svc.ts", 5);

    let r = compute_modules("src/modules", "/repo", &s).unwrap();
    let core = r.modules.iter().find(|x| x.name == "core").unwrap();
    assert!(core.has_associated_tests);
    assert!(core.in_module_test_file_count >= 1);

    let id = r.modules.iter().find(|x| x.name == "id").unwrap();
    assert!(id.has_associated_tests);
    assert_eq!(id.in_module_test_file_count, 0);

    let bare = r.modules.iter().find(|x| x.name == "bare").unwrap();
    assert!(!bare.has_associated_tests);
}

#[test]
fn repo_isolation_on_module_discovery() {
    let s = store();
    register(&s, "/repo/a", "src/modules/only_a/x.ts", 1);
    register(&s, "/repo/b", "src/modules/only_b/y.ts", 2);

    let ra = compute_modules("src/modules", "/repo/a", &s).unwrap();
    assert_eq!(ra.total_modules, 1);
    assert_eq!(ra.modules[0].name, "only_a");

    let rb = compute_modules("src/modules", "/repo/b", &s).unwrap();
    assert_eq!(rb.total_modules, 1);
    assert_eq!(rb.modules[0].name, "only_b");
}

#[test]
fn default_subject_is_src_modules() {
    let s = store();
    register(&s, "/repo", "src/modules/x/f.ts", 1);
    let r = compute_modules("", "/repo", &s).unwrap();
    assert_eq!(r.subject, "src/modules");
    assert_eq!(r.total_modules, 1);
}
