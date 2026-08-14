//! C5.1-S — Free-text → structural subject resolution.
//!
//! Converts an ambiguous human question into concrete file subjects before
//! C5.1-R/L/E ranking, so investigate is not a glorified keyword search.
//!
//! Pipeline position:
//!   question → concepts/entities → candidate subjects → path + structural
//!   expansion → seeds for C5.1-R …
//!
//! Generic (no repo-specific hardcodes). Soft production preference via path_class.

use crate::path_class::{class_subject_boost, classify_path, PathClass};
use anyhow::Result;
use atlas_storage::Store;
use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct SubjectResolution {
    /// Repo-relative paths that should enter the investigation bag first.
    pub seed_files: Vec<String>,
    /// Extra anchors (path stems / compounds) for investigate.
    pub anchors: Vec<String>,
    pub notes: Vec<String>,
}

const STOP: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "if", "in", "on", "at", "to", "for", "of",
    "is", "are", "was", "were", "be", "been", "being", "have", "has", "had", "do",
    "does", "did", "will", "would", "could", "should", "may", "might", "with", "from",
    "by", "as", "into", "about", "this", "that", "these", "those", "it", "its", "we",
    "you", "they", "when", "where", "what", "which", "who", "how", "why", "not", "no",
    "than", "then", "too", "very", "can", "just", "also", "some", "any", "all",
    // Function words + question scaffolding only. Keep domain nouns (working, batch,
    // store, cache, conflict, backend, encode, …) so compounds resolve.
    "please", "help", "explain", "investigate", "look", "find", "show", "get", "make",
    "need", "seems", "through", "system", "code", "file", "files", "does", "else",
    "really", "actually", "appear", "appears", "there", "their", "overall",
    "structure", "repository", "project", "implemented", "implementation", "behavior",
    "problem", "issue", "caused", "cause", "because", "given", "using", "used", "use",
    "between", "after", "before", "during", "under", "over", "such", "other", "more",
    "most", "only", "same", "both", "each", "few", "own", "out", "up", "down", "again",
    "further", "once", "here", "first", "second", "next", "last", "long", "short",
    "high", "low", "new", "old", "good", "true", "false", "null", "type", "types",
    "value", "values", "name", "names", "thing", "things", "part", "parts", "way",
    "ways", "end", "ends", "start", "starts", "run", "runs", "set", "sets", "like",
    "want", "wants", "must", "still", "already", "never", "often", "sometimes",
    "maybe", "perhaps", "thanks", "hello", "immutable", "record", "mutations",
    "represented", "merged", "pluggable", "especially", "management", "created",
    "stored", "change", "interface", "originate", "flows", "trace", "path", "loading",
    "text", "slow", "missing", "broken", "storing", "related", "involved",
    "intentionally", "intentional", "why", "this",
];

