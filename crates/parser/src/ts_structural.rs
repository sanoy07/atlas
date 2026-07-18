use atlas_ir::{StructuralEdge, StructuralEdgeKind, StructuralEvidence};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ─── Mongoose query method names ─────────────────────────────────────────────

const MONGOOSE_QUERY_METHODS: &[&str] = &[
    "find",
    "findOne",
    "findById",
    "findByIdAndUpdate",
    "findByIdAndDelete",
    "findOneAndUpdate",
    "findOneAndDelete",
    "create",
    "insertMany",
    "updateOne",
    "updateMany",
    "deleteOne",
    "deleteMany",
    "countDocuments",
    "exists",
    "aggregate",
];

// ─── Public entry point ───────────────────────────────────────────────────────

/// Extract all observable structural edges from a single TypeScript source file.
///
/// `source_file`  — repo-relative path to this file (e.g. "src/modules/core/services/listing.service.ts")
/// `content`      — full text of the file
/// `known_files`  — set of all repo-relative `.ts` paths (used for import resolution)
/// `repo_root`    — absolute path to the repository root
///
/// Unresolved imports produce edges where `target_file` is prefixed "UNRESOLVED:".
pub fn extract_structural_edges(
    source_file: &str,
    content: &str,
    known_files: &HashSet<String>,
    repo_root: &str,
) -> Vec<StructuralEdge> {
    extract_structural_edges_with_aliases(source_file, content, known_files, repo_root, &HashMap::new())
}

fn extract_structural_edges_with_aliases(
    source_file: &str,
    content: &str,
    known_files: &HashSet<String>,
    repo_root: &str,
    aliases: &HashMap<String, String>,
) -> Vec<StructuralEdge> {
    let mut edges: Vec<StructuralEdge> = Vec::new();

    // Pass 1: collect all import statements.
    // Maps imported symbol name → resolved target file (or "UNRESOLVED:..." string).
    let (import_edges, symbol_to_file) =
        extract_imports(source_file, content, known_files, repo_root, aliases);
    edges.extend(import_edges);

    // Pass 2: extract static calls and model references using the symbol map.
    edges.extend(extract_calls(source_file, content, &symbol_to_file));

    edges
}

// ─── Pass 1 — Import extraction ───────────────────────────────────────────────

/// Parse ES import statements and return:
/// - A Vec of IMPORTS edges
/// - A map from imported symbol name → resolved target file path
fn extract_imports(
    source_file: &str,
    content: &str,
    known_files: &HashSet<String>,
    repo_root: &str,
    aliases: &HashMap<String, String>,
) -> (Vec<StructuralEdge>, HashMap<String, String>) {
    let mut edges: Vec<StructuralEdge> = Vec::new();
    let mut symbol_to_file: HashMap<String, String> = HashMap::new();

    for (line_idx, line) in content.lines().enumerate() {
        let line_num = (line_idx + 1) as u32;
        let trimmed = line.trim();

        // Skip comment lines.
        if trimmed.starts_with("//") || trimmed.starts_with("*") || trimmed.starts_with("/*") {
            continue;
        }

        // Named import:  import { A, B } from "./path.js"
        // Side-effect:   import "./path.js"
        // Type import:   import type { A } from "./path.js"  (still track — type deps matter)
        if !trimmed.starts_with("import ") {
            continue;
        }

        // Extract the from-path — handles both named (`from "..."`) and
        // side-effect (`import "./path.js"`) forms.
        let Some(target_raw) = extract_from_path(trimmed)
            .or_else(|| extract_side_effect_path(trimmed))
        else { continue };

        // Resolve: relative → resolve from source dir; alias → resolve via tsconfig paths; else external.
        let resolved = if target_raw.starts_with('.') {
            resolve_import(source_file, &target_raw, known_files, repo_root)
        } else if let Some(alias_resolved) = resolve_alias(&target_raw, aliases, known_files) {
            alias_resolved
        } else {
            format!("UNRESOLVED:external:{}", target_raw)
        };

        // Extract named symbols: "import { A, B } from ..." → ["A", "B"]
        let symbols = extract_named_symbols(trimmed);

        // Register symbol → file mapping for Pass 2.
        for sym in &symbols {
            symbol_to_file.insert(sym.clone(), resolved.clone());
        }

        let edge = StructuralEdge {
            source_file:   source_file.to_string(),
            source_symbol: None,
            target_file:   resolved,
            target_symbol: None,
            kind:          StructuralEdgeKind::Imports,
            evidence:      StructuralEvidence {
                source_file: source_file.to_string(),
                line:        Some(line_num),
                snippet:     trimmed.to_string(),
                extractor:   "typescript-es-import".to_string(),
            },
        };
        edges.push(edge);
    }

    (edges, symbol_to_file)
}

