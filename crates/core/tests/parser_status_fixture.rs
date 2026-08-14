//! End-to-end acceptance tests for real `parser_failure` semantics.
//!
//! Verifies that after ingest, `files.analysis_status` distinguishes:
//!   - `analyzed`             — a source file the extractor read and parsed
//!   - `parser_failure`       — a source file the extractor tried and failed on
//!   - `not_analyzed_language` — a source-looking file whose language has no extractor
//!   - `not_source_file`      — everything else (docs, config, assets)
//!
//! These are per-file, authoritative facts on `files.analysis_status`, not
//! reconstructions from `structural_edges`.  A file with zero edges is
//! genuinely different from a file the parser could not read.

use atlas_core::{ingest_typescript, stamp_analysis_status};
use atlas_storage::Store;
use std::io::Write;
use std::path::Path;
use tempfile::TempDir;

fn temp_repo() -> (TempDir, String, Store) {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path().to_string_lossy().into_owned();
    let db   = dir.path().join("atlas.db");
    let store = Store::open(db.to_str().unwrap()).expect("store");
    (dir, repo, store)
}

fn write_text(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Write a file whose bytes are deliberately not valid UTF-8, so
/// `std::fs::read_to_string` returns `InvalidData`.
fn write_invalid_utf8(root: &Path, rel: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = std::fs::File::create(path).unwrap();
    // Lone continuation byte (0x80) with no leading byte — invalid UTF-8.
    f.write_all(&[0x80, 0x80, 0x80, 0x0a]).unwrap();
}

/// Insert a bare `files` row (as if a commit had touched it), so the final
/// `stamp_analysis_status` sweep sees the file.  Extractors then override
/// the status if they attempted the file.
fn register_file(store: &Store, repo_path: &str, rel: &str) {
    use atlas_ir::Commit;
    use chrono::{DateTime, Utc};
    let hash = format!("h{:08x}", rel.as_bytes().iter().map(|&b| b as u32).sum::<u32>());
    let c = Commit {
        hash:          hash.clone(),
        short_hash:    hash[..7].to_string(),
        message:       "seed".into(),
        author_name:   "T".into(),
        author_email:  "t@x".into(),
        timestamp:     DateTime::<Utc>::from_timestamp(1, 0).unwrap(),
        files_changed: vec![rel.to_string()],
        parents:       vec![],
    };
    store.insert_commit(&c, repo_path).unwrap();
}

fn status_of(store: &Store, path: &str, repo_path: &str) -> Option<String> {
    store.analysis_status(path, repo_path).unwrap()
}

#[test]
fn ts_source_file_reads_cleanly_gets_analyzed() {
    let (dir, repo, store) = temp_repo();
    write_text(dir.path(), "src/service.ts", "export const x = 1;\n");
    register_file(&store, &repo, "src/service.ts");

    ingest_typescript(&repo, &store).unwrap();
    stamp_analysis_status(&repo, &store).unwrap();

    assert_eq!(status_of(&store, "src/service.ts", &repo).as_deref(), Some("analyzed"),
        "a readable .ts file must be marked `analyzed`");
}

#[test]
fn ts_source_with_invalid_utf8_gets_parser_failure() {
    let (dir, repo, store) = temp_repo();
    write_invalid_utf8(dir.path(), "src/broken.ts");
    register_file(&store, &repo, "src/broken.ts");

    ingest_typescript(&repo, &store).unwrap();
    stamp_analysis_status(&repo, &store).unwrap();

    assert_eq!(status_of(&store, "src/broken.ts", &repo).as_deref(), Some("parser_failure"),
        "a .ts file that fails to decode must be marked `parser_failure`, not analyzed or unknown");
}

#[test]
fn unsupported_language_gets_not_analyzed_language() {
    let (dir, repo, store) = temp_repo();
    // No extractor for Go, Kotlin, Elixir.
    write_text(dir.path(), "cmd/main.go",     "package main\n");
    write_text(dir.path(), "app/App.kt",      "class App {}\n");
    write_text(dir.path(), "lib/mod.ex",      "defmodule M do end\n");
    for f in ["cmd/main.go", "app/App.kt", "lib/mod.ex"] {
        register_file(&store, &repo, f);
    }

    ingest_typescript(&repo, &store).unwrap();
    stamp_analysis_status(&repo, &store).unwrap();

    for f in ["cmd/main.go", "app/App.kt", "lib/mod.ex"] {
        assert_eq!(status_of(&store, f, &repo).as_deref(), Some("not_analyzed_language"),
            "{f} should be classified as `not_analyzed_language` — extension is a source language Atlas doesn't parse");
    }
}

#[test]
fn non_source_files_get_not_source_file() {
    let (dir, repo, store) = temp_repo();
    write_text(dir.path(), "README.md",       "# Docs\n");
    write_text(dir.path(), "assets/logo.png", "not really a png\n");
    write_text(dir.path(), "config.yaml",     "key: value\n");
    for f in ["README.md", "assets/logo.png", "config.yaml"] {
        register_file(&store, &repo, f);
    }

    ingest_typescript(&repo, &store).unwrap();
    stamp_analysis_status(&repo, &store).unwrap();

    for f in ["README.md", "assets/logo.png", "config.yaml"] {
        assert_eq!(status_of(&store, f, &repo).as_deref(), Some("not_source_file"),
            "{f} should be `not_source_file` — no extractor could ever consume this");
    }
}

#[test]
fn extractor_authoritative_over_extension_fallback() {
    // A .ts file that parses successfully must NOT be later overwritten by the
    // extension-based `stamp_analysis_status` sweep.  Extractor is authoritative.
    let (dir, repo, store) = temp_repo();
    write_text(dir.path(), "src/service.ts", "export const x = 1;\n");
    register_file(&store, &repo, "src/service.ts");

    // Simulate the stamp running BEFORE the extractor: it would set "analyzed"
    // via the extension.  Then the extractor runs and re-affirms.
    stamp_analysis_status(&repo, &store).unwrap();
    assert_eq!(status_of(&store, "src/service.ts", &repo).as_deref(), Some("analyzed"));

    ingest_typescript(&repo, &store).unwrap();
    // Even after re-running stamp, extractor's "analyzed" remains.
    stamp_analysis_status(&repo, &store).unwrap();
    assert_eq!(status_of(&store, "src/service.ts", &repo).as_deref(), Some("analyzed"),
        "extractor's decision must be preserved through the stamp sweep");
}

#[test]
fn extractor_parser_failure_survives_final_stamp() {
    // parser_failure written by the extractor must NOT be overwritten to
    // `analyzed` by the extension-based sweep.  If the sweep ran first, the
    // extractor should still upgrade to parser_failure.
    let (dir, repo, store) = temp_repo();
    write_invalid_utf8(dir.path(), "src/broken.ts");
    register_file(&store, &repo, "src/broken.ts");

    ingest_typescript(&repo, &store).unwrap();
    assert_eq!(status_of(&store, "src/broken.ts", &repo).as_deref(), Some("parser_failure"));

    // Final sweep must not overwrite parser_failure.
    stamp_analysis_status(&repo, &store).unwrap();
    assert_eq!(status_of(&store, "src/broken.ts", &repo).as_deref(), Some("parser_failure"),
        "parser_failure must survive the extension-based sweep");
}