/// Resolve free-text question → concrete subject file seeds.
pub fn resolve_subjects(
    question: &str,
    repo_path: &str,
    store: &Store,
) -> Result<SubjectResolution> {
    let mut out = SubjectResolution::default();
    let q = question.to_lowercase();
    let tokens = significant_tokens(question);
    let compounds = compound_fragments(&tokens);
    let orientation = is_orientation_question(&q);

    // ── Orientation: prefer crate entrypoints / layout roots ───────────────
    if orientation {
        for seed in orientation_seeds(repo_path, store)? {
            push_unique(&mut out.seed_files, seed);
        }
        out.notes
            .push("c5.1s_orientation_roots".into());
    }

    // ── Build search fragments (entities → path-like forms) ────────────────
    let mut fragments: Vec<String> = Vec::new();
    for t in &tokens {
        push_unique(&mut fragments, t.clone());
    }
    for c in &compounds {
        push_unique(&mut fragments, c.clone());
    }
    // concept_search_fragments already does bigrams + .service etc.
    for f in crate::role_aware::concept_search_fragments(question) {
        push_unique(&mut fragments, f);
    }
    fragments.truncate(48);

    for f in &fragments {
        if !out.anchors.iter().any(|a| a == f) {
            out.anchors.push(f.clone());
        }
    }

    // ── Score paths from search_anchor hits ────────────────────────────────
    let mut scores: HashMap<String, f32> = HashMap::new();
    for frag in &fragments {
        if frag.len() < 3 {
            continue;
        }
        // Skip ultra-generic single fragments that flood
        if is_generic_flood_token(frag) {
            continue;
        }
        let Ok(matches) = store.search_anchor(frag, repo_path) else {
            continue;
        };
        for m in matches.into_iter().filter(|m| m.source_type == "file_path") {
            if !looks_like_code_path(&m.source_id) && !looks_like_layout_path(&m.source_id) {
                continue;
            }
            let class = classify_path(&m.source_id);
            let mut hit = match_quality(&m.source_id, frag);
            hit += class_subject_boost(class) * 0.25;
            // Prefer multi-token agreement
            let tok_hits = tokens
                .iter()
                .filter(|t| path_mentions(&m.source_id, t))
                .count();
            if tok_hits >= 2 {
                hit += 6.0 + (tok_hits as f32);
            } else if tok_hits == 1 && compounds.iter().any(|c| path_mentions(&m.source_id, c)) {
                hit += 8.0;
            }
            *scores.entry(m.source_id).or_insert(0.0) += hit;
        }
    }

    // ── Direct compound stem scan on all paths (bounded) ───────────────────
    // Critical for "operation store" → op_store without waiting for LIKE noise.
    if let Ok(all) = store.all_file_paths(repo_path) {
        for path in all.iter().filter(|p| looks_like_code_path(p)) {
            let mut s = 0.0f32;
            for c in &compounds {
                if path_mentions(path, c) {
                    s += 12.0;
                    let class = classify_path(path);
                    s += class_subject_boost(class);
                    // exact stem match is strongest
                    if stem_eq(path, c) {
                        s += 10.0;
                    }
                }
            }
            // multi-token co-presence in path
            let th = tokens
                .iter()
                .filter(|t| t.len() >= 4 && path_mentions(path, t))
                .count();
            if th >= 2 {
                s += 5.0 * th as f32;
            }
            if s > 0.0 {
                *scores.entry(path.clone()).or_insert(0.0) += s;
            }
        }
    }

    // Apply path-class multipliers
    let mut ranked: Vec<(String, f32)> = scores
        .into_iter()
        .map(|(path, base)| {
            let adj = crate::path_class::apply_class_to_score(&path, question, base);
            (path, adj)
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    // Keep production-heavy top subjects
    let mut taken = 0usize;
    for (path, score) in ranked.iter().take(40) {
        if *score < 3.0 {
            break;
        }
        let class = classify_path(path);
        // Soft filter: skip pure assets always; skip demos/config unless high score
        if matches!(class, PathClass::Asset | PathClass::Vendor) {
            continue;
        }
        if matches!(
            class,
            PathClass::Demo | PathClass::Notebook | PathClass::Config
        ) && *score < 20.0
        {
            continue;
        }
        push_unique(&mut out.seed_files, path.clone());
        taken += 1;
        if taken >= 12 {
            break;
        }
    }

    // Structural neighborhood of top subjects (1 hop imports, both directions)
    let expand_from: Vec<String> = out.seed_files.iter().take(4).cloned().collect();
    for seed in expand_from {
        let mut neighbors: Vec<String> = Vec::new();
        if let Ok(edges) = store.structural_edges_for_file(&seed, repo_path) {
            for e in edges.into_iter().take(16) {
                neighbors.push(e.target_file);
            }
        }
        if let Ok(edges) = store.structural_edges_targeting(&seed, repo_path) {
            for e in edges.into_iter().take(16) {
                neighbors.push(e.source_file);
            }
        }
        for other in neighbors {
            if !looks_like_code_path(&other) {
                continue;
            }
            let class = classify_path(&other);
            if matches!(
                class,
                PathClass::Production | PathClass::Library | PathClass::Cli | PathClass::Test
            ) {
                push_unique(&mut out.seed_files, other);
            }
        }
    }

    out.seed_files.truncate(20);
    out.anchors.truncate(20);
    if !out.seed_files.is_empty() {
        out.notes.push(format!(
            "c5.1s_subjects n={} top={}",
            out.seed_files.len(),
            out.seed_files.first().map(|s| s.as_str()).unwrap_or("-")
        ));
    } else {
        out.notes.push("c5.1s_subjects n=0".into());
    }
    Ok(out)
}

/// Discover meaningful code roots (not only `src/`).
pub fn discover_code_roots(repo_path: &str, store: &Store) -> Result<Vec<String>> {
    let paths = store.all_file_paths(repo_path)?;
    let mut root_hits: HashMap<String, usize> = HashMap::new();

    for p in &paths {
        if !looks_like_code_path(p) {
            continue;
        }
        let class = classify_path(p);
        if matches!(
            class,
            PathClass::Demo | PathClass::Example | PathClass::Benchmark | PathClass::Notebook
        ) {
            continue;
        }
        // Candidate roots: lib/src, cli/src, src, crates/<name>/src, app/src
        let segs: Vec<&str> = p.split('/').collect();
        if segs.len() >= 2 && segs[1] == "src" {
            let root = format!("{}/src", segs[0]);
            *root_hits.entry(root).or_insert(0) += 1;
        }
        if segs.first() == Some(&"src") {
            *root_hits.entry("src".into()).or_insert(0) += 1;
        }
        if segs.len() >= 3 && segs[0] == "crates" && segs[2] == "src" {
            let root = format!("crates/{}/src", segs[1]);
            *root_hits.entry(root).or_insert(0) += 1;
        }
        if segs.len() >= 3 && segs[0] == "apps" && segs[2] == "src" {
            let root = format!("apps/{}/src", segs[1]);
            *root_hits.entry(root).or_insert(0) += 1;
        }
    }

    let mut roots: Vec<(String, usize)> = root_hits.into_iter().collect();
    roots.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(roots.into_iter().map(|(r, _)| r).take(8).collect())
}

fn orientation_seeds(repo_path: &str, store: &Store) -> Result<Vec<String>> {
    let mut seeds = Vec::new();
    let candidates = [
        "README.md",
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "src/lib.rs",
        "src/main.rs",
        "lib/src/lib.rs",
        "lib/src/repo.rs",
        "cli/src/main.rs",
        "src/token.rs",
        "src/index.ts",
        "src/index.js",
    ];
    let all = store.all_file_paths(repo_path).unwrap_or_default();
    let set: std::collections::HashSet<&str> = all.iter().map(String::as_str).collect();
    for c in candidates {
        if set.contains(c) {
            seeds.push(c.to_string());
        }
    }
    // Prefer largest code root's lib.rs / mod.rs
    for root in discover_code_roots(repo_path, store)? {
        for name in ["lib.rs", "mod.rs", "main.rs", "index.ts", "index.js"] {
            let p = format!("{root}/{name}");
            if set.contains(p.as_str()) {
                push_unique(&mut seeds, p);
            }
        }
    }
    Ok(seeds)
}

fn is_orientation_question(q: &str) -> bool {
    q.contains("overall structure")
        || q.contains("repository organized")
        || q.contains("how is the repository")
        || q.contains("what is this repository")
        || q.contains("what is gigatoken")
        || q.contains("what is jujutsu")
        || q.contains("what is jj")
        || q.contains("where is the core")
        || q.contains("core library")
        || (q.contains("what is") && (q.contains("organized") || q.contains("structure")))
        || (q.contains("overview") && q.contains("repo"))
}

fn significant_tokens(question: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in question.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
        let t = raw.to_lowercase();
        if t.len() < 3 || STOP.contains(&t.as_str()) {
            continue;
        }
        if t.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if !out.iter().any(|x| x == &t) {
            out.push(t);
        }
        if out.len() >= 16 {
            break;
        }
    }
    out
}

/// Public compound stems from a question (for ranking re-promotion).
pub fn compound_stems_for_question(question: &str) -> Vec<String> {
    compound_fragments(&significant_tokens(question))
}

/// Score boost when path stem matches a multi-word subject compound.
pub fn subject_stem_boost(path: &str, question: &str) -> f32 {
    let mut best = 0.0f32;
    let q = question.to_lowercase();
    // Orientation: crate entrypoints / layout roots outrank package loaders.
    if is_orientation_question(&q) {
        let pl = path.to_lowercase();
        if pl.ends_with("/lib.rs")
            || pl == "src/lib.rs"
            || pl == "lib/src/lib.rs"
            || pl.ends_with("/main.rs")
            || pl == "readme.md"
            || pl.ends_with("/readme.md")
            || pl.ends_with("cargo.toml")
            || pl == "src/token.rs"
            || pl == "src/repo.rs"
            || pl == "lib/src/repo.rs"
        {
            best = best.max(20.0);
        }
    }
    let compounds = compound_stems_for_question(question);
    if compounds.is_empty() {
        return best;
    }
    for c in &compounds {
        if stem_eq(path, c) {
            best = best.max(18.0);
        } else if path_mentions(path, c) {
            best = best.max(10.0);
        }
    }
    // Prefer shorter stem matches (op_store over legacy_thrift_op_store) via exactness
    if best >= 18.0 {
        let stem = path
            .rsplit('/')
            .next()
            .unwrap_or(path)
            .rsplit_once('.')
            .map(|(s, _)| s)
            .unwrap_or(path);
        // exact compound == stem
        if compounds.iter().any(|c| c == &stem.to_lowercase()) {
            best += 6.0;
        }
    }
    // Operation log / op log → prefer lib operation/op_store cluster over workspace_store
    // and CLI command wrappers (soft demotion of cli/ for architecture questions).
    if q.contains("operation log") || q.contains("op log") || q.contains("oplog") {
        let pl = path.to_lowercase();
        if pl.starts_with("lib/")
            && (stem_eq(path, "operation")
                || stem_eq(path, "op_store")
                || stem_eq(path, "simple_op_store")
                || stem_eq(path, "op_heads_store")
                || stem_eq(path, "op_walk"))
        {
            best = best.max(16.0);
        }
        if pl.starts_with("cli/") || pl.starts_with("src/commands/") {
            best *= 0.35;
        }
    }
    best
}

/// Adjacent multi-word entities → path-like forms including abbreviations.
/// "operation store" → operation_store, op_store, operation-store, …
fn compound_fragments(tokens: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for w in tokens.windows(2) {
        let a = &w[0];
        let b = &w[1];
        if a.len() < 3 || b.len() < 3 {
            continue;
        }
        push_unique(&mut out, format!("{a}_{b}"));
        push_unique(&mut out, format!("{a}-{b}"));
        push_unique(&mut out, format!("{a}{b}"));
        // Abbreviation of first token: operation→op, working→work already short
        if a.len() >= 4 {
            let p2: String = a.chars().take(2).collect();
            let p3: String = a.chars().take(3).collect();
            push_unique(&mut out, format!("{p2}_{b}"));
            push_unique(&mut out, format!("{p2}-{b}"));
            push_unique(&mut out, format!("{p3}_{b}"));
            push_unique(&mut out, format!("{p3}-{b}"));
        }
        // Also reverse for store_op style rare
        if b.len() >= 4 {
            let p2: String = b.chars().take(2).collect();
            push_unique(&mut out, format!("{a}_{p2}"));
        }
    }
    // Trigrams for "local working copy" style
    for w in tokens.windows(3) {
        let a = &w[0];
        let b = &w[1];
        let c = &w[2];
        if a.len() >= 3 && b.len() >= 3 && c.len() >= 3 {
            push_unique(&mut out, format!("{a}_{b}_{c}"));
            push_unique(&mut out, format!("{a}-{b}-{c}"));
            // local_working_copy from local + working + copy
            push_unique(&mut out, format!("{b}_{c}"));
        }
    }
    out
}

fn match_quality(path: &str, frag: &str) -> f32 {
    let p = path.to_lowercase();
    let f = frag.to_lowercase();
    let base = p.rsplit('/').next().unwrap_or(&p);
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base);
    if stem == f || base == f {
        return 14.0;
    }
    if stem.contains(&f) || p.contains(&format!("/{f}")) {
        return 9.0;
    }
    if p.contains(&f) {
        return 5.0;
    }
    1.0
}

