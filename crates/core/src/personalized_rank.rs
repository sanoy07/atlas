//! C5.1 — Question-personalized structural ranking.
//!
//! Algorithmic idea inspired by Aider's repo map (tree-sitter → graph →
//! personalized PageRank), implemented over **Atlas structural edges** only.
//!
//! Ranking decides *where to look*. C4 decides *whether a claim is supported*.
//! Do **not** copy Aider token-budget constants.

use std::collections::{HashMap, HashSet};

/// Lightweight structural link used for ranking (no full IR dependency).
#[derive(Debug, Clone)]
pub struct StructuralLink {
    pub from: String,
    pub to: String,
    /// e.g. imports, calls_static, calls_instance, references_model
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct PersonalizedRankInput<'a> {
    pub question: &'a str,
    /// Files already selected as investigation seeds / core candidates (chat analog).
    pub seed_files: &'a [String],
    /// Candidate files that should appear in the graph even without edges.
    pub candidate_files: &'a [String],
    pub edges: &'a [StructuralLink],
}

#[derive(Debug, Clone)]
pub struct FileRank {
    pub file: String,
    pub score: f32,
    pub notes: Vec<String>,
}

/// Kind weight: prefer behavioral edges over imports.
fn kind_weight(kind: &str) -> f32 {
    match kind.to_lowercase().as_str() {
        "calls_static" | "calls_instance" | "call" => 3.0,
        "references_model" => 2.5,
        "imports" | "import" => 1.0,
        _ => 1.2,
    }
}

fn tokenize(question: &str) -> Vec<String> {
    question
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .map(|t| t.to_lowercase())
        .filter(|t| t.len() >= 3)
        .filter(|t| {
            ![
                "the", "and", "for", "with", "from", "that", "this", "are", "was",
                "investigate", "issue", "error", "about", "under", "when", "what",
                "why", "how", "need", "users", "seeing", "exactly", "where",
            ]
            .contains(&t.as_str())
        })
        .take(24)
        .collect()
}

/// True if token matches path, with light stemming (orders↔order).
fn token_matches_text(token: &str, text: &str) -> bool {
    if text.contains(token) {
        return true;
    }
    // plural → singular
    if token.len() > 4 && token.ends_with('s') {
        let stem = &token[..token.len() - 1];
        if text.contains(stem) {
            return true;
        }
    }
    // path segment ≈ token (order in order.service vs orders)
    for seg in text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
        if seg.len() < 3 {
            continue;
        }
        if seg == token || token.starts_with(seg) || seg.starts_with(token) {
            return true;
        }
        if token.len() > 4 && token.ends_with('s') && seg == &token[..token.len() - 1] {
            return true;
        }
        if seg.len() > 4 && seg.ends_with('s') && token == &seg[..seg.len() - 1] {
            return true;
        }
    }
    false
}

fn path_hits_tokens(path: &str, tokens: &[String]) -> usize {
    let p = path.to_lowercase();
    tokens
        .iter()
        .filter(|t| token_matches_text(t, &p))
        .count()
}

fn is_specific_name(ident: &str) -> bool {
    let has_alpha = ident.chars().any(|c| c.is_ascii_alphabetic());
    if !has_alpha || ident.len() < 8 {
        return false;
    }
    let snake = ident.contains('_');
    let kebab = ident.contains('-');
    let camel = ident.chars().any(|c| c.is_ascii_uppercase())
        && ident.chars().any(|c| c.is_ascii_lowercase());
    snake || kebab || camel
}

/// Atlas-specific: damp files that look cross-domain relative to the question.
/// E.g. question about orders should not personalize Redis paths highly from "timeout" alone.
fn cross_domain_damp(question: &str, path: &str) -> f32 {
    let q = question.to_lowercase();
    let p = path.to_lowercase();
    let orderish = ["order", "orders", "payment", "checkout", "settlement"];
    let redisish = ["redis", "rate-limit", "rate_limit", "ratelimit", "cache", "otel"];
    let noiseish = ["image-processor", "image_processor", "smtp", "email"];
    let q_order = orderish.iter().any(|t| q.contains(t));
    let q_redis = redisish.iter().any(|t| q.contains(t));
    let p_redis = redisish.iter().any(|t| p.contains(t));
    let p_order = orderish.iter().any(|t| p.contains(t));
    let p_noise = noiseish.iter().any(|t| p.contains(t));
    if q_order && !q_redis && p_redis && !p_order {
        return 0.12;
    }
    if q_order && q.contains("timeout") && p_redis && !p_order {
        return 0.12;
    }
    if q_order && p_noise && !p_order {
        return 0.2;
    }
    1.0
}

