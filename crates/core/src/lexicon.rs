/// Repository Lexicon Builder
///
/// Builds vocabulary relationships during ingest so investigations can expand
/// user anchors into repository-specific terminology deterministically.
///
/// Three passes:
///   Pass 1 — Structural analysis: token decomposition from file paths produces
///             Abbreviation, CaseVariant, CompoundComponent, and ModuleSibling
///             relationships. Pure static analysis, no external data needed.
///
///   Pass 2 — Commit correlation: commit subject lines × file path tokens produce
///             CommitBridge relationships.  When a subject word co-occurs with a
///             file-path token in ≥ MIN_CO_OCCURRENCE commits, a bridge is recorded.
///             This is the highest-ROI pass because it captures what the team
///             actually called things when they changed them.
///
///   Pass 3 — Morphological derivation: for each path token ending in a regular
///             English derivation suffix (-ize/-ise), generate the user-query forms
///             that derive from it (-ization/-isation, -izer/-iser) and store them
///             as MorphologicalVariant relationships pointing back to the path token.
///             Fixes the "tokenization" → "tokenize" gap.
///
/// The result is a set of LexiconRelationship records persisted per repo_path.
/// Investigation Phase 0a queries these before the existing concept resolution pass.

use anyhow::Result;
use atlas_ir::{LexiconRelKind, LexiconRelationship};
use atlas_storage::Store;
use std::collections::{HashMap, HashSet};

/// Minimum co-occurrence in commit messages before a CommitBridge is recorded.
const MIN_CO_OCCURRENCE: u32 = 5;

/// Maximum character length for a token to be kept (filters URL fragments, hashes).
const MAX_TOKEN_LEN: usize = 40;

/// Minimum character length for a non-abbreviation token.
const MIN_TOKEN_LEN: usize = 2;

/// Words that appear in almost every repository and carry no vocabulary signal.
const GENERIC_TERMS: &[&str] = &[
    "src", "lib", "mod", "rs", "ts", "js", "py", "go", "rb",
    "test", "tests", "spec", "specs",
    "the", "and", "for", "with", "not", "from", "into",
    "add", "fix", "update", "remove", "use", "get", "set",
    "new", "old", "tmp", "temp",
    "main", "index", "util", "utils", "helper", "helpers",
    "types", "type", "config", "configs", "common", "shared",
    "base", "core", "init", "setup",
    // English prepositions, articles, and particles that appear as path fragments
    "in", "on", "of", "to", "at", "by", "up", "if", "as", "is", "it",
    // Structural/meta words that are not domain vocabulary
    "not", "no", "do", "id", "ok", "ui", "io",
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k",
];

fn is_generic(token: &str) -> bool {
    GENERIC_TERMS.contains(&token)
}

/// Decompose a file path into lowercase tokens from ALL path components (directories
/// and filename), splitting on `_`, `-`, and camelCase boundaries.
///
/// Including directory tokens is critical for vocabulary coverage: a file at
/// `src/pretokenize/fast/cl100k.rs` should produce the token "pretokenize" so that
/// CommitBridge can link "tokenization" (in commit subjects) to that module name.
pub fn decompose_path(path: &str) -> Vec<String> {
    let components: Vec<&str> = path.split('/').collect();
    let n = components.len();
    let mut seen: HashSet<String> = HashSet::new();
    let mut result: Vec<String> = Vec::new();

    for (i, &component) in components.iter().enumerate() {
        // Strip extension from the final component (filename); leave directory names as-is.
        let text = if i == n - 1 {
            if let Some(dot) = component.rfind('.') { &component[..dot] } else { component }
        } else {
            component
        };

        for tok in split_component(text) {
            if tok.len() >= MIN_TOKEN_LEN
                && tok.len() <= MAX_TOKEN_LEN
                && tok.chars().all(|c| c.is_alphanumeric())
                && seen.insert(tok.clone())
            {
                result.push(tok);
            }
        }
    }

    result
}

