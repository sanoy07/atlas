//! B9: anomalies from observed peer patterns / test linkage / deps.

use atlas_core::compute_anomalies;
use atlas_ir::{AnomalyKind, Commit};
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
fn peer_structure_deviation_surfaces() {
    let s = store();
    // 3 peers; a,c have services/; b lacks it — majority deviation.
    register(&s, "/r", "src/modules/a/services/a.ts", 1);
    register(&s, "/r", "src/modules/b/models/b.ts", 2);
    register(&s, "/r", "src/modules/c/services/c.ts", 3);

    let r = compute_anomalies("src/modules", "/r", &s).unwrap();
    assert!(r
        .anomalies
        .iter()
        .any(|a| a.kind == AnomalyKind::PeerStructureDeviation && a.subject.contains("/b")));
}

#[test]
fn missing_tests_anomaly_for_module_without_tests() {
    let s = store();
    register(&s, "/r", "src/modules/bare/x.ts", 1);
    register(&s, "/r", "src/modules/withtests/x.ts", 2);
    register(&s, "/r", "tests/withtests/t.ts", 3);

    let r = compute_anomalies("src/modules", "/r", &s).unwrap();
    assert!(r.anomalies.iter().any(|a| {
        a.kind == AnomalyKind::MissingAssociatedTests && a.subject.ends_with("/bare")
    }));
    assert!(!r.anomalies.iter().any(|a| {
        a.kind == AnomalyKind::MissingAssociatedTests && a.subject.ends_with("/withtests")
    }));
}

#[test]
fn declared_unobserved_dependency_anomaly() {
    let s = store();
    register(&s, "/r", "src/modules/m/x.ts", 1);
    s.insert_configuration_artifact(
        "/r",
        "package.json",
        "package_json",
        r#"{"dependencies":{"never-imported":"1.0.0"}}"#,
        "abc",
    )
    .unwrap();

    let r = compute_anomalies("src/modules", "/r", &s).unwrap();
    assert!(r.anomalies.iter().any(|a| {
        a.kind == AnomalyKind::DeclaredDependencyUnobserved && a.subject == "never-imported"
    }));
}

#[test]
fn normal_module_with_tests_has_no_missing_tests_anomaly() {
    let s = store();
    register(&s, "/r", "src/modules/ok/x.ts", 1);
    register(&s, "/r", "src/modules/ok/__tests__/x.test.ts", 2);
    let r = compute_anomalies("src/modules", "/r", &s).unwrap();
    assert!(!r
        .anomalies
        .iter()
        .any(|a| a.kind == AnomalyKind::MissingAssociatedTests));
}

#[test]
fn anomalies_sorted_deterministically() {
    let s = store();
    register(&s, "/r", "src/modules/a/x.ts", 1);
    register(&s, "/r", "src/modules/b/x.ts", 2);
    s.insert_configuration_artifact(
        "/r",
        "package.json",
        "package_json",
        r#"{"dependencies":{"zzz":"1","aaa":"1"}}"#,
        "abc",
    )
    .unwrap();
    let r = compute_anomalies("src/modules", "/r", &s).unwrap();
    // Ensure sort is stable: consecutive pairs are non-decreasing by kind then subject.
    for w in r.anomalies.windows(2) {
        let ka = format!("{:?}|{}", w[0].kind, w[0].subject);
        let kb = format!("{:?}|{}", w[1].kind, w[1].subject);
        assert!(ka <= kb, "{} should be <= {}", ka, kb);
    }
}

#[test]
fn language_does_not_claim_bad_architecture() {
    let s = store();
    register(&s, "/r", "src/modules/a/x.ts", 1);
    let r = compute_anomalies("src/modules", "/r", &s).unwrap();
    let blob = serde_json::to_string(&r).unwrap().to_lowercase();
    assert!(!blob.contains("bad architecture"));
    assert!(!blob.contains("poor design"));
    assert!(!blob.contains("\"bug\""));
}
