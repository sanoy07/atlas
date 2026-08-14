//! C5.1-L — Identifier-weighted lexical relevance (GrepRAG / Sourcegraph-inspired).
//!
//! Makes lexical retrieval smarter without embeddings:
//! - exact identifier / path beats generic words
//! - structure-aware dedup reduces near-duplicate noise
//!
//! Operates on the **candidate bag** before C5.1 PageRank. Ranking still decides
//! presentation order; this decides who survives into the bag with quality scores.

use atlas_ir::CandidateArtifact;
use std::collections::{HashMap, HashSet};

/// Score a repository path against a question + anchors.
/// Higher = stronger retrieval relevance (not claim support).
pub fn score_path_for_question(path: &str, question: &str, anchors: &[String]) -> f32 {
    let q = question.to_lowercase();
    let p = path.to_lowercase();
    let basename = p.rsplit('/').next().unwrap_or(&p);
    let stem = basename
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(basename);

    let q_tokens = tokenize_query(&q);
    let mut score = 0.0f32;
    let mut notes_strong = 0u32;

    // --- Strong signals ---
    for a in anchors {
        let al = a.to_lowercase();
        if al.is_empty() {
            continue;
        }
        // Exact path
        if p == al || p.ends_with(&al) {
            score += 12.0;
            notes_strong += 1;
            continue;
        }
        // Exact basename / stem
        if basename == al || stem == al || stem == al.trim_end_matches(".ts") {
            score += 10.0;
            notes_strong += 1;
            continue;
        }
        // Identifier-like anchor (OrderService, order.service, redis-rate-limiter)
        if is_identifier_like(&al) {
            if p.contains(&al) || stem.contains(&al.replace('.', "-")) {
                score += 8.0;
                notes_strong += 1;
                continue;
            }
            // CamelCase split match
            for part in split_ident(&al) {
                if part.len() >= 4 && (stem.contains(&part) || p.contains(&part)) {
                    score += 3.0;
                }
            }
        }
        // Path component (order in …/order.service.ts)
        if path_component_hit(&p, &al) {
            score += if al.len() >= 5 { 5.0 } else { 2.0 };
        }
    }

    // Question tokens with graded weight
    for t in &q_tokens {
        let w = token_weight(t);
        if p.contains(t.as_str()) || stem.contains(t.as_str()) {
            score += w;
            if w >= 4.0 {
                notes_strong += 1;
            }
        } else if t.ends_with('s') && t.len() > 4 {
            let stem_t = &t[..t.len() - 1];
            if p.contains(stem_t) || stem.contains(stem_t) {
                score += w * 0.9;
            }
        }
    }

    // Issue / PR refs in path rarely; boost if question mentions issue and path is infra match
    if q.contains("redis")
        && (p.contains("redis") || p.contains("rate-limit") || p.contains("/caching/"))
    {
        score += 6.0;
        notes_strong += 1;
    }
    if (q.contains("auth") || q.contains("login"))
        && (p.contains("auth") || p.contains("identity") || stem.contains("auth"))
    {
        score += 5.0;
    }
    if (q.contains("order") || q.contains("orders"))
        && (p.contains("order") || stem.contains("order"))
    {
        score += 5.0;
        notes_strong += 1;
    }

    // --- Medium: repository role ---
    if p.contains("/services/") || p.contains("resolvers") {
        score += 1.5;
    }
    if p.contains("/models/") && notes_strong > 0 {
        score += 1.0;
    }

    // --- Path class soft prior (production > demo/asset/CI) ---
    score = crate::path_class::apply_class_to_score(path, question, score);
    // Compound subject stems: "operation store" → op_store exact stem
    score += crate::subject_resolve::subject_stem_boost(path, question);

    // --- Negative / damp ---
    if is_generated_or_vendor(&p) {
        score *= 0.05;
    }
    if is_test_path(&p) && !q.contains("test") {
        score *= 0.65;
    }
    if (p.contains("/contracts/") || p.contains("common/commands/"))
        && !q.contains("contract")
        && !q.contains("command")
    {
        score *= 0.25;
    }
    // Generic single-token-only paths without strong match
    if notes_strong == 0 && score < 3.0 {
        score *= 0.5;
    }
    // Prefer concrete implementations over index barrels when both match
    if (basename == "index.ts" || basename == "index.js") && notes_strong < 2 {
        score *= 0.55;
    }

    // C5.1-E: role-shaped primacy (service/entrypoint vs satellite), not repo hardcodes
    score += primary_artifact_boost(stem, &q);

    score
}