/// Split a single path component on `_`, `-`, and camelCase boundaries.
fn split_component(component: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for part in component.split(|c: char| c == '_' || c == '-') {
        let mut current = String::new();
        let chars: Vec<char> = part.chars().collect();
        for (i, &ch) in chars.iter().enumerate() {
            let next_upper = chars.get(i + 1).map(|c| c.is_uppercase()).unwrap_or(false);
            let prev_lower  = i > 0 && chars[i - 1].is_lowercase();
            if ch.is_uppercase() && prev_lower {
                if !current.is_empty() {
                    tokens.push(current.to_lowercase());
                    current = String::new();
                }
            }
            if ch.is_uppercase() && !next_upper && !current.is_empty()
                && current.chars().all(|c| c.is_uppercase())
            {
                // keep accumulating (acronym run ending)
            }
            current.push(ch);
            if ch.is_uppercase() && next_upper && i + 2 < chars.len() && chars[i + 2].is_lowercase() {
                tokens.push(current.to_lowercase());
                current = String::new();
            }
        }
        if !current.is_empty() {
            tokens.push(current.to_lowercase());
        }
    }
    tokens
}

/// Decompose a commit subject line into significant words.
/// Strips conventional commit prefixes ("feat:", "fix:", etc.) and common noise.
pub fn decompose_subject(subject: &str) -> Vec<String> {
    // Strip conventional commit prefix: "feat(scope): message" → "message"
    let stripped = if let Some(colon) = subject.find(':') {
        let prefix = &subject[..colon];
        if prefix.len() <= 20 && !prefix.contains(' ') {
            subject[colon + 1..].trim()
        } else {
            subject.trim()
        }
    } else {
        subject.trim()
    };

    stripped
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .filter(|w| {
            w.len() >= MIN_TOKEN_LEN
                && w.len() <= MAX_TOKEN_LEN
                && !is_generic(w)
        })
        .collect()
}

/// Detect abbreviation relationships: "op" ↔ "operation", "tx" ↔ "transaction".
/// Rules:
///   1. Short is a prefix of long AND short.len() <= 4
///   2. Short is all consonants from long (simple vowel-drop)
///   3. Short is the first letter of each word in a multi-word long form
fn detect_abbreviation(short: &str, long: &str) -> bool {
    if short.len() >= long.len() { return false; }
    if short.len() > 5 { return false; }

    // Rule 1: prefix
    if long.starts_with(short) && short.len() <= 4 { return true; }

    // Rule 2: vowel drop — short consists of consonants from long in order
    let vowels = ['a', 'e', 'i', 'o', 'u'];
    let consonants_of_long: String = long.chars()
        .filter(|c| c.is_alphabetic() && !vowels.contains(c))
        .collect();
    if consonants_of_long == short { return true; }

    // Rule 3: initialism — "op" from "operation" won't match but
    // "repo" from "repository" matches rule 1.
    // "tx" from "transaction": t-r-a-n-s-a-c-t-i-o-n → consonants = trnsct → no
    // Keep it simple; rule 1+2 cover most cases.
    false
}

