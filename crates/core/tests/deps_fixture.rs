//! B7: package.json declared deps ↔ structural external observations.

use atlas_core::{compute_dependency_linkage, external_package_name};
use atlas_ir::{Commit, DependencySection, StructuralEdge, StructuralEdgeKind, StructuralEvidence};
use atlas_storage::Store;
use chrono::{DateTime, Utc};

fn store() -> Store {
    Store::open(":memory:").unwrap()
}

fn seed_file(s: &Store, repo: &str, path: &str, seed: u32) {
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
                snippet: "import x from 'pkg'".into(),
                extractor: "test".into(),
            },
        },
        repo,
    )
    .unwrap();
}

fn put_pkg(s: &Store, repo: &str, json: &str) {
    s.insert_configuration_artifact(repo, "package.json", "package_json", json, "deadbeef")
        .unwrap();
}

#[test]
fn external_package_name_extraction() {
    assert_eq!(
        external_package_name("UNRESOLVED:external:mongoose").as_deref(),
        Some("mongoose")
    );
    assert_eq!(
        external_package_name("UNRESOLVED:external:dotenv/config").as_deref(),
        Some("dotenv")
    );
    assert_eq!(
        external_package_name("UNRESOLVED:external:@graphql-codegen/cli").as_deref(),
        Some("@graphql-codegen/cli")
    );
    assert_eq!(
        external_package_name("UNRESOLVED:external:@scope/name/sub").as_deref(),
        Some("@scope/name")
    );
    assert!(external_package_name("src/foo.ts").is_none());
}

#[test]
fn declared_but_unused_package() {
    let s = store();
    put_pkg(
        &s,
        "/r",
        r#"{"dependencies":{"lodash":"^4.0.0","mongoose":"^8.0.0"}}"#,
    );
    seed_file(&s, "/r", "src/a.ts", 1);
    edge(&s, "/r", "src/a.ts", "UNRESOLVED:external:mongoose");

    let r = compute_dependency_linkage("/r", &s).unwrap();
    let lodash = r.packages.iter().find(|p| p.package_name == "lodash").unwrap();
    assert!(lodash.is_declared);
    assert!(!lodash.is_observed);
    assert_eq!(lodash.section, DependencySection::Dependencies);

    let mongoose = r.packages.iter().find(|p| p.package_name == "mongoose").unwrap();
    assert!(mongoose.is_declared && mongoose.is_observed);
    assert_eq!(mongoose.observation_count, 1);
    assert_eq!(r.declared_unobserved, 1);
    assert_eq!(r.declared_and_observed, 1);
}

#[test]
fn package_imported_by_multiple_files() {
    let s = store();
    put_pkg(&s, "/r", r#"{"dependencies":{"graphql":"16.0.0"}}"#);
    seed_file(&s, "/r", "src/a.ts", 1);
    seed_file(&s, "/r", "src/b.ts", 2);
    edge(&s, "/r", "src/a.ts", "UNRESOLVED:external:graphql");
    edge(&s, "/r", "src/b.ts", "UNRESOLVED:external:graphql");

    let r = compute_dependency_linkage("/r", &s).unwrap();
    let g = r.packages.iter().find(|p| p.package_name == "graphql").unwrap();
    assert_eq!(g.observation_count, 2);
    assert_eq!(g.distinct_source_files, 2);
}

#[test]
fn dev_dependency_section_preserved() {
    let s = store();
    put_pkg(
        &s,
        "/r",
        r#"{"devDependencies":{"jest":"^29.0.0"},"dependencies":{"express":"4.0.0"}}"#,
    );
    let r = compute_dependency_linkage("/r", &s).unwrap();
    let jest = r.packages.iter().find(|p| p.package_name == "jest").unwrap();
    assert_eq!(jest.section, DependencySection::DevDependencies);
    let express = r.packages.iter().find(|p| p.package_name == "express").unwrap();
    assert_eq!(express.section, DependencySection::Dependencies);
}

#[test]
fn observed_undeclared_package() {
    let s = store();
    put_pkg(&s, "/r", r#"{"dependencies":{}}"#);
    seed_file(&s, "/r", "src/a.ts", 1);
    edge(&s, "/r", "src/a.ts", "UNRESOLVED:external:secret-pkg");

    let r = compute_dependency_linkage("/r", &s).unwrap();
    let p = r.packages.iter().find(|p| p.package_name == "secret-pkg").unwrap();
    assert!(!p.is_declared);
    assert!(p.is_observed);
    assert_eq!(p.section, DependencySection::Undeclared);
    assert_eq!(r.observed_undeclared, 1);
}

#[test]
fn deterministic_sort_by_observation_count_then_name() {
    let s = store();
    put_pkg(
        &s,
        "/r",
        r#"{"dependencies":{"aaa":"1","zzz":"1","mid":"1"}}"#,
    );
    seed_file(&s, "/r", "src/a.ts", 1);
    edge(&s, "/r", "src/a.ts", "UNRESOLVED:external:zzz");
    edge(&s, "/r", "src/a.ts", "UNRESOLVED:external:mid");
    edge(&s, "/r", "src/a.ts", "UNRESOLVED:external:mid");

    let r = compute_dependency_linkage("/r", &s).unwrap();
    // mid has 2 obs, zzz has 1, aaa has 0
    assert_eq!(r.packages[0].package_name, "mid");
    assert_eq!(r.packages[1].package_name, "zzz");
    assert_eq!(r.packages[2].package_name, "aaa");
}

#[test]
fn no_package_json_still_reports_observed() {
    let s = store();
    seed_file(&s, "/r", "src/a.ts", 1);
    edge(&s, "/r", "src/a.ts", "UNRESOLVED:external:only-obs");
    let r = compute_dependency_linkage("/r", &s).unwrap();
    assert_eq!(r.total_declared, 0);
    assert_eq!(r.total_observed, 1);
    assert!(r.declaration_provenance.contains("no package.json")
        || r.declaration_provenance.contains("working-tree")
        || r.declaration_provenance.contains("not present"));
}
