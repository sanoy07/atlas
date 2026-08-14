use atlas_ir::{StructuralEdge, StructuralEdgeKind, StructuralEvidence};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const C_EXTENSIONS: &[&str] = &["c", "h", "cu", "cuh", "m", "cpp", "cc", "hpp"];

pub fn is_c_file(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| C_EXTENSIONS.contains(&e))
        .unwrap_or(false)
}

/// Extract `#include "local.h"` edges from a single C/C++/CUDA/ObjC source file.
/// System includes (`#include <...>`) are ignored — only local includes matter.
pub fn extract_c_includes(
    source_file: &str,
    content: &str,
    known_files: &HashSet<String>,
) -> Vec<StructuralEdge> {
    let source_dir = Path::new(source_file)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut edges = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        let rest = match line.strip_prefix("#include") {
            Some(r) => r.trim(),
            None => continue,
        };
        let rest = match rest.strip_prefix('"') {
            Some(r) => r,
            None => continue, // skip system includes (<...>)
        };
        let end = match rest.find('"') {
            Some(i) => i,
            None => continue,
        };
        let include = &rest[..end];

        // Try relative to the including file's directory, then from repo root.
        let candidate = if source_dir.is_empty() {
            include.to_string()
        } else {
            normalize_path(&format!("{}/{}", source_dir, include))
        };

        let resolved = if known_files.contains(&candidate) {
            candidate
        } else if known_files.contains(include) {
            include.to_string()
        } else {
            continue; // not a file in this repo (probably vendored or generated)
        };

        edges.push(StructuralEdge {
            source_file:   source_file.to_string(),
            source_symbol: None,
            target_file:   resolved,
            target_symbol: None,
            kind:          StructuralEdgeKind::Imports,
            evidence:      StructuralEvidence {
                source_file: source_file.to_string(),
                line:        None,
                snippet:     format!("#include \"{}\"", include),
                extractor:   "c_includes".to_string(),
            },
        });
    }

    edges
}

/// Recursively collect all C-family source files under `root`.
fn collect_c_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Skip hidden directories and common build output directories.
            if name.starts_with('.') || name == "target" || name == "build" { continue; }
            collect_c_files(root, &path, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if C_EXTENSIONS.contains(&ext) {
                if let Ok(rel) = path.strip_prefix(root) {
                    let rel_str = rel.to_string_lossy().replace('\\', "/");
                    out.push((rel_str, path));
                }
            }
        }
    }
}

/// Returns true if `repo_root` contains any C-family source files.
/// Used by the ingest pipeline to decide whether to run C structural extraction.
pub fn repo_has_c_files(repo_root: &str) -> bool {
    let root = Path::new(repo_root);
    has_c_files_recursive(root)
}

fn has_c_files_recursive(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" || name == "build" { continue; }
            if has_c_files_recursive(&path) { return true; }
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if C_EXTENSIONS.contains(&ext) { return true; }
        }
    }
    false
}

/// Walk all C-family files in `repo_root` and extract include edges.
///
/// Backwards-compat shim over `extract_all_with_outcomes`.
pub fn extract_all(repo_root: &str) -> Vec<StructuralEdge> {
    extract_all_with_outcomes(repo_root).0
}

/// Extract include edges AND per-file outcomes for status stamping.
pub fn extract_all_with_outcomes(
    repo_root: &str,
) -> (Vec<StructuralEdge>, Vec<crate::FileAnalysis>) {
    let root = Path::new(repo_root);
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    collect_c_files(root, root, &mut files);

    let known: HashSet<String> = files.iter().map(|(rel, _)| rel.clone()).collect();
    let mut edges    = Vec::new();
    let mut outcomes = Vec::with_capacity(files.len());

    for (rel, abs) in &files {
        match std::fs::read_to_string(abs) {
            Ok(content) => {
                edges.extend(extract_c_includes(rel, &content, &known));
                outcomes.push(crate::FileAnalysis {
                    file:   rel.clone(),
                    status: crate::FileAnalysisStatus::Analyzed,
                });
            }
            Err(err) => {
                let reason = if err.kind() == std::io::ErrorKind::InvalidData {
                    "invalid utf-8".to_string()
                } else {
                    format!("read error: {}", err.kind())
                };
                outcomes.push(crate::FileAnalysis {
                    file:   rel.clone(),
                    status: crate::FileAnalysisStatus::ParserFailure { reason },
                });
            }
        }
    }

    (edges, outcomes)
}

fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => { parts.pop(); }
            c => parts.push(c),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_local_includes_skips_system() {
        let known: HashSet<String> = ["ds4.h", "ds4_kvstore.h", "rax.h"]
            .iter().map(|s| s.to_string()).collect();
        let content = r#"
#include "ds4.h"
#include <stdio.h>
#include "ds4_kvstore.h"
#include "missing.h"
        "#;
        let edges = extract_c_includes("ds4_server.c", content, &known);
        let targets: Vec<&str> = edges.iter().map(|e| e.target_file.as_str()).collect();
        assert!(targets.contains(&"ds4.h"),        "got: {:?}", targets);
        assert!(targets.contains(&"ds4_kvstore.h"), "got: {:?}", targets);
        assert!(!targets.contains(&"stdio.h"),     "system include leaked");
        assert!(!targets.contains(&"missing.h"),   "unresolved include leaked");
    }

    #[test]
    fn resolves_relative_include() {
        let known: HashSet<String> = ["rocm/ds4_rocm.h".to_string()].into_iter().collect();
        let content = r#"#include "ds4_rocm.h""#;
        let edges = extract_c_includes("rocm/ds4_rocm.cu", content, &known);
        assert_eq!(edges.len(), 1, "got: {:?}", edges);
        assert_eq!(edges[0].target_file, "rocm/ds4_rocm.h");
    }

    #[test]
    fn is_c_file_recognizes_extensions() {
        assert!(is_c_file("ds4.c"));
        assert!(is_c_file("ds4_cuda.cu"));
        assert!(is_c_file("ds4_metal.m"));
        assert!(is_c_file("ds4.h"));
        assert!(!is_c_file("ds4.rs"));
        assert!(!is_c_file("ds4.py"));
    }
}