/// Pass 1: build structural relationships from file paths alone.
/// Produces: Abbreviation, CaseVariant, CompoundComponent, ModuleSibling.
fn pass1_structural(
    file_paths: &[String],
    repo_path: &str,
    store: &Store,
) -> Result<()> {
    // Collect all unique path tokens globally
    let mut all_tokens: HashSet<String> = HashSet::new();
    // Map from lowercase token to example file paths (for grounding)
    let mut token_to_files: HashMap<String, Vec<String>> = HashMap::new();

    for path in file_paths {
        let toks = decompose_path(path);
        for tok in toks {
            if !is_generic(&tok) {
                token_to_files.entry(tok.clone()).or_default().push(path.clone());
                all_tokens.insert(tok);
            }
        }
    }

    let tokens: Vec<String> = all_tokens.into_iter().collect();
    let total_files = file_paths.len().max(1);

    // A token that appears in more than 40% of files is a project-wide prefix
    // (e.g. "ds4" in a repo where every file is ds4_*.c), not meaningful vocabulary.
    // Using it as a CompoundComponent anchor makes every query expand into the whole repo.
    let is_ubiquitous = |tok: &str| -> bool {
        token_to_files.get(tok).map(|v| v.len()).unwrap_or(0) * 100 / total_files > 40
    };

    // CaseVariant: same token under different casing already normalised — nothing to do.
    // We do record compound-component relationships: "op_store" → "op", "store"
    for path in file_paths {
        let parts: Vec<&str> = path.split('/').last()
            .unwrap_or(path)
            .split(|c: char| c == '_' || c == '-' || c == '.')
            .filter(|p| !p.is_empty())
            .collect();

        // Only process short compound names (≤ 4 parts after splitting on _ . -).
        // Long hyphen-separated config file names (e.g. ui.conflict-marker-style_not_in_enum.toml)
        // produce semantically incoherent pairs — the parts are not vocabulary synonyms.
        if parts.len() >= 2 && parts.len() <= 5 {
            // The full stem tokens
            let stem_parts: Vec<String> = parts[..parts.len() - 1] // drop extension
                .iter()
                .map(|p| p.to_lowercase())
                .filter(|p| !is_generic(p) && p.len() >= MIN_TOKEN_LEN && !is_ubiquitous(p))
                .collect();

            if stem_parts.len() >= 2 {
                // Each part is a component of the compound name
                for part in &stem_parts {
                    for other in &stem_parts {
                        if part == other { continue; }
                        let rel = LexiconRelationship {
                            from_term:           part.clone(),
                            to_term:             other.clone(),
                            kind:                LexiconRelKind::CompoundComponent,
                            confidence:          0.70,
                            co_occurrence_count: 1,
                        };
                        store.upsert_lexicon_relationship(&rel, repo_path)?;
                    }
                }
            }
        }
    }

    // Abbreviation: compare all pairs of tokens
    for (i, tok_a) in tokens.iter().enumerate() {
        for tok_b in tokens[i + 1..].iter() {
            let (short, long) = if tok_a.len() < tok_b.len() {
                (tok_a.as_str(), tok_b.as_str())
            } else {
                (tok_b.as_str(), tok_a.as_str())
            };
            if detect_abbreviation(short, long) {
                // short ↔ long (bidirectional)
                let rel_fwd = LexiconRelationship {
                    from_term:           short.to_string(),
                    to_term:             long.to_string(),
                    kind:                LexiconRelKind::Abbreviation,
                    confidence:          0.88,
                    co_occurrence_count: 1,
                };
                let rel_bwd = LexiconRelationship {
                    from_term:           long.to_string(),
                    to_term:             short.to_string(),
                    kind:                LexiconRelKind::Abbreviation,
                    confidence:          0.88,
                    co_occurrence_count: 1,
                };
                store.upsert_lexicon_relationship(&rel_fwd, repo_path)?;
                store.upsert_lexicon_relationship(&rel_bwd, repo_path)?;
            }
        }
    }

    // ModuleSibling: files in the same directory that share tokens get a sibling link
    let mut dir_tokens: HashMap<String, Vec<String>> = HashMap::new();
    for path in file_paths {
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
        let toks = decompose_path(path);
        for tok in toks {
            if !is_generic(&tok) && tok.len() >= MIN_TOKEN_LEN {
                dir_tokens.entry(dir.to_string()).or_default().push(tok);
            }
        }
    }

    for (_dir, toks) in &dir_tokens {
        let unique: Vec<&String> = {
            let mut seen = HashSet::new();
            toks.iter().filter(|t| seen.insert(*t)).collect()
        };
        for (i, tok_a) in unique.iter().enumerate() {
            for tok_b in unique[i + 1..].iter() {
                if tok_a == tok_b { continue; }
                let rel_fwd = LexiconRelationship {
                    from_term:           tok_a.to_string(),
                    to_term:             tok_b.to_string(),
                    kind:                LexiconRelKind::ModuleSibling,
                    confidence:          0.60,
                    co_occurrence_count: 1,
                };
                let rel_bwd = LexiconRelationship {
                    from_term:           tok_b.to_string(),
                    to_term:             tok_a.to_string(),
                    kind:                LexiconRelKind::ModuleSibling,
                    confidence:          0.60,
                    co_occurrence_count: 1,
                };
                store.upsert_lexicon_relationship(&rel_fwd, repo_path)?;
                store.upsert_lexicon_relationship(&rel_bwd, repo_path)?;
            }
        }
    }

    Ok(())
}