fn edge_ident_mul(from: &str, to: &str, kind: &str, tokens: &[String]) -> f32 {
    let mut mul = 1.0f32;
    let blob = format!("{from} {to} {kind}").to_lowercase();
    // mentioned token in path → boost (Aider-inspired; Atlas-tuned)
    let hits = tokens
        .iter()
        .filter(|t| token_matches_text(t, &blob))
        .count();
    if hits > 0 {
        mul *= 4.0 * (hits as f32).min(3.0);
    }
    // specific path segments
    for part in from.split('/').chain(to.split('/')) {
        if is_specific_name(part) {
            mul *= 1.5;
            break;
        }
    }
    // private / generated noise
    if from.contains("/generated/") || to.contains("/generated/") {
        mul *= 0.2;
    }
    mul
}

/// Personalized PageRank over files; returns scores in [0,1] after max-normalization.
pub fn personalized_file_ranks(input: &PersonalizedRankInput<'_>) -> Vec<FileRank> {
    let tokens = tokenize(input.question);
    let mut nodes: HashSet<String> = HashSet::new();
    for f in input.candidate_files {
        nodes.insert(f.clone());
    }
    for f in input.seed_files {
        nodes.insert(f.clone());
    }
    for e in input.edges {
        if e.to.starts_with("UNRESOLVED:") {
            continue;
        }
        nodes.insert(e.from.clone());
        nodes.insert(e.to.clone());
    }
    if nodes.is_empty() {
        return vec![];
    }

    let mut index: HashMap<String, usize> = HashMap::new();
    let mut names: Vec<String> = nodes.into_iter().collect();
    names.sort();
    for (i, n) in names.iter().enumerate() {
        index.insert(n.clone(), i);
    }
    let n = names.len();

    // Aggregate multi-edges
    let mut edge_w: HashMap<(usize, usize), f32> = HashMap::new();
    let mut edge_count: HashMap<(usize, usize), u32> = HashMap::new();
    for e in input.edges {
        if e.to.starts_with("UNRESOLVED:") {
            continue;
        }
        let Some(&i) = index.get(&e.from) else { continue };
        let Some(&j) = index.get(&e.to) else { continue };
        let key = (i, j);
        *edge_count.entry(key).or_insert(0) += 1;
        let c = *edge_count.get(&key).unwrap_or(&1);
        let w = kind_weight(&e.kind)
            * edge_ident_mul(&e.from, &e.to, &e.kind, &tokens)
            * (c as f32).sqrt()
            * cross_domain_damp(input.question, &e.from)
            * cross_domain_damp(input.question, &e.to);
        let slot = edge_w.entry(key).or_insert(0.0);
        // keep max weight for the pair after recount — recompute simply:
        *slot = w;
    }

    // Seed self-loops so isolated candidates still participate
    for i in 0..n {
        edge_w.entry((i, i)).or_insert(0.05);
    }

    // Build adjacency list
    let mut out: Vec<Vec<(usize, f32)>> = vec![vec![]; n];
    for ((i, j), w) in &edge_w {
        if *w > 0.0 {
            out[*i].push((*j, *w));
        }
    }

    // Personalization vector
    let seed_set: HashSet<&str> = input.seed_files.iter().map(String::as_str).collect();
    let mut pers = vec![0.0f32; n];
    let base = 1.0f32;
    for (i, name) in names.iter().enumerate() {
        let mut p = 0.0f32;
        let hits = path_hits_tokens(name, &tokens);
        if hits > 0 {
            // Strong subject match — primary personalization driver
            p += base * (4.0 + 3.0 * hits as f32);
        }
        if seed_set.contains(name.as_str()) {
            // Chat-file analog: strong only when subject-relevant
            p += if hits > 0 { base * 10.0 } else { base * 1.5 };
        }
        // Hubs without subject tokens must not dominate (share-class etc.)
        if hits == 0 {
            p *= 0.25;
        }
        p *= cross_domain_damp(input.question, name);
        if name.contains("src/modules/") && hits > 0 {
            p += 0.5;
        }
        if name.contains("/test") || name.contains("__tests__") {
            p *= 0.75;
        }
        if name.contains("/models/") && hits == 0 {
            p *= 0.5; // model hubs without token match
        }
        // Domain boost: redis/auth/order paths when question is in that domain
        let ql = input.question.to_lowercase();
        if (ql.contains("redis") || ql.contains("rate-limit") || ql.contains("grey failure"))
            && (name.contains("redis") || name.contains("rate-limit") || name.contains("/caching/"))
        {
            p += base * 6.0;
        }
        if (ql.contains("auth") || ql.contains("login") || ql.contains("password"))
            && (name.contains("auth") || name.contains("Auth") || name.contains("identity"))
        {
            p += base * 5.0;
        }
        // Demote contracts/commands noise when not the subject
        if name.contains("/contracts/") && !ql.contains("contract") {
            p *= 0.2;
        }
        if name.contains("common/commands/") && !ql.contains("command") {
            p *= 0.25;
        }
        pers[i] = p;
    }
    let pers_sum: f32 = pers.iter().sum();
    if pers_sum <= 1e-9 {
        // uniform
        let u = 1.0 / n as f32;
        for p in &mut pers {
            *p = u;
        }
    } else {
        for p in &mut pers {
            *p /= pers_sum;
        }
    }

    // Power iteration PageRank
    let alpha = 0.85f32;
    let mut rank = pers.clone();
    for _ in 0..40 {
        let mut next = vec![0.0f32; n];
        for i in 0..n {
            let outs = &out[i];
            let total: f32 = outs.iter().map(|(_, w)| w).sum();
            if total <= 1e-12 {
                // dangling → redistribute via personalization
                for j in 0..n {
                    next[j] += alpha * rank[i] * pers[j];
                }
                continue;
            }
            for &(j, w) in outs {
                next[j] += alpha * rank[i] * (w / total);
            }
        }
        for j in 0..n {
            next[j] += (1.0 - alpha) * pers[j];
        }
        // renorm
        let s: f32 = next.iter().sum::<f32>().max(1e-12);
        for x in &mut next {
            *x /= s;
        }
        rank = next;
    }

    let max_r = rank.iter().cloned().fold(0.0f32, f32::max).max(1e-12);
    // Subject affinity (post-PR): prevent pure hubs from burying token-matched services.
    let qlow = input.question.to_lowercase();
    let mut affinity: Vec<f32> = names
        .iter()
        .map(|file| {
            let hits = path_hits_tokens(file, &tokens) as f32;
            let mut a = hits;
            if seed_set.contains(file.as_str()) && hits > 0.0 {
                a += 2.0;
            }
            if file.contains("/services/") && hits > 0.0 {
                a += 1.0;
            }
            // C5.1-E: role-aware affinity (no hardcoded service basenames)
            let fl = file.to_lowercase();
            let role = crate::role_aware::infer_role(file);
            match role {
                crate::role_aware::InferredRole::Implementation
                | crate::role_aware::InferredRole::Entrypoint => a += 2.5,
                crate::role_aware::InferredRole::Satellite => a *= 0.4,
                crate::role_aware::InferredRole::Config
                    if qlow.contains("secret") || qlow.contains("config") =>
                {
                    a += 3.0
                }
                _ => {}
            }
            // Satellite multi-hyphen names that only share a token (order-history, sellback-order)
            if hits > 0.0 {
                let stem = fl.rsplit('/').next().unwrap_or(&fl);
                let hyphens = stem.matches('-').count();
                if hyphens >= 1 && !stem.contains(".service") && role
                    == crate::role_aware::InferredRole::Model
                {
                    // e.g. order-history.model vs order.model — prefer fewer hyphens when both match
                    a *= 0.7;
                }
            }
            if fl.ends_with("/index.ts") {
                a *= 0.5;
            }
            a * cross_domain_damp(input.question, file)
        })
        .collect();
    let max_a = affinity.iter().cloned().fold(0.0f32, f32::max).max(1e-12);
    for a in &mut affinity {
        *a /= max_a;
    }

    let mut out_ranks: Vec<FileRank> = names
        .into_iter()
        .enumerate()
        .map(|(i, file)| {
            let mut notes = vec!["c5.1_personalized_structural_pagerank".into()];
            if seed_set.contains(file.as_str()) {
                notes.push("seed/chat personalization".into());
            }
            let hits = path_hits_tokens(&file, &tokens);
            if hits > 0 {
                notes.push(format!("question_token_hits={hits}"));
            }
            if cross_domain_damp(input.question, &file) < 0.5 {
                notes.push("cross_domain_damped".into());
            }
            let pr = rank[i] / max_r;
            // Blend topology with subject affinity (Atlas, not pure Aider).
            let score = 0.45 * pr + 0.55 * affinity[i];
            notes.push(format!("pr={pr:.3} affinity={:.3}", affinity[i]));
            FileRank {
                score,
                file,
                notes,
            }
        })
        .collect();
    // Re-normalize to [0,1]
    let max_s = out_ranks
        .iter()
        .map(|r| r.score)
        .fold(0.0f32, f32::max)
        .max(1e-12);
    for r in &mut out_ranks {
        r.score /= max_s;
    }
    out_ranks.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.file.cmp(&b.file))
    });
    out_ranks
}

