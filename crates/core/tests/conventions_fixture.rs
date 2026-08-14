//! B1: `atlas conventions` — verifies the aggregation itself.
//!
//! Tests file-existence counting across peer directories.  Does NOT test
//! semantic interpretation.  A pattern is a count; whether that count
//! constitutes a "convention" is a separate layer that does not exist.

use atlas_core::detect_peer_structure;
use atlas_ir::{Commit, PeerStructureReport};
use atlas_storage::Store;
use chrono::{DateTime, Utc};

fn temp_store() -> Store {
    Store::open(":memory:").unwrap()
}

/// Register a file by inserting a synthetic commit that touched it.
/// This is how RWATP files enter `files` table too — via `commit_files`
/// during `ingest_git`.  Using the same mechanism keeps the test fixture
/// close to the real ingestion path.
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

fn find_pattern<'a>(r: &'a PeerStructureReport, element: &str) -> Option<&'a atlas_ir::PeerStructurePattern> {
    r.patterns.iter().find(|p| p.element == element)
        .or_else(|| r.singletons.iter().find(|p| p.element == element))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn empty_subject_produces_empty_report_not_error() {
    let store = temp_store();
    let r = detect_peer_structure("src/modules", "repo", &store).unwrap();
    assert_eq!(r.peers.len(), 0);
    assert!(r.patterns.is_empty());
    assert!(r.deviations.is_empty());
    assert_eq!(r.subject, "src/modules");
}

#[test]
fn immediate_subdirectory_pattern_counts_correctly() {
    let store = temp_store();
    let repo = "repo";
    // 3 peers under src/modules, each with a services/ subdirectory.
    register(&store, repo, "src/modules/a/services/a.service.ts", 1);
    register(&store, repo, "src/modules/b/services/b.service.ts", 2);
    register(&store, repo, "src/modules/c/services/c.service.ts", 3);

    let r = detect_peer_structure("src/modules", repo, &store).unwrap();
    assert_eq!(r.peers, vec!["a".to_string(), "b".to_string(), "c".to_string()]);

    let services = find_pattern(&r, "services/").expect("services/ element");
    assert_eq!(services.prevalence_num, 3);
    assert_eq!(services.prevalence_den, 3);
    assert_eq!(services.present_in, vec!["a", "b", "c"]);

    // Also detects the suffix pattern inside services/.
    let svc_files = find_pattern(&r, "services/*.service.ts").expect("suffix pattern");
    assert_eq!(svc_files.prevalence_num, 3);
}

#[test]
fn deviation_reported_when_peer_lacks_majority_element() {
    let store = temp_store();
    let repo = "repo";
    // 3 peers.  b lacks graphql/permissions.ts that a and c have.
    for p in ["a", "b", "c"] {
        register(&store, repo, &format!("src/modules/{}/services/{}.service.ts", p, p), p.len() as u32);
        register(&store, repo, &format!("src/modules/{}/models/{}.model.ts", p, p), p.len() as u32);
        register(&store, repo, &format!("src/modules/{}/graphql/{}.typeDefs.ts", p, p), p.len() as u32);
    }
    register(&store, repo, "src/modules/a/graphql/permissions.ts", 100);
    register(&store, repo, "src/modules/c/graphql/permissions.ts", 200);

    let r = detect_peer_structure("src/modules", repo, &store).unwrap();

    let perms = find_pattern(&r, "graphql/permissions.ts").expect("permissions element");
    assert_eq!(perms.prevalence_num, 2, "2 of 3 peers have it");
    assert_eq!(perms.prevalence_den, 3);

    // 2/3 > 3/2 = 1.5 → strict majority → deviation for `b`.
    let b_dev = r.deviations.iter().find(|d| d.peer == "b" && d.element == "graphql/permissions.ts")
        .expect("deviation for b");
    assert_eq!(b_dev.peer_prevalence_num, 2);
    assert_eq!(b_dev.peer_prevalence_den, 3);
}