/// Pass 2: commit correlation — the highest ROI pass.
///
/// For every commit: take the subject line tokens and the path tokens of touched
/// files.  If a subject token and a path token co-occur ≥ MIN_CO_OCCURRENCE times,
/// record a CommitBridge relationship.
///
/// This captures "conflict" → "merge" when developers wrote "fix conflict handling"
/// in commits that touched merge.rs, without any semantic understanding.
fn pass2_commit_bridge(repo_path: &str, store: &Store) -> Result<()> {
    let commits = store.all_commits_with_files(repo_path)?;

    // (subject_token, path_token) → co-occurrence count
    let mut matrix: HashMap<(String, String), u32> = HashMap::new();

    for commit in &commits {
        let subject = commit.message.lines().next().unwrap_or("").to_string();
        let subject_tokens: Vec<String> = decompose_subject(&subject);
        if subject_tokens.is_empty() { continue; }

        let mut path_tokens: HashSet<String> = HashSet::new();
        for file in &commit.files {
            for tok in decompose_path(file) {
                if !is_generic(&tok) && tok.len() >= MIN_TOKEN_LEN {
                    path_tokens.insert(tok);
                }
            }
        }
        if path_tokens.is_empty() { continue; }

        for stok in &subject_tokens {
            for ptok in &path_tokens {
                if stok == ptok { continue; }
                *matrix.entry((stok.clone(), ptok.clone())).or_insert(0) += 1;
            }
        }
    }

    for ((stok, ptok), count) in matrix {
        if count < MIN_CO_OCCURRENCE { continue; }

        let confidence = compute_bridge_confidence(count);

        // subject_token → path_token: user says "conflict", repo has "merge"
        let rel_fwd = LexiconRelationship {
            from_term:           stok.clone(),
            to_term:             ptok.clone(),
            kind:                LexiconRelKind::CommitBridge,
            confidence,
            co_occurrence_count: count,
        };
        // path_token → subject_token: so "merge" also expands to "conflict"
        let rel_bwd = LexiconRelationship {
            from_term:           ptok.clone(),
            to_term:             stok.clone(),
            kind:                LexiconRelKind::CommitBridge,
            confidence:          confidence * 0.85, // slightly lower for reverse direction
            co_occurrence_count: count,
        };
        store.upsert_lexicon_relationship(&rel_fwd, repo_path)?;
        store.upsert_lexicon_relationship(&rel_bwd, repo_path)?;
    }

    Ok(())
}

/// Pass 3: morphological derivation.
///
/// For each path token ending in a regular English verb suffix (*ize, *ise),
/// generate the derived noun/agent forms that a user might query.  Store them
/// as MorphologicalVariant relationships FROM the derived form TO the path token,
/// so that `lexicon_expand("tokenization", repo)` returns "tokenize".
///
/// Only generates forms that are NOT themselves already path tokens — if a repo
/// has both "tokenize.rs" and "tokenization.rs" the user's query lands directly.
fn pass3_morphological(
    file_paths: &[String],
    repo_path:  &str,
    store:      &Store,
) -> Result<()> {
    let path_token_set: HashSet<String> = file_paths
        .iter()
        .flat_map(|p| decompose_path(p))
        .filter(|t| !is_generic(t))
        .collect();

    for token in &path_token_set {
        let variants = morphological_variants(token);
        for (variant, confidence) in variants {
            // Skip if the variant is already a first-class path token.
            if path_token_set.contains(&variant) { continue; }
            if is_generic(&variant)              { continue; }
            let rel = LexiconRelationship {
                from_term:           variant,
                to_term:             token.clone(),
                kind:                LexiconRelKind::MorphologicalVariant,
                confidence,
                co_occurrence_count: 0,
            };
            store.upsert_lexicon_relationship(&rel, repo_path)?;
        }
    }
    Ok(())
}

/// Common verb prefixes whose removal can expose a root that has morphological variants.
/// "pretokenize" → strip "pre" → "tokenize" → generate "tokenization" → "pretokenize".
const VERB_PREFIXES: &[&str] = &[
    "pre", "de", "re", "un", "over", "out", "co", "dis", "mis",
];

