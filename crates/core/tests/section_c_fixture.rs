//! Section C: map / focus / impact — claim layers and ranking.

use atlas_core::{build_focus, build_impact, build_map, resolve_modules_subject};
use atlas_ir::{Commit, EpistemicLayer, StructuralEdge, StructuralEdgeKind, StructuralEvidence};
use atlas_storage::Store;
use chrono::{DateTime, Utc};

fn store() -> Store {
    Store::open(":memory:").unwrap()
}

fn commit(s: &Store, repo: &str, hash: &str, ts: i64, files: &[&str]) {
    let c = Commit {
        hash: hash.into(),
        short_hash: hash[..7.min(hash.len())].into(),
        message: format!("c {hash}"),
        author_name: "Dev".into(),
        author_email: "d@x.com".into(),
        timestamp: DateTime::<Utc>::from_timestamp(ts, 0).unwrap(),
        files_changed: files.iter().map(|f| f.to_string()).collect(),
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

fn seed(s: &Store, repo: &str) {
    commit(
        s,
        repo,
        "c1aaaaa",
        100,
        &["src/modules/core/services/order.service.ts"],
    );
    commit(
        s,
        repo,
        "c2bbbbb",
        200,
        &[
            "src/modules/core/services/order.service.ts",
            "src/modules/payment/services/pay.service.ts",
        ],
    );
    commit(
        s,
        repo,
        "c3ccccc",
        300,
        &["src/modules/payment/services/pay.service.ts"],
    );
    edge(
        s,
        repo,
        "src/modules/core/services/order.service.ts",
        "src/modules/payment/services/pay.service.ts",
    );
    edge(
        s,
        repo,
        "src/modules/payment/services/pay.service.ts",
        "src/modules/core/models/order.model.ts",
    );
    commit(
        s,
        repo,
        "c4ddddd",
        400,
        &["src/modules/core/models/order.model.ts"],
    );
    s.insert_configuration_artifact(repo, "package.json", "package_json", r#"{"dependencies":{}}"#, "ab")
        .unwrap();
}

#[test]
fn map_prefers_src_modules_when_present() {
    let s = store();
    seed(&s, "/r");
    assert_eq!(resolve_modules_subject("/r", &s).unwrap(), "src/modules");
    let m = build_map("/r", &s).unwrap();
    assert!(m.modules.contains(&"core".into()));
    assert!(m.modules.contains(&"payment".into()));
    assert!(!m.claims.is_empty());
    assert!(m.claims.iter().any(|c| c.layer == EpistemicLayer::Observed));
    assert!(!m.limitations.is_empty());
}

#[test]
fn map_falls_back_to_src_layers() {
    let s = store();
    commit(&s, "/r", "a1aaaaa", 1, &["src/services/x.ts"]);
    commit(&s, "/r", "a2bbbbb", 2, &["src/handlers/y.ts"]);
    let sub = resolve_modules_subject("/r", &s).unwrap();
    assert_eq!(sub, "src");
    let m = build_map("/r", &s).unwrap();
    assert_eq!(m.modules_subject, "src");
    assert!(m.modules.contains(&"services".into()) || m.modules.contains(&"handlers".into()));
}

#[test]
fn focus_file_lists_edges_and_authors() {
    let s = store();
    seed(&s, "/r");
    let f = build_focus(
        "src/modules/core/services/order.service.ts",
        "/r",
        &s,
    )
    .unwrap();
    assert_eq!(f.subject_kind, "file");
    assert!(!f.outgoing.is_empty() || !f.incoming.is_empty() || !f.authors.is_empty());
    assert!(!f.claims.is_empty());
}

#[test]
fn focus_module_name_resolves() {
    let s = store();
    seed(&s, "/r");
    let f = build_focus("core", "/r", &s).unwrap();
    assert!(f.subject.contains("core"));
    assert_eq!(f.subject_kind, "module");
}

#[test]
fn impact_ranks_structural_and_cochange_neighbors() {
    let s = store();
    seed(&s, "/r");
    let imp = build_impact(
        "src/modules/core/services/order.service.ts",
        "/r",
        &s,
    )
    .unwrap();
    assert!(!imp.neighbors.is_empty());
    // payment should appear via edge and co-change
    assert!(
        imp.neighbors
            .iter()
            .any(|n| n.path.contains("payment")),
        "expected payment neighbor: {:?}",
        imp.neighbors.iter().map(|n| &n.path).collect::<Vec<_>>()
    );
    // scores non-increasing
    for w in imp.neighbors.windows(2) {
        assert!(w[0].rank_score + 1e-6 >= w[1].rank_score);
    }
    assert!(!imp.dimensions_methodology.is_empty());
    assert!(imp.claims.iter().any(|c| matches!(
        c.layer,
        EpistemicLayer::Derived | EpistemicLayer::Observed | EpistemicLayer::Unknown
    )));
}

#[test]
fn impact_empty_unknown_claim() {
    let s = store();
    commit(&s, "/r", "z1zzzzz", 1, &["src/modules/lonely/a.ts"]);
    let imp = build_impact("src/modules/lonely/a.ts", "/r", &s).unwrap();
    assert!(imp.neighbors.is_empty());
    assert!(imp
        .claims
        .iter()
        .any(|c| c.layer == EpistemicLayer::Unknown));
}

#[test]
fn map_json_roundtrip() {
    let s = store();
    seed(&s, "/r");
    let m = build_map("/r", &s).unwrap();
    let j = serde_json::to_string(&m).unwrap();
    let back: atlas_ir::MapReport = serde_json::from_str(&j).unwrap();
    assert_eq!(back.schema_version, 1);
    assert_eq!(back.modules.len(), m.modules.len());
}