/// Prefer implementation/entrypoint stems over satellites (hyphen count as secondary cue).
fn primary_artifact_boost(stem: &str, q: &str) -> f32 {
    let mut b = 0.0f32;
    let role = crate::role_aware::infer_role(&format!("placeholder/{stem}.ts"));
    match role {
        crate::role_aware::InferredRole::Implementation => b += 5.0,
        crate::role_aware::InferredRole::Entrypoint => b += 4.0,
        crate::role_aware::InferredRole::Model => b += 2.0,
        crate::role_aware::InferredRole::Config => {
            if q.contains("secret") || q.contains("config") || q.contains("startup") {
                b += 5.0;
            } else {
                b += 1.0;
            }
        }
        crate::role_aware::InferredRole::Satellite => b -= 3.0,
        _ => {}
    }
    let hyphens = stem.matches('-').count();
    if hyphens == 0 && (stem.contains("service") || stem.contains("model")) {
        b += 2.0;
    } else if hyphens >= 2 {
        b -= 1.5;
    }
    if stem == "index" {
        b -= 3.0;
    }
    b
}

fn tokenize_query(q: &str) -> Vec<String> {
    q.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && c != '.')
        .map(|t| t.to_lowercase())
        .filter(|t| t.len() >= 3)
        .filter(|t| {
            ![
                "the", "and", "for", "with", "from", "that", "this", "are", "was",
                "users", "seeing", "exactly", "where", "what", "when", "how", "why",
                "need", "does", "into", "under", "about", "github", "issue", "feat",
                "implement", "configure", "prevent", "during", "after",
            ]
            .contains(&t.as_str())
        })
        .take(32)
        .collect()
}

fn token_weight(t: &str) -> f32 {
    // Strong: identifier-ish or domain-specific
    if is_identifier_like(t) && t.len() >= 6 {
        return 6.0;
    }
    match t {
        "timeout" | "error" | "fail" | "failure" | "login" | "password" | "auth"
        | "authentication" | "redis" | "order" | "orders" | "checkout" | "settlement"
        | "concurrent" | "race" | "grey" | "gray" => 4.0,
        "service" | "model" | "handler" | "module" | "system" | "process" | "processing"
        | "create" | "created" | "flow" | "explain" => 1.0,
        _ if t.len() >= 8 => 3.0,
        _ => 1.5,
    }
}

fn is_identifier_like(s: &str) -> bool {
    s.contains('_')
        || s.contains('-')
        || s.contains('.')
        || (s.chars().any(|c| c.is_ascii_uppercase()) && s.chars().any(|c| c.is_ascii_lowercase()))
        || s.chars().all(|c| c.is_ascii_alphanumeric()) && s.len() >= 6 && s.chars().any(|c| c.is_ascii_uppercase())
}

fn split_ident(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in s.split(|c: char| c == '_' || c == '-' || c == '.' || c == '/') {
        if part.is_empty() {
            continue;
        }
        // CamelCase split
        let mut cur = String::new();
        for (i, ch) in part.chars().enumerate() {
            if i > 0 && ch.is_ascii_uppercase() && !cur.is_empty() {
                out.push(cur.to_lowercase());
                cur.clear();
            }
            cur.push(ch);
        }
        if !cur.is_empty() {
            out.push(cur.to_lowercase());
        }
    }
    out
}

fn path_component_hit(path: &str, token: &str) -> bool {
    path.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .any(|seg| seg == token || (token.len() >= 4 && seg.contains(token)))
}

fn is_generated_or_vendor(p: &str) -> bool {
    p.contains("node_modules/")
        || p.contains("/dist/")
        || p.contains("/build/")
        || p.contains("generated/")
        || p.contains(".min.")
}

fn is_test_path(p: &str) -> bool {
    p.contains("/test")
        || p.contains("__tests__")
        || p.contains(".test.")
        || p.contains(".spec.")
        || p.starts_with("tests/")
}