// ─── Pass 2 — Call extraction ─────────────────────────────────────────────────

fn extract_calls(
    source_file: &str,
    content: &str,
    symbol_to_file: &HashMap<String, String>,
) -> Vec<StructuralEdge> {
    let mut edges: Vec<StructuralEdge> = Vec::new();

    for (line_idx, line) in content.lines().enumerate() {
        let line_num = (line_idx + 1) as u32;
        let trimmed = line.trim();

        if trimmed.starts_with("//") || trimmed.starts_with("*") || trimmed.starts_with("/*") {
            continue;
        }

        // Find all occurrences of "Identifier.method(" in this line.
        // We scan character by character to handle multiple calls per line.
        let bytes = trimmed.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            // Find the next '.' that follows an identifier.
            let Some(dot_pos) = find_next_dot(trimmed, i) else { break };
            i = dot_pos + 1;

            // Read identifier before the dot (reverse scan from dot_pos-1).
            let Some(object_name) = read_ident_before(trimmed, dot_pos) else { continue };

            let first = object_name.chars().next().unwrap_or('a');
            let is_upper = first.is_uppercase();

            // Lowercase identifiers are only interesting if they were explicitly
            // imported — check the map first so we don't waste work.
            if !is_upper && !symbol_to_file.contains_key(object_name) {
                continue;
            }

            // Read method name after the dot.
            let Some((method_name, has_call_paren)) = read_ident_after(trimmed, i) else {
                continue
            };
            if !has_call_paren {
                continue;
            }

            // Look up which file this object was imported from.
            let Some(target_file) = symbol_to_file.get(object_name) else { continue };

            // Classify:
            //   uppercase + mongoose query → ReferencesModel
            //   uppercase + other          → CallsStatic
            //   lowercase (imported)       → CallsInstance
            let kind = if !is_upper {
                StructuralEdgeKind::CallsInstance
            } else if is_mongoose_query(&method_name) {
                StructuralEdgeKind::ReferencesModel
            } else {
                StructuralEdgeKind::CallsStatic
            };

            let extractor = match kind {
                StructuralEdgeKind::ReferencesModel => "typescript-mongoose-ref",
                StructuralEdgeKind::CallsStatic     => "typescript-static-call",
                StructuralEdgeKind::CallsInstance   => "typescript-instance-call",
                StructuralEdgeKind::Imports         => unreachable!(),
            };

            let target_symbol = format!("{}.{}", object_name, method_name);

            edges.push(StructuralEdge {
                source_file:   source_file.to_string(),
                source_symbol: None,
                target_file:   target_file.clone(),
                target_symbol: Some(target_symbol.clone()),
                kind,
                evidence:      StructuralEvidence {
                    source_file: source_file.to_string(),
                    line:        Some(line_num),
                    snippet:     trimmed.to_string(),
                    extractor:   extractor.to_string(),
                },
            });
        }
    }

    // Deduplicate by (target_file, target_symbol, kind) — keep first occurrence.
    let mut seen: HashSet<(String, Option<String>, String)> = HashSet::new();
    edges.retain(|e| {
        let kind_str = format!("{:?}", e.kind);
        seen.insert((e.target_file.clone(), e.target_symbol.clone(), kind_str))
    });

    edges
}

