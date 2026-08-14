//! B10: configuration artifact provenance.

use atlas_core::{compute_config_inventory, compute_config_provenance};
use atlas_ir::Commit;
use atlas_storage::Store;
use chrono::{DateTime, Utc};

fn store() -> Store {
    Store::open(":memory:").unwrap()
}

fn commit(s: &Store, repo: &str, hash: &str, ts: i64, files: &[&str]) {
    let c = Commit {
        hash: hash.into(),
        short_hash: hash[..7.min(hash.len())].into(),
        message: format!("msg {}", hash),
        author_name: "Alice".into(),
        author_email: "a@x.com".into(),
        timestamp: DateTime::<Utc>::from_timestamp(ts, 0).unwrap(),
        files_changed: files.iter().map(|f| f.to_string()).collect(),
        parents: vec![],
    };
    s.insert_commit(&c, repo).unwrap();
}

#[test]
fn inventory_lists_artifacts() {
    let s = store();
    s.insert_configuration_artifact("/r", "package.json", "package_json", "{}", "aa")
        .unwrap();
    s.insert_configuration_artifact("/r", "tsconfig.json", "tsconfig", "{}", "bb")
        .unwrap();
    commit(&s, "/r", "c1aaaaa", 100, &["package.json"]);

    let inv = compute_config_inventory("/r", &s).unwrap();
    assert_eq!(inv.total_artifacts, 2);
    assert_eq!(inv.artifacts[0].file_path, "package.json");
    assert_eq!(inv.artifacts[0].touching_commit_count, 1);
    assert_eq!(inv.artifacts[1].file_path, "tsconfig.json");
    assert_eq!(inv.artifacts[1].touching_commit_count, 0);
}

#[test]
fn provenance_reports_commits_and_sha() {
    let s = store();
    s.insert_configuration_artifact(
        "/r",
        "package.json",
        "package_json",
        r#"{"name":"x"}"#,
        "sha123",
    )
    .unwrap();
    commit(&s, "/r", "c1aaaaa", 100, &["package.json"]);
    commit(&s, "/r", "c2bbbbb", 200, &["package.json"]);

    let r = compute_config_provenance("package.json", "/r", &s).unwrap();
    assert!(r.artifact_present);
    assert_eq!(r.sha256.as_deref(), Some("sha123"));
    assert_eq!(r.touching_commit_count, 2);
    assert_eq!(r.first_touch, Some(100));
    assert_eq!(r.last_touch, Some(200));
    assert!(r.limitations.iter().any(|l| l.contains("CURRENT")));
}

#[test]
fn missing_artifact_still_reports_history() {
    let s = store();
    commit(&s, "/r", "c1aaaaa", 100, &["Cargo.toml"]);
    let r = compute_config_provenance("Cargo.toml", "/r", &s).unwrap();
    assert!(!r.artifact_present);
    assert_eq!(r.touching_commit_count, 1);
    assert!(r.limitations.iter().any(|l| l.contains("No configuration_artifacts")));
}

#[test]
fn sha_consistency() {
    let s = store();
    s.insert_configuration_artifact("/r", "package.json", "package_json", "BODY", "deadbeef")
        .unwrap();
    let r = compute_config_provenance("package.json", "/r", &s).unwrap();
    assert_eq!(r.sha256.as_deref(), Some("deadbeef"));
    assert_eq!(r.content_byte_len, Some(4));
}

#[test]
fn historical_path_redirect_when_identity_exists() {
    let s = store();
    commit(&s, "/r", "intro01", 100, &["old-config.json"]);
    commit(&s, "/r", "rename1", 200, &["package.json"]);
    s.insert_configuration_artifact("/r", "package.json", "package_json", "{}", "zz")
        .unwrap();

    let id = s.insert_file_identity("/r").unwrap();
    s.insert_path_observation(id, "old-config.json", "intro01", Some("rename1"), "/r")
        .unwrap();
    s.insert_path_observation(id, "package.json", "rename1", None, "/r")
        .unwrap();
    s.populate_identity_commits("/r").unwrap();

    let r = compute_config_provenance("old-config.json", "/r", &s).unwrap();
    let rn = r.redirect_note.expect("redirect");
    assert_eq!(rn.original_subject, "old-config.json");
    assert_eq!(rn.current_path, "package.json");
    assert_eq!(r.file_path, "package.json");
    assert!(r.identity_id.is_some());
    assert!(r.identity_commit_count.unwrap_or(0) >= 1);
}

#[test]
fn repo_isolation() {
    let s = store();
    s.insert_configuration_artifact("/r/a", "package.json", "package_json", "A", "aa")
        .unwrap();
    s.insert_configuration_artifact("/r/b", "package.json", "package_json", "B", "bb")
        .unwrap();
    commit(&s, "/r/a", "caaaaaa", 100, &["package.json"]);
    commit(&s, "/r/b", "cbbbbbb", 200, &["package.json"]);

    let ra = compute_config_provenance("package.json", "/r/a", &s).unwrap();
    assert_eq!(ra.sha256.as_deref(), Some("aa"));
    assert_eq!(ra.touching_commit_count, 1);
    assert_eq!(ra.touching_commits[0].hash, "caaaaaa");

    let rb = compute_config_provenance("package.json", "/r/b", &s).unwrap();
    assert_eq!(rb.sha256.as_deref(), Some("bb"));
    assert_eq!(rb.touching_commits[0].hash, "cbbbbbb");
}