#[test]
fn all_peers_counted_no_stub_exclusion_from_denominator() {
    let store = temp_store();
    let repo = "repo";
    // 4 peers: 3 substantial, 1 with a single file.  Denominator stays 4.
    for p in ["a", "b", "c"] {
        register(&store, repo, &format!("src/modules/{}/services/x.service.ts", p), p.len() as u32);
        register(&store, repo, &format!("src/modules/{}/models/y.model.ts",   p), p.len() as u32);
    }
    // Stub peer.
    register(&store, repo, "src/modules/stub/index.ts", 999);

    let r = detect_peer_structure("src/modules", repo, &store).unwrap();
    assert_eq!(r.peers.len(), 4, "stub must count as a peer");
    assert_eq!(r.peers, vec!["a".to_string(), "b".to_string(), "c".to_string(), "stub".to_string()]);

    let services = find_pattern(&r, "services/").expect("services element");
    assert_eq!(services.prevalence_den, 4, "denominator must be full peer count including stub");
    assert_eq!(services.prevalence_num, 3, "stub lacks services/");

    // Stub should appear as low-complexity note, NOT excluded from patterns.
    let note = r.low_complexity_note.as_ref().expect("low-complexity note expected");
    assert!(note.low_complexity_peers.iter().any(|(p, n)| p == "stub" && *n == 1),
        "stub with 1 file should appear in low-complexity note");
    // And a/b/c should NOT appear because they have 2 files each (< default threshold 5 — so they will).
    // The default threshold is 5; a/b/c have 2 files each → they'll also appear.
    // This test doesn't care about that — only that stub is present.
}

#[test]
fn subject_with_child_directories_is_treated_as_parent() {
    // Simpler semantics: any subject with immediate child directories is
    // the peer parent.  `atlas conventions src/modules/blockchain` therefore
    // reports on blockchain's own substructure (services, models, graphql…)
    // rather than comparing blockchain against its sibling modules.  For
    // the sibling comparison, callers pass the module *container* path
    // (e.g. `atlas conventions src/modules`).
    let store = temp_store();
    let repo = "repo";
    for p in ["a", "b", "c"] {
        register(&store, repo, &format!("src/modules/{}/services/x.service.ts", p), p.len() as u32);
    }

    let r = detect_peer_structure("src/modules/a", repo, &store).unwrap();
    assert_eq!(r.peer_parent, "src/modules/a",
        "subject with children is the peer_parent");
    assert_eq!(r.peers, vec!["services".to_string()]);
}

#[test]
fn leaf_subject_falls_back_to_sibling_comparison() {
    // A subject with NO child directories (a leaf) falls back to comparing
    // against its parent's children.  This is the only case where auto-
    // detection changes the semantic — leaf subjects have nowhere else
    // to be a parent of.
    let store = temp_store();
    let repo = "repo";
    for p in ["a", "b", "c"] {
        register(&store, repo, &format!("src/modules/{}/services/x.service.ts", p), p.len() as u32);
    }
    // Add a leaf sibling `d` that has just a top-level file (no subdirs).
    register(&store, repo, "src/modules/d/index.ts", 999);

    let r = detect_peer_structure("src/modules/d", repo, &store).unwrap();
    assert_eq!(r.peer_parent, "src/modules",
        "leaf subject falls back to its parent as peer_parent");
    // `d` IS a child of src/modules (it contains index.ts), so it appears
    // in the peer set alongside a, b, c.  The test guards the fallback
    // semantic (leaf uses parent) — not that d is somehow excluded.
    assert_eq!(r.peers, vec!["a".to_string(), "b".to_string(), "c".to_string(), "d".to_string()]);
    // And d appears as a deviation for services/ (3 of 4 peers have it).
    assert!(r.deviations.iter().any(|dev| dev.peer == "d" && dev.element == "services/"),
        "d should be flagged as deviating from the 3/4 services/ pattern");
}

#[test]
fn singletons_and_repeated_patterns_split_at_prevalence_two() {
    let store = temp_store();
    let repo = "repo";
    // 3 peers.  services/ in a and b (prevalence 2 → pattern).
    // rare/     in c only (prevalence 1 → singleton).
    register(&store, repo, "src/modules/a/services/x.service.ts", 1);
    register(&store, repo, "src/modules/b/services/y.service.ts", 2);
    register(&store, repo, "src/modules/c/rare/z.ts",             3);

    let r = detect_peer_structure("src/modules", repo, &store).unwrap();
    let svc = r.patterns.iter().find(|p| p.element == "services/").expect("pattern");
    assert_eq!(svc.prevalence_num, 2);

    let rare = r.singletons.iter().find(|p| p.element == "rare/").expect("singleton");
    assert_eq!(rare.prevalence_num, 1);
    assert_eq!(rare.present_in, vec!["c"]);

    assert!(!r.patterns.iter().any(|p| p.element == "rare/"),
        "singleton must NOT appear in patterns list");
}

