//! B2: `atlas coupling` — verifies module→module edge aggregation.
//!
//! Tests observation counts and their partitioning into internal
//! (excluded), cross-module (matrix), external (separate), and platform
//! (separate).  Does NOT test any semantic interpretation of the numbers.

use atlas_core::compute_module_coupling;
use atlas_ir::{ModuleCouplingReport, StructuralEdge, StructuralEdgeKind, StructuralEvidence};
use atlas_storage::Store;
use atlas_ir::Commit;
use chrono::{DateTime, Utc};

fn temp_store() -> Store {
    Store::open(":memory:").unwrap()
}

/// Register a file by inserting a synthetic commit that touched it,
/// so it appears in `store.all_file_paths` (the same mechanism the
/// ingest pipeline uses).
fn register(store: &Store, repo: &str, path: &str, seed: u32) {
    let hash = format!("{:016x}{:016x}", seed as u64, path.len() as u64);
    let c = Commit {
        hash:          hash.clone(),
        short_hash:    hash[..7].to_string(),
        message:       "seed".into(),
        author_name:   "T".into(),
        author_email:  "t@x".into(),
        timestamp:     DateTime::<Utc>::from_timestamp(1, 0).unwrap(),
        files_changed: vec![path.to_string()],
        parents:       vec![],
    };
    store.insert_commit(&c, repo).unwrap();
}

