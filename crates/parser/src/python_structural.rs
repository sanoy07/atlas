use atlas_ir::{StructuralEdge, StructuralEdgeKind, StructuralEvidence};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Extract `import` and `from ... import` edges from a Python source file.
///
/// We resolve only relative imports and same-package absolute imports.
/// External packages (not found in known_files as a `.py` path) are ignored.
pub fn extract_python_imports(
    source_file: &str,
    content: &str,
    known_files: &HashSet<String>,
) -> Vec<StructuralEdge> {
    let source_dir = Path::new(source_file)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut edges = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Skip comments and docstrings.
        if trimmed.starts_with('#') || trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''") {
            continue;
        }

        // Handle `from . import foo`, `from .sub import bar`, `from ..pkg import baz`
        if let Some(from_target) = parse_from_import(trimmed) {
            if let Some(target) = resolve_python_module(
                &from_target, &source_dir, known_files
            ) {
                let snippet = trimmed.chars().take(80).collect::<String>();
                edges.push(make_edge(source_file, target, line_num, snippet));
            }
            continue;
        }

        // Handle `import foo.bar` (simple module import, no `from`)
        if let Some(module) = parse_plain_import(trimmed) {
            if let Some(target) = resolve_python_module(&module, &source_dir, known_files) {
                let snippet = trimmed.chars().take(80).collect::<String>();
                edges.push(make_edge(source_file, target, line_num, snippet));
            }
        }
    }

    // Deduplicate same (source, target) pairs from multiple import lines.
    edges.sort_by(|a, b| a.target_file.cmp(&b.target_file));
    edges.dedup_by(|a, b| a.source_file == b.source_file && a.target_file == b.target_file);

    edges
}

// ── Parsers ───────────────────────────────────────────────────────────────────

/// Parse `from X import Y` — returns `X` as a module path string.
/// Returns None for non-from-import lines.
fn parse_from_import(line: &str) -> Option<String> {
    let rest = line.strip_prefix("from ")?;
    // Find `import` keyword
    let import_pos = rest.find(" import ")?;
    let module = rest[..import_pos].trim();
    Some(module.to_string())
}

/// Parse `import X` (no `from`) — returns the top-level module name.
fn parse_plain_import(line: &str) -> Option<String> {
    let rest = line.strip_prefix("import ")?;
    // Take first module (before comma, `as`, or `(`)
    let module = rest
        .split(',').next()?
        .split(" as ").next()?
        .split('(').next()?
        .trim();
    Some(module.to_string())
}

// ── Resolution ────────────────────────────────────────────────────────────────

/// Resolve a Python module path to a `.py` file path.
///
/// Handles:
///   - Relative imports: `.foo`, `..pkg.sub`
///   - Absolute imports: `pkg.sub` (resolved relative to source dir or repo root)
fn resolve_python_module(
    module: &str,
    source_dir: &str,
    known_files: &HashSet<String>,
) -> Option<String> {
    if module.is_empty() { return None; }

    // Count leading dots for relative imports.
    let dots = module.chars().take_while(|c| *c == '.').count();

    let base_dir = if dots > 0 {
        // Navigate up `dots` levels from source_dir.
        let mut path = Path::new(source_dir).to_path_buf();
        for _ in 0..dots.saturating_sub(1) {
            path = path.parent().map(|p| p.to_path_buf()).unwrap_or(path);
        }
        path.to_string_lossy().into_owned()
    } else {
        source_dir.to_string()
    };

    let module_path = &module[dots..]; // strip leading dots

    if module_path.is_empty() {
        // `from . import X` — the base_dir is the package; look for __init__.py
        let candidate = format!("{}/__init__.py", base_dir);
        let c = candidate.trim_start_matches("./").to_string();
        return if known_files.contains(&c) { Some(c) } else { None };
    }

    // Convert `pkg.sub.mod` → `pkg/sub/mod`
    let rel = module_path.replace('.', "/");

    for base in [base_dir.as_str(), ""] {
        let file_candidate = if base.is_empty() {
            format!("{}.py", rel)
        } else {
            format!("{}/{}.py", base, rel)
        };
        let init_candidate = if base.is_empty() {
            format!("{}/__init__.py", rel)
        } else {
            format!("{}/{}/__init__.py", base, rel)
        };

        let fc = file_candidate.trim_start_matches("./").to_string();
        let ic = init_candidate.trim_start_matches("./").to_string();

        if known_files.contains(&fc) { return Some(fc); }
        if known_files.contains(&ic) { return Some(ic); }
    }

    None
}

fn make_edge(source_file: &str, target_file: String, line_num: usize, snippet: String) -> StructuralEdge {
    StructuralEdge {
        source_file:   source_file.to_string(),
        source_symbol: None,
        target_file,
        target_symbol: None,
        kind:          StructuralEdgeKind::Imports,
        evidence:      StructuralEvidence {
            source_file: source_file.to_string(),
            line:        Some(line_num as u32 + 1),
            snippet,
            extractor:   "python_imports".to_string(),
        },
    }
}

// ── File collection ───────────────────────────────────────────────────────────

fn collect_python_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "__pycache__" || name == "venv"
                || name == ".venv" || name == "node_modules" || name == "target" { continue; }
            collect_python_files(root, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("py") {
            if let Ok(rel) = path.strip_prefix(root) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                out.push((rel_str, path));
            }
        }
    }
}

pub fn repo_has_python_files(repo_root: &str) -> bool {
    let root = Path::new(repo_root);
    has_python_files_recursive(root)
}

fn has_python_files_recursive(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "__pycache__" || name == "venv"
                || name == ".venv" || name == "target" { continue; }
            if has_python_files_recursive(&path) { return true; }
        } else if path.extension().and_then(|e| e.to_str()) == Some("py") {
            return true;
        }
    }
    false
}

pub fn extract_all(repo_root: &str) -> Vec<StructuralEdge> {
    extract_all_with_outcomes(repo_root).0
}

pub fn extract_all_with_outcomes(
    repo_root: &str,
) -> (Vec<StructuralEdge>, Vec<crate::FileAnalysis>) {
    let root = Path::new(repo_root);
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    collect_python_files(root, root, &mut files);

    let known: HashSet<String> = files.iter().map(|(rel, _)| rel.clone()).collect();
    let mut edges    = Vec::new();
    let mut outcomes = Vec::with_capacity(files.len());

    for (rel, abs) in &files {
        match std::fs::read_to_string(abs) {
            Ok(content) => {
                edges.extend(extract_python_imports(rel, &content, &known));
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