/// Derive user-query forms from a repository path token.
///
/// Two sources of variants:
///   1. Direct: the token itself ends in a morphological suffix (*ize → *ization, etc.)
///   2. Prefix-stripped: the token starts with a known verb prefix; strip it and derive
///      variants of the root, pointing back to the original token.
///      "pretokenize" → "pre" + "tokenize" → generates "tokenization" → "pretokenize".
///
/// Returns (derived_form, confidence) pairs.
fn morphological_variants(token: &str) -> Vec<(String, f32)> {
    let mut v = Vec::new();

    // Direct variants from the full token.
    v.extend(ize_variants(token, 1.0));

    // Prefix-stripped variants: expose the root and derive from it.
    for prefix in VERB_PREFIXES {
        if let Some(root) = token.strip_prefix(prefix) {
            if root.len() >= 4 {
                // Slightly lower confidence since we stripped a prefix.
                v.extend(ize_variants(root, 0.85));
            }
        }
    }

    v
}

/// Generate *ization/*iser/*ising/*ised forms from a token ending in *ize or *ise.
/// `scale` is multiplied into the base confidences (use 1.0 for direct, 0.85 for prefix-stripped).
fn ize_variants(token: &str, scale: f32) -> Vec<(String, f32)> {
    let mut v = Vec::new();

    if let Some(stem) = token.strip_suffix("ize") {
        if stem.len() >= 3 {
            v.push((format!("{}ization", stem), 0.90 * scale));
            v.push((format!("{}izer",    stem), 0.85 * scale));
            v.push((format!("{}izing",   stem), 0.80 * scale));
            v.push((format!("{}ized",    stem), 0.80 * scale));
        }
    }

    if let Some(stem) = token.strip_suffix("ise") {
        if stem.len() >= 3 {
            v.push((format!("{}isation", stem), 0.90 * scale));
            v.push((format!("{}iser",    stem), 0.85 * scale));
            v.push((format!("{}ising",   stem), 0.80 * scale));
            v.push((format!("{}ised",    stem), 0.80 * scale));
        }
    }

    v
}

fn compute_bridge_confidence(count: u32) -> f32 {
    // Asymptotically approaches 0.85 as count grows.
    // k=0.15 ensures the MIN_CONFIDENCE threshold (0.65) is crossed at exactly
    // MIN_CO_OCCURRENCE (5): count=5 → 0.685, count=10 → 0.772, count=25 → 0.842
    // Previously k=0.04 gave count=5 → 0.563, below the query threshold — entries
    // were stored but always discarded, making the 5-occurrence floor meaningless.
    let base = 0.50_f32;
    let max  = 0.85_f32;
    let k    = 0.15_f32;
    base + (max - base) * (1.0 - (-k * count as f32).exp())
}