// ─── Path alias resolution ────────────────────────────────────────────────────

/// Read tsconfig.json at `{repo_root}/tsconfig.json` and extract `compilerOptions.paths`
/// as a map of alias-prefix → replacement-prefix.
/// e.g. `"@/*": ["./src/*"]` → `{"@/" → "src/"}`.
/// Returns an empty map if the file is absent, unparseable, or has no paths.
fn load_path_aliases(repo_root: &str) -> HashMap<String, String> {
    let tsconfig_path = format!("{}/tsconfig.json", repo_root);
    let Ok(raw) = std::fs::read_to_string(&tsconfig_path) else {
        return HashMap::new();
    };

    // tsconfig supports JSONC (JSON with comments). Strip full-line // comments before parsing.
    let stripped: String = raw
        .lines()
        .map(|line| if line.trim().starts_with("//") { "" } else { line })
        .collect::<Vec<_>>()
        .join("\n");

    let Ok(json) = serde_json::from_str::<serde_json::Value>(&stripped) else {
        return HashMap::new();
    };

    let Some(paths_obj) = json
        .get("compilerOptions")
        .and_then(|c| c.get("paths"))
        .and_then(|p| p.as_object())
    else {
        return HashMap::new();
    };

    let mut aliases = HashMap::new();
    for (pattern, replacements) in paths_obj {
        // "@/*" → alias_prefix = "@/"
        let alias_prefix = pattern.trim_end_matches('*');
        if alias_prefix.is_empty() { continue; }

        // First replacement: "./src/*" or "src/*" → replacement_prefix = "src/"
        let Some(first) = replacements
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
        else { continue };

        let replacement_prefix = first
            .trim_start_matches("./")
            .trim_end_matches('*');

        aliases.insert(alias_prefix.to_string(), replacement_prefix.to_string());
    }

    aliases
}

/// Try to resolve a non-relative import specifier via tsconfig path aliases.
/// Returns `Some(repo_relative_path)` when an alias matches and the target exists.
fn resolve_alias(
    specifier: &str,
    aliases: &HashMap<String, String>,
    known_files: &HashSet<String>,
) -> Option<String> {
    for (alias_prefix, replacement_prefix) in aliases {
        let Some(suffix) = specifier.strip_prefix(alias_prefix.as_str()) else { continue };
        let base = format!("{}{}", replacement_prefix, suffix);
        let stripped = base.strip_suffix(".js").unwrap_or(&base);
        let candidates = [
            format!("{}.ts", stripped),
            format!("{}.tsx", stripped),
            format!("{}/index.ts", stripped),
            format!("{}/index.tsx", stripped),
            stripped.to_string(),
        ];
        for candidate in &candidates {
            if known_files.contains(candidate.as_str()) {
                return Some(candidate.clone());
            }
        }
    }
    None
}

// ─── Path resolution ──────────────────────────────────────────────────────────

/// Resolve a relative TypeScript import specifier to a repo-relative `.ts` path.
/// Handles: `.js` extension rewriting, `index.ts` fallback, bare directory.
/// Returns "UNRESOLVED:<reason>:<original>" when resolution fails.
fn resolve_import(
    source_file: &str,
    specifier: &str,
    known_files: &HashSet<String>,
    _repo_root: &str,
) -> String {
    let source_dir = Path::new(source_file)
        .parent()
        .unwrap_or(Path::new(""));

    // Normalise the specifier: strip `.js` so we can try `.ts`.
    let stripped = specifier.strip_suffix(".js").unwrap_or(specifier);
    let base_path = source_dir.join(stripped);
    let normalised = normalise_path(&base_path);

    // Try candidates in order.
    let candidates = [
        format!("{}.ts", normalised),
        format!("{}.tsx", normalised),
        format!("{}/index.ts", normalised),
        format!("{}/index.tsx", normalised),
    ];

    for candidate in &candidates {
        if known_files.contains(candidate.as_str()) {
            return candidate.clone();
        }
    }

    format!("UNRESOLVED:not-found:{}", specifier)
}