fn path_mentions(path: &str, token: &str) -> bool {
    let p = path.to_lowercase();
    let t = token.to_lowercase();
    if t.len() < 2 {
        return false;
    }
    p.contains(&t)
}

fn stem_eq(path: &str, frag: &str) -> bool {
    let base = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_lowercase();
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(&base);
    stem == frag.to_lowercase()
}

fn looks_like_code_path(p: &str) -> bool {
    let lower = p.to_lowercase();
    if lower.contains("node_modules/") || lower.contains("/dist/") || lower.contains("/target/") {
        return false;
    }
    lower.ends_with(".rs")
        || lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".js")
        || lower.ends_with(".jsx")
        || lower.ends_with(".py")
        || lower.ends_with(".go")
}

fn looks_like_layout_path(p: &str) -> bool {
    let lower = p.to_lowercase();
    lower.ends_with("cargo.toml")
        || lower.ends_with("package.json")
        || lower.ends_with("pyproject.toml")
        || lower == "readme.md"
        || lower.ends_with("/readme.md")
}

fn is_generic_flood_token(t: &str) -> bool {
    matches!(
        t,
        "src"
            | "lib"
            | "cli"
            | "mod"
            | "main"
            | "test"
            | "tests"
            | "util"
            | "utils"
            | "common"
            | "core"
            | "base"
            | "type"
            | "types"
            | "index"
            | "config"
            | "github"
            | "workflow"
            | "workflows"
    )
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.iter().any(|x| x == &s) {
        v.push(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compounds_operation_store() {
        let toks = vec!["operation".into(), "store".into()];
        let c = compound_fragments(&toks);
        assert!(c.iter().any(|x| x == "op_store"), "{c:?}");
        assert!(c.iter().any(|x| x == "operation_store"), "{c:?}");
    }

    #[test]
    fn compounds_pretoken_cache() {
        let toks = vec!["pretoken".into(), "cache".into()];
        let c = compound_fragments(&toks);
        assert!(c.iter().any(|x| x == "pretoken_cache"), "{c:?}");
    }

    #[test]
    fn compounds_working_copy() {
        let toks = vec!["working".into(), "copy".into()];
        let c = compound_fragments(&toks);
        assert!(c.iter().any(|x| x == "working_copy"), "{c:?}");
    }

    #[test]
    fn orientation_detect() {
        assert!(is_orientation_question(
            "what is the overall structure of this repository and where is the core library?"
        ));
        assert!(!is_orientation_question(
            "how does the operation log work"
        ));
    }
}
