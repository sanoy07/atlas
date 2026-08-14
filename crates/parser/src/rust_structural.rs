use atlas_ir::{StructuralEdge, StructuralEdgeKind, StructuralEvidence};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const RUST_EXTENSIONS: &[&str] = &["rs"];

pub fn is_rust_file(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| RUST_EXTENSIONS.contains(&e))
        .unwrap_or(false)
}

/// Extract `use crate::...` and `use super::...` edges from a Rust source file.
///
/// We resolve only crate-internal `use` paths — `use std::`, `use tokio::`, etc. are
/// external dependencies and are ignored. Internal paths are those that begin with:
///   - `crate::` — absolute from the crate root
///   - `super::` — relative to the current module's parent
///   - `self::`  — relative to the current module
///
/// Path resolution: map `crate::foo::bar` to `src/foo/bar.rs` or `src/foo/bar/mod.rs`.
pub fn extract_rust_uses(
    source_file: &str,
    content: &str,
    known_files: &HashSet<String>,
) -> Vec<StructuralEdge> {
    let source_dir = Path::new(source_file)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Infer the `src/` root for crate:: resolution by walking up from the file.
    // We look for the first component named "src" in the path.
    let src_root = find_src_root(source_file);

    let mut edges = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Only process `use` declarations. Skip doc comments and non-use lines.
        let rest = match trimmed.strip_prefix("use ") {
            Some(r) => r,
            None => {
                // Also handle `pub use`, `pub(crate) use`, etc.
                if let Some(r) = trimmed
                    .strip_prefix("pub use ")
                    .or_else(|| trimmed.strip_prefix("pub(crate) use "))
                    .or_else(|| trimmed.strip_prefix("pub(super) use "))
                {
                    r
                } else {
                    continue;
                }
            }
        };

        // Strip trailing semicolon and whitespace.
        let rest = rest.trim_end_matches(';').trim();

        // Extract all module paths from the use statement (handles braced groups too).
        let paths = collect_use_paths(rest);

        for path in paths {
            let (prefix, module_path) = if let Some(m) = path.strip_prefix("crate::") {
                ("crate", m)
            } else if let Some(m) = path.strip_prefix("super::") {
                ("super", m)
            } else if let Some(m) = path.strip_prefix("self::") {
                ("self", m)
            } else {
                continue; // external crate — skip
            };

            // Strip the trailing item name (last `::ident` component that is a type/fn name).
            // Module paths use snake_case; type names use PascalCase or are ALL_CAPS.
            // We trim the last component if it looks like an item name, not a module.
            let module_only = trim_item_suffix(module_path);
            if module_only.is_empty() { continue; }

            // Resolve to a file path.
            let resolved = resolve_module_path(
                prefix, module_only, &source_dir, &src_root, known_files
            );

            if let Some(target) = resolved {
                let snippet = line.trim().chars().take(80).collect::<String>();
                edges.push(StructuralEdge {
                    source_file:   source_file.to_string(),
                    source_symbol: None,
                    target_file:   target,
                    target_symbol: None,
                    kind:          StructuralEdgeKind::Imports,
                    evidence:      StructuralEvidence {
                        source_file: source_file.to_string(),
                        line:        Some(line_num as u32 + 1),
                        snippet,
                        extractor:   "rust_uses".to_string(),
                    },
                });
            }
        }
    }

    // Deduplicate: same (source, target) pair may appear from multiple `use` lines.
    edges.sort_by(|a, b| a.target_file.cmp(&b.target_file));
    edges.dedup_by(|a, b| a.source_file == b.source_file && a.target_file == b.target_file);

    edges
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Collect individual module paths from a `use` statement.
/// Handles simple paths and single-level brace groups:
///   `foo::bar`           → ["foo::bar"]
///   `foo::{bar, baz}`    → ["foo::bar", "foo::baz"]
///   `foo::{bar, {nested}}` → simplified (only one brace level)
fn collect_use_paths(rest: &str) -> Vec<String> {
    // Find the first `{` — everything before is the common prefix.
    if let Some(brace) = rest.find('{') {
        let prefix = rest[..brace].trim_end_matches(':');
        let inner_end = rest.find('}').unwrap_or(rest.len());
        let inner = &rest[brace + 1..inner_end];
        inner.split(',')
            .map(|item| item.trim())
            .filter(|item| !item.is_empty() && *item != "_")
            .map(|item| {
                if prefix.is_empty() {
                    item.to_string()
                } else {
                    format!("{}::{}", prefix, item)
                }
            })
            .collect()
    } else {
        // Simple path — strip any trailing `as alias` or `::*`.
        let clean = rest
            .split(" as ").next().unwrap_or(rest)
            .trim_end_matches("::*")
            .trim()
            .to_string();
        if clean.is_empty() { vec![] } else { vec![clean] }
    }
}