/// Public entry point called from ingest.
/// Clears and rebuilds the full lexicon for `repo_path`.
pub fn build_lexicon(repo_path: &str, store: &Store) -> Result<usize> {
    store.clear_lexicon(repo_path)?;

    let file_paths = store.all_file_paths(repo_path)?;
    pass1_structural(&file_paths, repo_path, store)?;
    pass2_commit_bridge(repo_path, store)?;
    pass3_morphological(&file_paths, repo_path, store)?;

    let count = store.lexicon_count(repo_path)? as usize;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompose_path_simple() {
        let toks = decompose_path("lib/src/op_store.rs");
        assert!(toks.contains(&"op".to_string()), "got: {:?}", toks);
        assert!(toks.contains(&"store".to_string()), "got: {:?}", toks);
    }

    #[test]
    fn decompose_path_camel() {
        let toks = decompose_path("src/MutableRepo.rs");
        assert!(toks.contains(&"mutable".to_string()), "got: {:?}", toks);
        assert!(toks.contains(&"repo".to_string()), "got: {:?}", toks);
    }

    #[test]
    fn decompose_subject_strips_prefix() {
        let toks = decompose_subject("feat(backend): add conflict resolution logic");
        assert!(toks.contains(&"conflict".to_string()), "got: {:?}", toks);
        assert!(toks.contains(&"resolution".to_string()), "got: {:?}", toks);
        assert!(!toks.contains(&"feat".to_string()), "should strip prefix, got: {:?}", toks);
    }

    #[test]
    fn detect_abbreviation_prefix() {
        assert!(detect_abbreviation("op", "operation"));
        assert!(detect_abbreviation("repo", "repository"));
        assert!(!detect_abbreviation("xyz", "operation"));
    }

    #[test]
    fn detect_abbreviation_vowel_drop() {
        // "wrkr" from "worker" — consonants: wrkr
        assert!(detect_abbreviation("wrkr", "worker"));
    }

    #[test]
    fn bridge_confidence_grows() {
        let c5  = compute_bridge_confidence(5);
        let c10 = compute_bridge_confidence(10);
        let c50 = compute_bridge_confidence(50);
        // count=5 (MIN_CO_OCCURRENCE) must cross the 0.65 query threshold.
        assert!(c5 >= 0.65, "c5={} must be >= 0.65 (MIN_CONFIDENCE)", c5);
        assert!(c10 > c5,   "c10={} c5={}", c10, c5);
        assert!(c50 > c10,  "c50={} c10={}", c50, c10);
        assert!(c50 < 0.86, "c50={}", c50);
    }

    #[test]
    fn morphological_variants_ize() {
        let v = morphological_variants("tokenize");
        let forms: Vec<&str> = v.iter().map(|(s, _)| s.as_str()).collect();
        assert!(forms.contains(&"tokenization"), "got: {:?}", forms);
        assert!(forms.contains(&"tokenizer"),    "got: {:?}", forms);
    }

    #[test]
    fn morphological_variants_prefix_stripped() {
        // "pretokenize" → strip "pre" → "tokenize" → should generate "tokenization"
        let v = morphological_variants("pretokenize");
        let forms: Vec<&str> = v.iter().map(|(s, _)| s.as_str()).collect();
        assert!(forms.contains(&"tokenization"),     "expected tokenization, got: {:?}", forms);
        assert!(forms.contains(&"pretokenization"),  "expected pretokenization, got: {:?}", forms);
    }

    #[test]
    fn pass3_inserts_variant() {
        let store = atlas_storage::Store::open(":memory:").unwrap();
        let paths = vec!["src/pretokenize/tokenize.rs".to_string()];
        pass3_morphological(&paths, "/repo", &store).unwrap();
        let rows = store.lexicon_expand("tokenization", "/repo").unwrap();
        let found = rows.iter().any(|r| r.to_term == "tokenize");
        assert!(found, "expected tokenization→tokenize, got: {:?}", rows.iter().map(|r| &r.to_term).collect::<Vec<_>>());
    }

    #[test]
    fn build_lexicon_smoke() {
        let store = atlas_storage::Store::open(":memory:").unwrap();
        // Insert a few commits touching op_store.rs with subject mentioning "operation"
        use atlas_ir::Commit;
        use chrono::DateTime;
        let base_ts = 1_700_000_000_i64;
        for i in 0..5 {
            let c = Commit {
                hash:          format!("hash{:04}", i),
                short_hash:    format!("hash{:04}", i),
                message:       "fix: refactor operation logic".to_string(),
                author_name:   "Alice".to_string(),
                author_email:  "a@x.com".to_string(),
                timestamp:     DateTime::from_timestamp(base_ts + i, 0).unwrap(),
                files_changed: vec!["lib/src/op_store.rs".to_string()],

            parents:       vec![],            };
            store.insert_commit(&c, "/repo").unwrap();
        }
        let count = build_lexicon("/repo", &store).unwrap();
        assert!(count > 0, "expected some lexicon entries, got {}", count);
        let expansions = store.lexicon_expand("operation", "/repo").unwrap();
        // "operation" should expand to "op" via CommitBridge (subject "operation" co-occurs with path token "op")
        let has_op = expansions.iter().any(|r| r.to_term == "op");
        assert!(has_op, "expected operation→op bridge, got: {:?}", expansions.iter().map(|r| &r.to_term).collect::<Vec<_>>());
    }
}