/// Normalise a PathBuf to a forward-slash repo-relative string,
/// collapsing `..` and `.` components without touching the filesystem.
fn normalise_path(path: &Path) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => { parts.pop(); }
            std::path::Component::CurDir    => {}
            std::path::Component::Normal(s) => parts.push(s.to_str().unwrap_or("")),
            std::path::Component::RootDir   => parts.clear(),
            _                               => {}
        }
    }
    parts.join("/")
}

// ─── Text parsing helpers ─────────────────────────────────────────────────────

/// Extract the path string from a from-clause, e.g.:
/// `import { X } from "./foo.js"` → `"./foo.js"`
fn extract_from_path(line: &str) -> Option<String> {
    // Find `from "..."` or `from '...'` at the end of the statement.
    let from_idx = line.rfind(" from ")?;
    let after = line[from_idx + 6..].trim();
    let (quote, rest) = after.split_at(1);
    if quote != "\"" && quote != "'" {
        // Could be a side-effect import: `import "./path.js"`
        // In that case there's no `from`, handle separately.
        return None;
    }
    let end = rest.find(|c| c == '"' || c == '\'')?;
    Some(rest[..end].to_string())
}

/// For lines like `import "./path.js"` (side-effect, no `from`).
fn extract_side_effect_path(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with("import ") { return None; }
    if trimmed.contains(" from ") { return None; }
    let after = trimmed["import ".len()..].trim();
    if after.is_empty() { return None; }
    let quote = &after[..1];
    if quote != "\"" && quote != "'" { return None; }
    let rest = &after[1..];
    let end = rest.find(|c| c == '"' || c == '\'')?;
    Some(rest[..end].to_string())
}

/// Parse named symbols from `import { A, B as C } from "..."`.
/// Returns ["A", "C"] (alias targets, not originals, since that's what's in scope).
/// Returns empty vec for side-effect or default imports.
fn extract_named_symbols(line: &str) -> Vec<String> {
    let Some(open)  = line.find('{') else { return vec![] };
    let Some(close) = line.find('}') else { return vec![] };
    if close <= open { return vec![]; }
    let inner = &line[open + 1..close];
    inner
        .split(',')
        .map(|s| {
            let s = s.trim();
            // Handle "Original as Alias" — use the local name.
            if let Some(pos) = s.find(" as ") {
                s[pos + 4..].trim().to_string()
            } else {
                s.to_string()
            }
        })
        .filter(|s| !s.is_empty() && s.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false))
        .collect()
}

/// Find the next `.` in `s` starting at byte `from`, skipping string literals.
fn find_next_dot(s: &str, from: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut in_string: Option<u8> = None;
    for i in from..bytes.len() {
        match bytes[i] {
            b'"' | b'\'' | b'`' => {
                if in_string == Some(bytes[i]) {
                    in_string = None;
                } else if in_string.is_none() {
                    in_string = Some(bytes[i]);
                }
            }
            b'.' if in_string.is_none() => return Some(i),
            _ => {}
        }
    }
    None
}

/// Read an identifier immediately before byte position `dot_pos` in `s`.
fn read_ident_before(s: &str, dot_pos: usize) -> Option<&str> {
    let bytes = s.as_bytes();
    if dot_pos == 0 { return None; }
    let mut end = dot_pos;
    // Skip any whitespace before the dot.
    while end > 0 && bytes[end - 1] == b' ' { end -= 1; }
    if end == 0 { return None; }
    let mut start = end;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    if start == end { return None; }
    Some(&s[start..end])
}