/// Strip a trailing item suffix (type, function, constant) from a module path.
/// Module path components are snake_case; types are PascalCase or ALL_CAPS.
/// If the last component looks like a module (snake_case), keep it as-is.
fn trim_item_suffix(module_path: &str) -> &str {
    let parts: Vec<&str> = module_path.split("::").collect();
    if parts.is_empty() { return module_path; }
    let last = *parts.last().unwrap();

    // If the last component is PascalCase, ALL_CAPS, or a glob (*), strip it.
    let is_item = last == "*"
        || last.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
        || (last.chars().all(|c| c.is_uppercase() || c == '_') && last.len() > 1);

    if is_item && parts.len() > 1 {
        // Return everything before the last `::`
        let end = module_path.rfind("::").unwrap_or(module_path.len());
        &module_path[..end]
    } else {
        module_path
    }
}

/// Find the `src/` directory root by looking upward from the file path.
fn find_src_root(source_file: &str) -> String {
    let path = Path::new(source_file);
    let mut components = path.components().collect::<Vec<_>>();
    components.pop(); // remove filename
    for i in (0..components.len()).rev() {
        if components[i].as_os_str() == "src" {
            let root: PathBuf = components[..=i].iter().collect();
            return root.to_string_lossy().into_owned();
        }
    }
    // Fallback: assume source root is same directory as the file.
    path.parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Resolve a Rust module path to a file path in the repository.
/// Returns None if the file cannot be found among known_files.
fn resolve_module_path(
    prefix: &str,
    module_path: &str,
    source_dir: &str,
    src_root: &str,
    known_files: &HashSet<String>,
) -> Option<String> {
    let rel_parts: Vec<&str> = module_path.split("::").collect();

    let base = match prefix {
        "crate" => src_root.to_string(),
        "super" => {
            // One level up from source_dir.
            Path::new(source_dir)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| source_dir.to_string())
        }
        "self" => source_dir.to_string(),
        _ => return None,
    };

    let rel_joined = rel_parts.join("/");
    let candidate_file   = format!("{}/{}.rs", base, rel_joined);
    let candidate_mod    = format!("{}/{}/mod.rs", base, rel_joined);

    // Normalise: strip leading "./" if present.
    let norm = |s: &str| s.trim_start_matches("./").to_string();

    let c1 = norm(&candidate_file);
    let c2 = norm(&candidate_mod);

    if known_files.contains(&c1) {
        Some(c1)
    } else if known_files.contains(&c2) {
        Some(c2)
    } else {
        None
    }
}

// ── File collection ───────────────────────────────────────────────────────────

fn collect_rust_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" || name == "build" { continue; }
            collect_rust_files(root, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(rel) = path.strip_prefix(root) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                out.push((rel_str, path));
            }
        }
    }
}

pub fn repo_has_rust_files(repo_root: &str) -> bool {
    let root = Path::new(repo_root);
    has_rust_files_recursive(root)
}

fn has_rust_files_recursive(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" || name == "build" { continue; }
            if has_rust_files_recursive(&path) { return true; }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
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
    collect_rust_files(root, root, &mut files);

    let known: HashSet<String> = files.iter().map(|(rel, _)| rel.clone()).collect();
    let mut edges    = Vec::new();
    let mut outcomes = Vec::with_capacity(files.len());

    for (rel, abs) in &files {
        match std::fs::read_to_string(abs) {
            Ok(content) => {
                edges.extend(extract_rust_uses(rel, &content, &known));
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