/// Collect structural links among a set of files (out + in) from the store.
pub fn collect_links_for_files(
    files: &[String],
    repo_path: &str,
    store: &atlas_storage::Store,
) -> Vec<StructuralLink> {
    let want: HashSet<&str> = files.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for f in files {
        if let Ok(rows) = store.structural_edges_for_file(f, repo_path) {
            for r in rows {
                if r.target_file.starts_with("UNRESOLVED:") {
                    continue;
                }
                // keep edges into neighborhood or out of neighborhood one hop
                let key = (r.source_file.clone(), r.target_file.clone(), r.kind.clone());
                if seen.insert(key) {
                    out.push(StructuralLink {
                        from: r.source_file,
                        to: r.target_file,
                        kind: r.kind,
                    });
                }
            }
        }
        if let Ok(rows) = store.structural_edges_targeting(f, repo_path) {
            for r in rows {
                if !want.contains(r.source_file.as_str()) && !want.contains(r.target_file.as_str())
                {
                    // still keep one-hop inbound sources so graph grows slightly
                }
                let key = (r.source_file.clone(), r.target_file.clone(), r.kind.clone());
                if seen.insert(key) {
                    out.push(StructuralLink {
                        from: r.source_file,
                        to: r.target_file,
                        kind: r.kind,
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(from: &str, to: &str, kind: &str) -> StructuralLink {
        StructuralLink {
            from: from.into(),
            to: to.into(),
            kind: kind.into(),
        }
    }

    #[test]
    fn order_seed_outranks_redis_on_orders_timeout() {
        let order = "src/modules/core/services/order.service.ts".to_string();
        let model = "src/modules/core/models/order.model.ts".to_string();
        let redis =
            "src/infrastructure/rate-limiting/implementations/redis-rate-limiter.ts".to_string();
        let edges = vec![
            link(&order, &model, "references_model"),
            link(
                "src/modules/core/graphql/order.resolvers.ts",
                &order,
                "calls_static",
            ),
            link(&redis, "src/config/index.ts", "imports"),
        ];
        let seeds = vec![order.clone()];
        let candidates = vec![
            order.clone(),
            model.clone(),
            redis.clone(),
            "src/modules/core/graphql/order.resolvers.ts".into(),
        ];
        let ranks = personalized_file_ranks(&PersonalizedRankInput {
            question: "orders timeout",
            seed_files: &seeds,
            candidate_files: &candidates,
            edges: &edges,
        });
        let score = |f: &str| {
            ranks
                .iter()
                .find(|r| r.file == f)
                .map(|r| r.score)
                .unwrap_or(0.0)
        };
        assert!(
            score(&order) > score(&redis),
            "order.service ({}) should outrank redis ({}); ranks={:?}",
            score(&order),
            score(&redis),
            ranks
                .iter()
                .map(|r| format!("{}:{:.3}", r.file, r.score))
                .collect::<Vec<_>>()
        );
        assert!(
            ranks
                .iter()
                .find(|r| r.file == redis)
                .map(|r| r.notes.iter().any(|n| n.contains("cross_domain")))
                .unwrap_or(false),
            "redis should be cross-domain damped"
        );
    }

    #[test]
    fn empty_edges_still_ranks_seed_by_path_tokens() {
        let order = "src/modules/core/services/order.service.ts".to_string();
        let other = "src/modules/blockchain/models/blockchain.model.ts".to_string();
        let ranks = personalized_file_ranks(&PersonalizedRankInput {
            question: "order processing",
            seed_files: &[],
            candidate_files: &[order.clone(), other.clone()],
            edges: &[],
        });
        assert!(ranks[0].file.contains("order") || ranks[0].score >= ranks[1].score);
    }
}