/// Read the identifier starting at byte position `from` in `s`.
/// Returns (name, has_open_paren) — has_open_paren is true if `(` follows immediately.
fn read_ident_after(s: &str, from: usize) -> Option<(String, bool)> {
    let bytes = s.as_bytes();
    let start = from;
    let mut end = start;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    if end == start { return None; }
    let name = s[start..end].to_string();
    let has_paren = end < bytes.len() && bytes[end] == b'(';
    Some((name, has_paren))
}

fn is_mongoose_query(method: &str) -> bool {
    MONGOOSE_QUERY_METHODS.contains(&method)
}

// ─── Walk a directory ─────────────────────────────────────────────────────────

/// Collect all `.ts` and `.tsx` files under `root` as repo-relative paths.
pub fn collect_ts_files(root: &str) -> Vec<(String, PathBuf)> {
    let mut result = Vec::new();
    let root_path = Path::new(root);
    collect_recursive(root_path, root_path, &mut result);
    result
}

fn collect_recursive(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "node_modules" || name.starts_with('.') { continue; }
            collect_recursive(root, &path, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext == "ts" || ext == "tsx" {
                if let Ok(rel) = path.strip_prefix(root) {
                    let rel_str = rel.to_string_lossy().replace('\\', "/");
                    out.push((rel_str, path));
                }
            }
        }
    }
}

// ─── Integration entry point ──────────────────────────────────────────────────