/// Re-score and sort candidates; keep forced seeds first then by lexical score.
pub fn rerank_candidates(
    candidates: Vec<CandidateArtifact>,
    question: &str,
    anchors: &[String],
    forced_seeds: &[String],
) -> Vec<CandidateArtifact> {
    let seed_set: HashSet<&str> = forced_seeds.iter().map(String::as_str).collect();
    let mut scored: Vec<(f32, CandidateArtifact)> = candidates
        .into_iter()
        .map(|mut c| {
            let mut s = score_path_for_question(&c.file, question, anchors);
            if seed_set.contains(c.file.as_str()) {
                s += 15.0; // retrieval expansion seeds always retained with high priority
            }
            // Write into score.lexical for transparency
            c.score.lexical = (s / 20.0).clamp(0.0, 1.0);
            c.score.total = c.score.total.max(c.score.lexical);
            (s, c)
        })
        .collect();

    scored.sort_by(|a, b| {
        let a_seed = seed_set.contains(a.1.file.as_str());
        let b_seed = seed_set.contains(b.1.file.as_str());
        b_seed
            .cmp(&a_seed)
            .then_with(|| {
                b.0.partial_cmp(&a.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.1.file.cmp(&b.1.file))
    });

    let mut out: Vec<CandidateArtifact> = scored.into_iter().map(|(_, c)| c).collect();
    out = structure_aware_dedup(out, question);
    out
}

/// GrepRAG-style structure-aware dedup: collapse near-duplicates.
/// - Prefer `foo.service.ts` over `foo/index.ts` when both are weak
/// - Cap low-scoring files per parent directory
pub fn structure_aware_dedup(
    candidates: Vec<CandidateArtifact>,
    question: &str,
) -> Vec<CandidateArtifact> {
    let q = question.to_lowercase();
    let mut seen_stems: HashMap<String, usize> = HashMap::new();
    let mut per_dir: HashMap<String, usize> = HashMap::new();
    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut out = Vec::new();

    for c in candidates {
        if !seen_paths.insert(c.file.clone()) {
            continue; // exact path duplicate
        }
        let p = c.file.to_lowercase();
        let basename = p.rsplit('/').next().unwrap_or(&p);
        let stem = basename
            .rsplit_once('.')
            .map(|(s, _)| s)
            .unwrap_or(basename)
            .to_string();
        let parent = p.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();

        let strong = c.score.lexical >= 0.45
            || p.contains("order") && q.contains("order")
            || p.contains("redis") && q.contains("redis")
            || p.contains("auth") && (q.contains("auth") || q.contains("login"));

        // Drop weak index.ts barrels when a stronger sibling already kept
        if (basename == "index.ts" || basename == "index.js") && !strong {
            if seen_stems.values().any(|&n| n > 0) && c.score.lexical < 0.5 {
                // allow one index if nothing else from parent
                let cnt = per_dir.get(&parent).copied().unwrap_or(0);
                if cnt >= 1 {
                    continue;
                }
            }
        }

        let stem_count = seen_stems.entry(stem.clone()).or_insert(0);
        // Same stem in different folders: keep first (higher ranked) + one more if strong
        if *stem_count >= 1 && !strong && c.score.lexical < 0.4 {
            continue;
        }

        let dir_count = per_dir.entry(parent.clone()).or_insert(0);
        // Cap weak files per directory (contracts/commands dumps)
        if *dir_count >= 3 && !strong && c.score.lexical < 0.5 {
            continue;
        }
        if *dir_count >= 5 && c.score.lexical < 0.7 {
            continue;
        }

        *stem_count += 1;
        *dir_count += 1;
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_ir::{ArtifactRole, CandidateReason, ScoreBreakdown};

    fn cand(path: &str) -> CandidateArtifact {
        CandidateArtifact {
            file: path.into(),
            role: ArtifactRole::ProductionSource,
            reasons: vec![CandidateReason::AnchorMatch {
                anchor: "x".into(),
                via: "test".into(),
            }],
            score: ScoreBreakdown::default(),
        }
    }

    #[test]
    fn order_service_beats_generic_service_word() {
        let q = "orders timeout";
        let anchors = vec!["orders".into(), "timeout".into()];
        let a = score_path_for_question(
            "src/modules/core/services/order.service.ts",
            q,
            &anchors,
        );
        let b = score_path_for_question(
            "src/modules/core/services/global-settings.service.ts",
            q,
            &anchors,
        );
        assert!(a > b, "order.service {a} should beat global-settings {b}");
    }

    #[test]
    fn redis_rate_limiter_beats_blockchain_for_redis_question() {
        let q = "GitHub issue #19: Configure Redis Command Timeout";
        let anchors = vec!["redis".into(), "timeout".into(), "issue#19".into()];
        let a = score_path_for_question(
            "src/infrastructure/rate-limiting/implementations/redis-rate-limiter.ts",
            q,
            &anchors,
        );
        let b = score_path_for_question(
            "src/common/commands/blockchain/constants.ts",
            q,
            &anchors,
        );
        assert!(a > b * 2.0, "redis-rate-limiter {a} >> blockchain constants {b}");
    }

    #[test]
    fn authservice_beats_logger() {
        let q = "authentication fails after password reset";
        let anchors = vec!["authentication".into(), "password".into()];
        let a = score_path_for_question(
            "src/modules/identity/auth/AuthService.ts",
            q,
            &anchors,
        );
        let b = score_path_for_question(
            "src/infrastructure/logger/logger.interface.ts",
            q,
            &anchors,
        );
        assert!(a > b, "AuthService {a} > logger {b}");
    }

    #[test]
    fn dedup_drops_extra_weak_index() {
        let q = "redis timeout";
        let cands = vec![
            cand("src/infrastructure/messaging/redis/connection.ts"),
            cand("src/infrastructure/messaging/redis/index.ts"),
            cand("src/infrastructure/messaging/redis/index.ts"),
        ];
        let out = structure_aware_dedup(
            rerank_candidates(cands, q, &["redis".into()], &[]),
            q,
        );
        let indexes = out
            .iter()
            .filter(|c| c.file.ends_with("index.ts"))
            .count();
        assert!(indexes <= 1, "expected ≤1 index.ts, got {indexes}");
    }
}