#[test]
fn strict_majority_is_the_deviation_threshold() {
    // With 4 peers, an element present in exactly 2 is NOT a majority (2*2=4, not >4),
    // so peers lacking it should NOT be flagged as deviations.  An element in 3 IS
    // a majority (3*2=6 > 4).
    let store = temp_store();
    let repo = "repo";
    for p in ["a", "b", "c", "d"] {
        register(&store, repo, &format!("src/modules/{}/services/x.service.ts", p), p.len() as u32);
    }
    // Only a and b have graphql/.
    register(&store, repo, "src/modules/a/graphql/index.ts", 100);
    register(&store, repo, "src/modules/b/graphql/index.ts", 101);
    // a, b, c have models/.
    for p in ["a", "b", "c"] {
        register(&store, repo, &format!("src/modules/{}/models/x.model.ts", p), (p.len() * 10) as u32);
    }

    let r = detect_peer_structure("src/modules", repo, &store).unwrap();
    // graphql/ 2/4 → no majority → no deviation.
    assert!(!r.deviations.iter().any(|d| d.element == "graphql/"),
        "2 of 4 is not a strict majority; must not produce a deviation");
    // models/ 3/4 → strict majority → d must have a deviation.
    let dev = r.deviations.iter().find(|d| d.peer == "d" && d.element == "models/")
        .expect("d must be deviant for models/");
    assert_eq!(dev.peer_prevalence_num, 3);
    assert_eq!(dev.peer_prevalence_den, 4);
}

#[test]
fn exact_filename_pattern_does_not_greedily_match_suffixes() {
    // Regression: `graphql/permissions.ts` is an EXACT-name pattern.  A file
    // named `graphql/blockchain.permissions.ts` must NOT be counted as
    // matching it — that would silently merge a real naming deviation.
    let store = temp_store();
    let repo = "repo";
    // 2 peers with exact `permissions.ts`
    register(&store, repo, "src/modules/a/graphql/permissions.ts", 1);
    register(&store, repo, "src/modules/b/graphql/permissions.ts", 2);
    // 1 peer with a namespaced variant
    register(&store, repo, "src/modules/c/graphql/c.permissions.ts", 3);

    let r = detect_peer_structure("src/modules", repo, &store).unwrap();
    let exact = find_pattern(&r, "graphql/permissions.ts").expect("exact pattern");
    assert_eq!(exact.prevalence_num, 2,
        "exact-name element must NOT count namespaced variants (was greedy .ends_with)");
    assert_eq!(exact.present_in, vec!["a", "b"]);

    // c gets flagged as a deviation for the exact permissions.ts pattern.
    // Wait — only 2 of 3 have it, that's 2/3 < strict majority when
    // present*2 > peer_count → 2*2=4 > 3 → majority.  So yes.
    assert!(r.deviations.iter().any(|d| d.peer == "c" && d.element == "graphql/permissions.ts"),
        "c must be flagged as lacking exact `graphql/permissions.ts`");
}

#[test]
fn low_complexity_note_reports_threshold_explicitly() {
    let store = temp_store();
    let repo = "repo";
    // Three peers: a and b with 6 files each (above default threshold 5), c with 1.
    for i in 0..6 {
        register(&store, repo, &format!("src/modules/a/services/x{}.ts", i), 1000 + i);
        register(&store, repo, &format!("src/modules/b/services/y{}.ts", i), 2000 + i);
    }
    register(&store, repo, "src/modules/c/only.ts", 3000);

    let r = detect_peer_structure("src/modules", repo, &store).unwrap();
    let note = r.low_complexity_note.expect("note expected");
    assert_eq!(note.file_count_threshold, 5, "default threshold surfaced explicitly");
    assert_eq!(note.low_complexity_peers.len(), 1, "only c is below 5 files");
    assert_eq!(note.low_complexity_peers[0].0, "c");
    assert_eq!(note.low_complexity_peers[0].1, 1);
}