/// Extract all structural edges from all TypeScript files under `repo_root`.
/// Returns (edges, file_count) where file_count is the number of `.ts` files processed.
pub fn extract_all(repo_root: &str) -> (Vec<StructuralEdge>, usize) {
    let files = collect_ts_files(repo_root);
    let known: HashSet<String> = files.iter().map(|(rel, _)| rel.clone()).collect();
    let aliases = load_path_aliases(repo_root);
    let mut all_edges = Vec::new();

    for (rel_path, abs_path) in &files {
        let Ok(content) = std::fs::read_to_string(abs_path) else { continue };
        let edges = extract_structural_edges_with_aliases(rel_path, &content, &known, repo_root, &aliases);
        all_edges.extend(edges);
    }

    let count = files.len();
    (all_edges, count)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn known(files: &[&str]) -> HashSet<String> {
        files.iter().map(|s| s.to_string()).collect()
    }

    // ── CALLS_INSTANCE ───────────────────────────────────────────────────────

    /// Verifies that lowercase-imported singleton services produce CallsInstance
    /// edges, not CallsStatic edges.  This is the exact pattern used in the
    /// VestaScan enquiry resolver: `import { interestRequestService } from ...`
    /// followed by `interestRequestService.getTeaserCounts(...)`.
    #[test]
    fn instance_service_calls_produce_calls_instance_edges() {
        let content = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/typescript/src/modules/core/graphql/enquiry.resolvers.ts"
        ))
        .expect("fixture file");

        let known = known(&[
            "src/modules/core/services/interest-request.service.ts",
            "src/modules/core/services/listing.service.ts",
        ]);

        let edges = extract_structural_edges(
            "src/modules/core/graphql/enquiry.resolvers.ts",
            &content,
            &known,
            "/repo",
        );

        // The import edge must exist.
        let imports_service = edges.iter().any(|e|
            e.kind == StructuralEdgeKind::Imports
                && e.target_file == "src/modules/core/services/interest-request.service.ts"
        );
        assert!(imports_service, "must import interest-request.service.ts");

        // Two instance-call edges: getTeaserCounts and getRequestsForToken.
        let instance_calls: Vec<_> = edges.iter()
            .filter(|e| e.kind == StructuralEdgeKind::CallsInstance
                && e.target_file == "src/modules/core/services/interest-request.service.ts")
            .collect();

        let syms: Vec<_> = instance_calls.iter()
            .filter_map(|e| e.target_symbol.as_deref())
            .collect();

        assert!(
            syms.iter().any(|s| *s == "interestRequestService.getTeaserCounts"),
            "must have CallsInstance edge for getTeaserCounts: {:?}", syms
        );
        assert!(
            syms.iter().any(|s| *s == "interestRequestService.getRequestsForToken"),
            "must have CallsInstance edge for getRequestsForToken: {:?}", syms
        );
        assert!(
            instance_calls.iter().all(|e| e.evidence.extractor == "typescript-instance-call"),
            "extractor must be typescript-instance-call: {:?}", instance_calls
        );

        // The static call (ListingService) must NOT be classified as CallsInstance.
        let listing_instance: Vec<_> = edges.iter()
            .filter(|e| e.kind == StructuralEdgeKind::CallsInstance
                && e.target_file.contains("listing.service"))
            .collect();
        assert!(
            listing_instance.is_empty(),
            "uppercase ListingService must be CallsStatic, not CallsInstance: {:?}",
            listing_instance
        );

        // And the static call must actually appear as CallsStatic.
        let listing_static = edges.iter().any(|e|
            e.kind == StructuralEdgeKind::CallsStatic
                && e.target_file.contains("listing.service")
        );
        assert!(listing_static, "ListingService.createListing must be CALLS_STATIC");
    }

    // ── IMPORTS ──────────────────────────────────────────────────────────────

    #[test]
    fn named_import_produces_imports_edge() {
        let src = r#"import { Listing, ListingDocument } from "../models/listing.model.js";"#;
        let known = known(&["src/modules/core/models/listing.model.ts"]);
        let edges = extract_structural_edges(
            "src/modules/core/services/share-class.service.ts",
            src,
            &known,
            "/repo",
        );

        let imports: Vec<_> = edges.iter()
            .filter(|e| e.kind == StructuralEdgeKind::Imports)
            .collect();
        assert_eq!(imports.len(), 1, "expected one IMPORTS edge");
        assert_eq!(imports[0].target_file, "src/modules/core/models/listing.model.ts");
        assert_eq!(imports[0].evidence.extractor, "typescript-es-import");
        assert_eq!(imports[0].evidence.line, Some(1));
    }

    #[test]
    fn side_effect_import_produces_imports_edge() {
        // The import path resolves even without named symbols.
        let src = r#"import "../models/order.model.js";"#;
        let known = known(&["src/modules/core/models/order.model.ts"]);
        let edges = extract_structural_edges(
            "src/modules/core/services/listing.service.ts",
            src,
            &known,
            "/repo",
        );

        // Side-effect imports don't have a `from` clause, so they use the
        // side-effect path extractor within extract_imports.
        // The edge should still be produced with kind=Imports.
        let imports: Vec<_> = edges.iter()
            .filter(|e| e.kind == StructuralEdgeKind::Imports)
            .collect();
        assert_eq!(imports.len(), 1, "expected IMPORTS edge for side-effect import");
        assert_eq!(imports[0].target_file, "src/modules/core/models/order.model.ts");
    }

    #[test]
    fn unresolved_external_package_import_is_explicit() {
        let src = r#"import { Schema, model } from "mongoose";"#;
        let edges = extract_structural_edges(
            "src/modules/core/models/listing.model.ts",
            src,
            &known(&[]),
            "/repo",
        );

        let imports: Vec<_> = edges.iter()
            .filter(|e| e.kind == StructuralEdgeKind::Imports)
            .collect();
        assert_eq!(imports.len(), 1);
        assert!(
            imports[0].target_file.starts_with("UNRESOLVED:external:"),
            "external import must be UNRESOLVED:external:… but got: {}",
            imports[0].target_file
        );
    }

    #[test]
    fn unresolved_relative_import_is_explicit() {
        let src = r#"import { OrderService } from "./order.service.js";"#;
        // order.service.ts deliberately not in known_files — structural gap
        let edges = extract_structural_edges(
            "src/modules/core/graphql/listing.resolvers.ts",
            src,
            &known(&[]),
            "/repo",
        );

        let imports: Vec<_> = edges.iter()
            .filter(|e| e.kind == StructuralEdgeKind::Imports)
            .collect();
        assert_eq!(imports.len(), 1);
        assert!(
            imports[0].target_file.starts_with("UNRESOLVED:not-found:"),
            "missing file import must be UNRESOLVED:not-found:… but got: {}",
            imports[0].target_file
        );
    }

    #[test]
    fn commented_import_is_not_extracted() {
        let src = "// import { ShareClassService } from \"./share-class.service.js\";";
        let known = known(&["src/modules/core/services/share-class.service.ts"]);
        let edges = extract_structural_edges(
            "src/modules/core/services/listing.service.ts",
            src,
            &known,
            "/repo",
        );
        assert!(edges.is_empty(), "commented-out import must not produce edges");
    }

    // ── CALLS_STATIC ─────────────────────────────────────────────────────────

    #[test]
    fn static_call_produces_calls_static_edge() {
        let src = "await ShareClassService.ensureDefaultShareClass(\"placeholder\");";
        let mut symbol_map = HashMap::new();
        let target = "src/modules/core/services/share-class.service.ts";
        symbol_map.insert("ShareClassService".to_string(), target.to_string());

        let edges = extract_calls("src/modules/core/services/listing.service.ts", src, &symbol_map);

        let calls: Vec<_> = edges.iter()
            .filter(|e| e.kind == StructuralEdgeKind::CallsStatic)
            .collect();
        assert_eq!(calls.len(), 1, "expected one CALLS_STATIC edge");
        assert_eq!(calls[0].target_file, target);
        assert_eq!(
            calls[0].target_symbol.as_deref(),
            Some("ShareClassService.ensureDefaultShareClass"),
        );
    }

    #[test]
    fn lowercase_call_is_not_a_static_edge() {
        // Only uppercase identifiers are treated as imported service/model names.
        let src = "const x = helper.doThing();";
        let edges = extract_calls(
            "src/some.ts",
            src,
            &HashMap::new(),
        );
        assert!(edges.is_empty(), "lowercase object call must not produce edges");
    }

    #[test]
    fn string_literal_containing_static_call_pattern_is_not_extracted() {
        let src = "const msg = \"ShareClassService.fakeCall()\";";
        let mut symbol_map = HashMap::new();
        symbol_map.insert("ShareClassService".to_string(), "svc.ts".to_string());
        // The string literal should prevent the dot scanner from matching.
        let edges = extract_calls("src/some.ts", src, &symbol_map);
        // String literals are skipped by find_next_dot.
        assert!(
            edges.is_empty(),
            "static call pattern inside a string literal must not produce edges"
        );
    }

    // ── REFERENCES_MODEL ─────────────────────────────────────────────────────

    #[test]
    fn mongoose_findone_produces_references_model_edge() {
        let src = "const listing = await Listing.findOne({ name });";
        let mut symbol_map = HashMap::new();
        let model_file = "src/modules/core/models/listing.model.ts";
        symbol_map.insert("Listing".to_string(), model_file.to_string());

        let edges = extract_calls("src/modules/core/services/listing.service.ts", src, &symbol_map);

        let refs: Vec<_> = edges.iter()
            .filter(|e| e.kind == StructuralEdgeKind::ReferencesModel)
            .collect();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].target_file, model_file);
        assert_eq!(refs[0].target_symbol.as_deref(), Some("Listing.findOne"));
    }

    #[test]
    fn mongoose_create_produces_references_model_edge() {
        let src = "return Listing.create({ name, code: name.toLowerCase() });";
        let mut symbol_map = HashMap::new();
        let model_file = "src/modules/core/models/listing.model.ts";
        symbol_map.insert("Listing".to_string(), model_file.to_string());

        let edges = extract_calls("src/modules/core/services/listing.service.ts", src, &symbol_map);
        let refs: Vec<_> = edges.iter()
            .filter(|e| e.kind == StructuralEdgeKind::ReferencesModel)
            .collect();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].target_symbol.as_deref(), Some("Listing.create"));
    }

    // ── Full-file integration ─────────────────────────────────────────────────

    #[test]
    fn full_listing_service_produces_expected_edges() {
        let content = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/typescript/src/modules/core/services/listing.service.ts"
        ))
        .expect("fixture file");

        let known = known(&[
            "src/modules/core/models/listing.model.ts",
            "src/modules/core/models/order.model.ts",
            "src/modules/core/services/share-class.service.ts",
        ]);

        let edges = extract_structural_edges(
            "src/modules/core/services/listing.service.ts",
            &content,
            &known,
            "/repo",
        );

        let import_targets: Vec<_> = edges.iter()
            .filter(|e| e.kind == StructuralEdgeKind::Imports)
            .map(|e| e.target_file.as_str())
            .collect();

        assert!(
            import_targets.contains(&"src/modules/core/models/listing.model.ts"),
            "must import listing.model: {:?}", import_targets
        );
        assert!(
            import_targets.contains(&"src/modules/core/models/order.model.ts"),
            "must import order.model (side-effect): {:?}", import_targets
        );
        assert!(
            import_targets.contains(&"src/modules/core/services/share-class.service.ts"),
            "must import share-class.service: {:?}", import_targets
        );

        let calls: Vec<_> = edges.iter()
            .filter(|e| e.kind == StructuralEdgeKind::CallsStatic)
            .collect();
        let call_symbols: Vec<_> = calls.iter()
            .filter_map(|e| e.target_symbol.as_deref())
            .collect();
        assert!(
            call_symbols.iter().any(|s| s.starts_with("ShareClassService.")),
            "must have CALLS_STATIC to ShareClassService: {:?}", call_symbols
        );

        let model_refs: Vec<_> = edges.iter()
            .filter(|e| e.kind == StructuralEdgeKind::ReferencesModel)
            .collect();
        let ref_symbols: Vec<_> = model_refs.iter()
            .filter_map(|e| e.target_symbol.as_deref())
            .collect();
        assert!(
            ref_symbols.iter().any(|s| s.starts_with("Listing.")),
            "must have REFERENCES_MODEL to Listing: {:?}", ref_symbols
        );

        // Negative: string-literal calls must not appear.
        let bad: Vec<_> = edges.iter()
            .filter(|e| {
                e.target_symbol.as_deref()
                    .map(|s| s.contains("fakeCall") || s.contains("commentedCall"))
                    .unwrap_or(false)
            })
            .collect();
        assert!(bad.is_empty(), "string-literal / comment calls must not produce edges: {:?}", bad);
    }

    #[test]
    fn resolver_percentageraised_has_no_order_service_edge() {
        let content = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/typescript/src/modules/core/graphql/listing.resolvers.ts"
        ))
        .expect("fixture file");

        let known = known(&[
            "src/modules/core/services/listing.service.ts",
            "src/modules/core/models/listing.model.ts",
        ]);
        // order.service.ts is intentionally absent from known_files.

        let edges = extract_structural_edges(
            "src/modules/core/graphql/listing.resolvers.ts",
            &content,
            &known,
            "/repo",
        );

        // No edge should point to any order service.
        let order_edges: Vec<_> = edges.iter()
            .filter(|e| e.target_file.contains("order.service"))
            .collect();
        assert!(
            order_edges.is_empty(),
            "no OrderService edge should exist — it does not exist in the codebase: {:?}",
            order_edges
        );

        // But listing.service.ts should be imported.
        let imports_listing_service = edges.iter().any(|e|
            e.kind == StructuralEdgeKind::Imports
                && e.target_file == "src/modules/core/services/listing.service.ts"
        );
        assert!(imports_listing_service, "resolver must import listing.service.ts");
    }
}