fn ins_edge(
    store: &Store, repo: &str, source: &str, target: &str, kind: StructuralEdgeKind,
) {
    let edge = StructuralEdge {
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
    store.insert_structural_edge(&edge, repo).unwrap();
}

fn find_cell<'a>(r: &'a ModuleCouplingReport, src: &str, tgt: &str)
    -> Option<&'a atlas_ir::ModuleCouplingCell>
{
    r.cells.iter().find(|c| c.source_module == src && c.target_module == tgt)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn empty_subject_produces_empty_report() {
    let store = temp_store();
    let r = compute_module_coupling("src/modules", "repo", &store).unwrap();
    assert!(r.modules.is_empty());
    assert!(r.cells.is_empty());
    assert!(r.external_dependencies.is_empty());
    assert!(r.platform_usage.is_empty());
    assert_eq!(r.subject, "src/modules");
}

#[test]
fn single_cross_module_edge_produces_one_cell() {
    let store = temp_store();
    let repo = "repo";
    register(&store, repo, "src/modules/a/service.ts", 1);
    register(&store, repo, "src/modules/b/model.ts",   2);
    ins_edge(&store, repo, "src/modules/a/service.ts", "src/modules/b/model.ts",
             StructuralEdgeKind::Imports);

    let r = compute_module_coupling("src/modules", repo, &store).unwrap();
    assert_eq!(r.modules, vec!["a", "b"]);
    assert_eq!(r.cells.len(), 1, "sparse: exactly one non-zero cell");

    let cell = &r.cells[0];
    assert_eq!(cell.source_module, "a");
    assert_eq!(cell.target_module, "b");
    assert_eq!(cell.edge_count, 1);
    assert_eq!(cell.distinct_source_files, 1);
    assert_eq!(cell.distinct_target_files, 1);
    assert_eq!(cell.kinds.len(), 1);
    assert_eq!(cell.kinds[0].kind, "imports");
    assert_eq!(cell.kinds[0].edge_count, 1);
}

#[test]
fn internal_edges_are_excluded_from_cells() {
    // An edge within a single module is COHESION, not coupling.
    // It must NOT produce a coupling cell.
    let store = temp_store();
    let repo = "repo";
    register(&store, repo, "src/modules/a/service.ts", 1);
    register(&store, repo, "src/modules/a/helper.ts",  2);
    ins_edge(&store, repo, "src/modules/a/service.ts", "src/modules/a/helper.ts",
             StructuralEdgeKind::Imports);

    let r = compute_module_coupling("src/modules", repo, &store).unwrap();
    assert!(r.cells.is_empty(), "internal edges must NOT appear in coupling cells");
    assert_eq!(r.modules, vec!["a"]);
}

#[test]
fn edges_split_by_kind_in_breakdown() {
    let store = temp_store();
    let repo = "repo";
    register(&store, repo, "src/modules/a/s.ts", 1);
    register(&store, repo, "src/modules/b/m.ts", 2);
    // Three edges of two kinds between the same pair.
    ins_edge(&store, repo, "src/modules/a/s.ts", "src/modules/b/m.ts", StructuralEdgeKind::Imports);
    ins_edge(&store, repo, "src/modules/a/s.ts", "src/modules/b/m.ts", StructuralEdgeKind::CallsStatic);
    // For a second edge of imports we need a distinct symbol tuple —
    // insert_structural_edge is INSERT OR IGNORE on the unique constraint,
    // so use a different source file so the row is genuinely new.
    register(&store, repo, "src/modules/a/s2.ts", 3);
    ins_edge(&store, repo, "src/modules/a/s2.ts", "src/modules/b/m.ts", StructuralEdgeKind::Imports);

    let r = compute_module_coupling("src/modules", repo, &store).unwrap();
    let cell = find_cell(&r, "a", "b").expect("a→b cell");
    assert_eq!(cell.edge_count, 3);
    assert_eq!(cell.distinct_source_files, 2, "s.ts and s2.ts");
    assert_eq!(cell.distinct_target_files, 1, "only m.ts");

    // Kinds sorted by edge count desc: imports (2), calls_static (1).
    assert_eq!(cell.kinds.len(), 2);
    assert_eq!(cell.kinds[0].kind, "imports");
    assert_eq!(cell.kinds[0].edge_count, 2);
    assert_eq!(cell.kinds[1].kind, "calls_static");
    assert_eq!(cell.kinds[1].edge_count, 1);
}

#[test]
fn external_targets_go_to_external_section_not_cells() {
    let store = temp_store();
    let repo = "repo";
    register(&store, repo, "src/modules/a/s.ts", 1);
    ins_edge(&store, repo, "src/modules/a/s.ts", "UNRESOLVED:external:mongoose",
             StructuralEdgeKind::Imports);
    ins_edge(&store, repo, "src/modules/a/s.ts", "UNRESOLVED:external:mongoose",
             StructuralEdgeKind::Imports);

    let r = compute_module_coupling("src/modules", repo, &store).unwrap();
    assert!(r.cells.is_empty(), "external edges must NOT appear in coupling cells");
    assert_eq!(r.external_dependencies.len(), 1);
    let ext = &r.external_dependencies[0];
    assert_eq!(ext.source_module, "a");
    assert_eq!(ext.external_target, "UNRESOLVED:external:mongoose");
    // Both edges are (src, tgt, kind, extractor)-identical → INSERT OR IGNORE
    // collapses to one row.  Real ingest inserts different symbols; here we
    // simulate the deduplicated result.
    assert!(ext.edge_count >= 1);
    assert_eq!(ext.distinct_source_files, 1);
}

#[test]
fn platform_edges_go_to_platform_section_not_cells() {
    // Edge from a module to a non-module repo file (src/common) is
    // "platform usage", not module-to-module coupling.
    let store = temp_store();
    let repo = "repo";
    register(&store, repo, "src/modules/a/s.ts",         1);
    register(&store, repo, "src/common/util/helper.ts",  2);
    ins_edge(&store, repo, "src/modules/a/s.ts", "src/common/util/helper.ts",
             StructuralEdgeKind::Imports);

    let r = compute_module_coupling("src/modules", repo, &store).unwrap();
    assert!(r.cells.is_empty(), "platform-layer edges must NOT appear in coupling cells");
    assert_eq!(r.platform_usage.len(), 1);
    let plat = &r.platform_usage[0];
    assert_eq!(plat.source_module, "a");
    assert_eq!(plat.platform_target, "src/common",
        "platform target aggregated to first two path segments");
    assert_eq!(plat.edge_count, 1);
}

#[test]
fn distinct_source_and_target_counts_are_accurate() {
    let store = temp_store();
    let repo = "repo";
    for i in 0..3 { register(&store, repo, &format!("src/modules/a/s{}.ts", i), i); }
    for i in 0..2 { register(&store, repo, &format!("src/modules/b/m{}.ts", i), 100 + i); }
    // 3 source files each reach 2 targets = 6 edges.
    for i in 0..3 {
        for j in 0..2 {
            ins_edge(&store, repo,
                &format!("src/modules/a/s{}.ts", i),
                &format!("src/modules/b/m{}.ts", j),
                StructuralEdgeKind::Imports);
        }
    }

    let r = compute_module_coupling("src/modules", repo, &store).unwrap();
    let cell = find_cell(&r, "a", "b").expect("a→b cell");
    assert_eq!(cell.edge_count, 6);
    assert_eq!(cell.distinct_source_files, 3);
    assert_eq!(cell.distinct_target_files, 2);
}

#[test]
fn fan_out_and_fan_in_derived_correctly() {
    let store = temp_store();
    let repo = "repo";
    for m in ["a", "b", "c"] {
        register(&store, repo, &format!("src/modules/{}/f.ts", m), m.len() as u32);
    }
    // a→b, a→c: a has fan-out 2.
    // b→c: b has fan-out 1, c has fan-in 2 (from a AND b).
    ins_edge(&store, repo, "src/modules/a/f.ts", "src/modules/b/f.ts", StructuralEdgeKind::Imports);
    ins_edge(&store, repo, "src/modules/a/f.ts", "src/modules/c/f.ts", StructuralEdgeKind::Imports);
    ins_edge(&store, repo, "src/modules/b/f.ts", "src/modules/c/f.ts", StructuralEdgeKind::Imports);

    let r = compute_module_coupling("src/modules", repo, &store).unwrap();
    let a_out = r.fan_out.iter().find(|f| f.module == "a").expect("a fan-out");
    assert_eq!(a_out.edges, 2);
    assert_eq!(a_out.fan, 2, "a reaches b AND c");
    let b_out = r.fan_out.iter().find(|f| f.module == "b").expect("b fan-out");
    assert_eq!(b_out.fan, 1);
    let c_in = r.fan_in.iter().find(|f| f.module == "c").expect("c fan-in");
    assert_eq!(c_in.edges, 2);
    assert_eq!(c_in.fan, 2, "c is reached by a AND b");
}

#[test]
fn bidirectional_coupling_appears_as_two_cells() {
    // Atlas does NOT declare a "cycle" — it reports A→B and B→A as two
    // separate cells and lets the caller notice both directions.
    let store = temp_store();
    let repo = "repo";
    register(&store, repo, "src/modules/a/f.ts", 1);
    register(&store, repo, "src/modules/b/f.ts", 2);
    ins_edge(&store, repo, "src/modules/a/f.ts", "src/modules/b/f.ts", StructuralEdgeKind::Imports);
    ins_edge(&store, repo, "src/modules/b/f.ts", "src/modules/a/f.ts", StructuralEdgeKind::CallsStatic);

    let r = compute_module_coupling("src/modules", repo, &store).unwrap();
    assert_eq!(r.cells.len(), 2, "bidirectional coupling produces two cells (no cycle collapse)");
    assert!(find_cell(&r, "a", "b").is_some());
    assert!(find_cell(&r, "b", "a").is_some());
}

#[test]
fn cells_sorted_by_edge_count_descending() {
    let store = temp_store();
    let repo = "repo";
    register(&store, repo, "src/modules/a/s.ts", 1);
    register(&store, repo, "src/modules/b/t.ts", 2);
    register(&store, repo, "src/modules/c/u.ts", 3);
    // a→b: 1 edge.  a→c: 2 edges (different source files).
    register(&store, repo, "src/modules/a/s2.ts", 4);
    ins_edge(&store, repo, "src/modules/a/s.ts",  "src/modules/b/t.ts", StructuralEdgeKind::Imports);
    ins_edge(&store, repo, "src/modules/a/s.ts",  "src/modules/c/u.ts", StructuralEdgeKind::Imports);
    ins_edge(&store, repo, "src/modules/a/s2.ts", "src/modules/c/u.ts", StructuralEdgeKind::Imports);

    let r = compute_module_coupling("src/modules", repo, &store).unwrap();
    assert_eq!(r.cells.len(), 2);
    assert_eq!(r.cells[0].edge_count, 2, "higher-count cell comes first");
    assert_eq!(r.cells[0].target_module, "c");
    assert_eq!(r.cells[1].edge_count, 1);
}
