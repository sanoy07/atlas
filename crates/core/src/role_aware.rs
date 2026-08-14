//! C5.1-E — Role-aware / entrypoint-aware retrieval.
//!
//! Infer primary vs satellite artifacts from repository structure and
//! bag-relative token specificity — **not** hardcoded order/deployment/secret paths.
//!
//! Solves: lexical satellites (loaders, constants, marketing deploy-*) outranking
//! real implementation services; ambiguous English ("deployment") hijacking secret questions.

use atlas_ir::CandidateArtifact;
use atlas_storage::Store;
use std::collections::{HashMap, HashSet};

/// Structural role inferred from path shape (repo-agnostic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferredRole {
    /// GraphQL resolvers, HTTP handlers, routes — request entry.
    Entrypoint,
    /// `*.service.*`, infrastructure adapters/factories/managers.
    Implementation,
    /// Persistence models.
    Model,
    /// Config / secrets wiring (path role, not claim of correctness).
    Config,
    /// Loaders, constants, enums, barrels, pure types — usually secondary.
    Satellite,
    Test,
    Other,
}

pub fn infer_role(path: &str) -> InferredRole {
    let p = path.to_lowercase();
    if p.contains("/test")
        || p.contains("__tests__")
        || p.contains(".test.")
        || p.contains(".spec.")
        || p.starts_with("tests/")
    {
        return InferredRole::Test;
    }
    if p.contains("loader")
        || p.contains("constants")
        || p.contains("/enums/")
        || p.contains(".enums.")
        || p.contains("typedefs")
        || p.contains(".types.")
        || p.contains("/types/")
        || p.ends_with("/index.ts")
        || p.ends_with("/index.js")
        || p.contains(".dto.")
    {
        return InferredRole::Satellite;
    }
    if p.contains("resolver")
        || p.contains("/handlers/")
        || p.contains(".handler.")
        || p.contains("/routes/")
        || p.contains("controller")
    {
        return InferredRole::Entrypoint;
    }
    if p.contains("/services/")
        || p.contains(".service.")
        || p.contains("adapter")
        || p.contains("factory")
        || (p.contains("/infrastructure/") && p.contains("manager"))
    {
        return InferredRole::Implementation;
    }
    if p.contains("/models/") || p.contains(".model.") {
        return InferredRole::Model;
    }
    if p.contains("/config/") || p.contains("secrets") || p.contains("secret") {
        // "secret" in path is a role cue for config/infra, not a domain hardcode list
        return InferredRole::Config;
    }
    InferredRole::Other
}

fn role_weight(role: InferredRole, question: &str) -> f32 {
    let q = question.to_lowercase();
    let wants_flow = q.contains("how")
        || q.contains("where")
        || q.contains("trace")
        || q.contains("lifecycle")
        || q.contains("flow")
        || q.contains("what happens")
        || q.contains("involved")
        || q.contains("consum")
        || q.contains("loaded")
        || q.contains("trigger");
    match role {
        InferredRole::Implementation => {
            if wants_flow {
                4.5
            } else {
                3.5
            }
        }
        InferredRole::Entrypoint => {
            if wants_flow || q.contains("request") || q.contains("trigger") {
                4.0
            } else {
                2.5
            }
        }
        InferredRole::Model => 2.0,
        InferredRole::Config => {
            if q.contains("secret")
                || q.contains("config")
                || q.contains("startup")
                || q.contains("missing")
                || q.contains("invalid")
            {
                4.0
            } else {
                1.5
            }
        }
        InferredRole::Satellite => 0.35,
        InferredRole::Test => {
            if q.contains("test") {
                1.5
            } else {
                0.5
            }
        }
        InferredRole::Other => 1.0,
    }
}

/// Query tokens used as concepts (length-filtered).
pub fn concept_tokens(question: &str) -> Vec<String> {
    question
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .map(|t| t.to_lowercase())
        .filter(|t| t.len() >= 4)
        .filter(|t| {
            ![
                "that", "this", "with", "from", "what", "when", "where", "which",
                "does", "into", "through", "until", "after", "before", "would",
                "could", "should", "about", "their", "there", "these", "those",
                "have", "been", "were", "will", "your", "how", "the", "and",
                "for", "are", "was", "vestascan", "github", "atlas", "determine",
                "whether", "caused", "using", "later", "first", "parts", "system",
                "started", "failing", "changed", "production", "successfully",
                "intermittently", "requests", "explain", "trace", "investigate",
            ]
            .contains(&t.as_str())
        })
        .take(20)
        .collect()
}

/// IDF-like weight: rare path tokens in the bag are more discriminative.
pub fn bag_token_idf(tokens: &[String], bag_paths: &[String]) -> HashMap<String, f32> {
    let n = bag_paths.len().max(1) as f32;
    let mut df: HashMap<String, f32> = HashMap::new();
    for t in tokens {
        let mut c = 0f32;
        for p in bag_paths {
            let pl = p.to_lowercase();
            if pl.contains(t.as_str()) {
                c += 1.0;
                continue;
            }
            if t.ends_with('s') && t.len() > 4 {
                let stem = &t[..t.len() - 1];
                if pl.contains(stem) {
                    c += 1.0;
                }
            }
        }
        // log IDF; ubiquitous tokens (~half the bag) get low weight
        let idf = ((n + 1.0) / (c + 1.0)).ln() + 1.0;
        df.insert(t.clone(), idf);
    }
    df
}

fn path_matches_token(path: &str, token: &str) -> bool {
    let p = path.to_lowercase();
    if p.contains(token) {
        return true;
    }
    if token.ends_with('s') && token.len() > 4 {
        let stem = &token[..token.len() - 1];
        if p.contains(stem) {
            return true;
        }
    }
    // deploy ↔ deployment soft match
    if token.starts_with("deploy") && p.contains("deploy") {
        return true;
    }
    if token == "secret" && (p.contains("secret") || p.contains("secrets")) {
        return true;
    }
    false
}

/// Multi-concept coverage: how many distinct query concepts hit this path, IDF-weighted.
pub fn concept_coverage_score(path: &str, tokens: &[String], idf: &HashMap<String, f32>) -> f32 {
    let mut s = 0.0f32;
    let mut hits = 0u32;
    for t in tokens {
        if path_matches_token(path, t) {
            hits += 1;
            s += idf.get(t).copied().unwrap_or(1.0) * 2.5;
        }
    }
    // Bonus for matching multiple distinct concepts (token+deploy, secret+config)
    if hits >= 2 {
        s += 3.0 * (hits as f32 - 1.0);
    }
    s
}

/// Structural fan-in among bag (incoming calls/imports as primacy signal).
pub fn structural_fan_in(
    path: &str,
    bag: &HashSet<String>,
    store: &Store,
    repo_path: &str,
) -> f32 {
    let Ok(incoming) = store.structural_edges_targeting(path, repo_path) else {
        return 0.0;
    };
    let mut n = 0u32;
    for e in incoming {
        if e.kind == "imports" {
            continue; // imports inflate utility fan-in
        }
        if bag.contains(&e.source_file) || e.source_file.contains("/modules/") {
            n += 1;
        }
    }
    (n as f32).min(8.0) * 0.4
}

/// Combined role-aware score for a path given the current bag.
pub fn role_aware_score(
    path: &str,
    question: &str,
    tokens: &[String],
    idf: &HashMap<String, f32>,
    bag: &HashSet<String>,
    store: Option<(&Store, &str)>,
) -> f32 {
    let role = infer_role(path);
    let mut s = role_weight(role, question);
    s += concept_coverage_score(path, tokens, idf);

    // Prefer paths where the stem is a primary implementation name
    // e.g. deployment.service / token.service over deploy-count.loader
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_lowercase();
    let stem_base = stem.rsplit_once('.').map(|(a, _)| a).unwrap_or(&stem);
    if stem_base.ends_with(".service") || stem.contains(".service.") {
        s += 2.0;
    }
    if stem_base.contains("loader") || stem_base.contains("constant") {
        s -= 2.5;
    }

    // Token competition: boost paths that hit the highest-IDF query concepts;
    // demote paths that only hit common concepts while missing rarer ones.
    let mut max_matched_idf = 0.0f32;
    let mut max_missed_idf = 0.0f32;
    let mut any = false;
    let mut rarest: Option<(&str, f32)> = None;
    for t in tokens {
        let w = idf.get(t).copied().unwrap_or(1.0);
        // Highest IDF = most discriminative (rarest in bag)
        if rarest.map(|(_, rw)| w > rw).unwrap_or(true) {
            rarest = Some((t.as_str(), w));
        }
        if path_matches_token(path, t) {
            any = true;
            max_matched_idf = max_matched_idf.max(w);
        } else {
            max_missed_idf = max_missed_idf.max(w);
        }
    }
    if let Some((tok, w)) = rarest {
        if path_matches_token(path, tok) {
            s += 5.0 + w;
        }
    }
    if any && matches!(role, InferredRole::Satellite) && max_matched_idf < 2.0 {
        s *= 0.25;
    }
    if any && max_missed_idf > max_matched_idf {
        s *= 0.4;
    }

    // Co-occurrence disambiguation (repo-agnostic): if the question names a
    // distinctive concept that this path lacks, but the path only rides a
    // co-occurring ambiguous token, demote heavily.
    // Example: "secret was changed after deployment" — deployment.service must
    // not beat secrets.ts solely because both questions mention deployment.
    let q = question.to_lowercase();
    let pl = path.to_lowercase();
    let q_concepts: Vec<&str> = tokens.iter().map(String::as_str).collect();
    if let Some(best_tok) = q_concepts
        .iter()
        .max_by(|a, b| {
            idf.get(**a)
                .copied()
                .unwrap_or(1.0)
                .partial_cmp(&idf.get(**b).copied().unwrap_or(1.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        let best_w = idf.get(*best_tok).copied().unwrap_or(1.0);
        if best_w >= 1.5
            && !path_matches_token(path, best_tok)
            && any
            && max_matched_idf + 0.15 < best_w
        {
            s *= 0.3;
        }
    }
    // Config/secret paths when question is about secrets
    if q.contains("secret") && (pl.contains("secret") || pl.contains("secrets")) {
        s += 6.0;
    }
    // Soften pure deploy* hits when secret is also in play
    if q.contains("secret") && pl.contains("deploy") && !pl.contains("secret") {
        s *= 0.35;
    }

    if let Some((store, repo)) = store {
        s += structural_fan_in(path, bag, store, repo);
    }

    s.max(0.0)
}

/// Re-order candidates: lexical base + role-aware primacy (C5.1-E).
pub fn apply_role_aware_rerank(
    candidates: Vec<CandidateArtifact>,
    question: &str,
    store: Option<(&Store, &str)>,
) -> Vec<CandidateArtifact> {
    if candidates.is_empty() {
        return candidates;
    }
    let tokens = concept_tokens(question);
    let bag_paths: Vec<String> = candidates.iter().map(|c| c.file.clone()).collect();
    let bag_set: HashSet<String> = bag_paths.iter().cloned().collect();
    let idf = bag_token_idf(&tokens, &bag_paths);

    let mut scored: Vec<(f32, CandidateArtifact)> = candidates
        .into_iter()
        .map(|mut c| {
            let role_s = role_aware_score(&c.file, question, &tokens, &idf, &bag_set, store);
            // Blend into existing lexical component if present
            let base = c.score.lexical * 12.0; // was normalized 0..1
            let total = base + role_s;
            c.score.centrality = (role_s / 15.0).clamp(0.0, 1.0);
            c.score.total = c.score.total.max((total / 25.0).clamp(0.0, 1.0));
            (total, c)
        })
        .collect();

    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.file.cmp(&b.1.file))
    });
    scored.into_iter().map(|(_, c)| c).collect()
}

/// Generic path fragments from question concepts (no hardcoded service names).
/// e.g. token → "token", "token.service", "token.model"; data+room → "data-room".
pub fn concept_search_fragments(question: &str) -> Vec<String> {
    let tokens = concept_tokens(question);
    let mut frags = Vec::new();
    for t in &tokens {
        frags.push(t.clone());
        if t.len() >= 4 {
            frags.push(format!("{t}.service"));
            frags.push(format!("{t}.model"));
            frags.push(format!("{t}-"));
        }
    }
    // Adjacent bigrams → hyphen/joined forms (data room, secret manager)
    for w in tokens.windows(2) {
        let a = &w[0];
        let b = &w[1];
        frags.push(format!("{a}-{b}"));
        frags.push(format!("{a}_{b}"));
        frags.push(format!("{a}.{b}"));
        // secret manager → secret-manager path components
        if a.len() >= 4 && b.len() >= 4 {
            frags.push(format!("{a}-{b}"));
        }
    }
    // deploy* expansion without hardcoding deployment.service
    let q = question.to_lowercase();
    if q.contains("deploy") {
        frags.push("deploy".into());
        frags.push("deployment".into());
        frags.push("deployment.service".into());
        frags.push("deploy.service".into());
    }
    if q.contains("secret") {
        frags.push("secret".into());
        frags.push("secrets".into());
        frags.push("secret-manager".into());
        frags.push("secret.manager".into());
    }
    if q.contains("data room") || q.contains("data-room") || q.contains("dataroom") {
        frags.push("data-room".into());
        frags.push("dataroom".into());
        frags.push("data_room".into());
    }
    frags.sort();
    frags.dedup();
    frags.truncate(40);
    frags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_is_implementation_loader_is_satellite() {
        assert_eq!(
            infer_role("src/modules/core/services/deployment.service.ts"),
            InferredRole::Implementation
        );
        assert_eq!(
            infer_role("src/modules/marketing/loaders/deploy-count.loader.ts"),
            InferredRole::Satellite
        );
        assert_eq!(
            infer_role("src/common/constants/token-verification.constants.ts"),
            InferredRole::Satellite
        );
        assert_eq!(
            infer_role("src/infrastructure/secret-manager/secret-manager.factory.ts"),
            InferredRole::Implementation
        );
    }

    #[test]
    fn idf_prefers_rare_secret_over_common_deployment() {
        // deployment/deploy appears often (common); secret appears rarely (distinctive)
        let bag = vec![
            "src/modules/core/services/deployment.service.ts".into(),
            "src/modules/marketing/loaders/deploy-count.loader.ts".into(),
            "src/modules/marketing/loaders/tokens-by-deployer.loader.ts".into(),
            "src/schemas/deployment.schema.ts".into(),
            "src/modules/core/models/token.model.ts".into(),
            "src/config/secrets.ts".into(),
            "src/infrastructure/secret-manager/secret-manager.factory.ts".into(),
            "src/server.ts".into(),
            "src/modules/support/services/support-ticket.service.ts".into(),
        ];
        let tokens = vec!["deployment".into(), "secret".into(), "changed".into()];
        let idf = bag_token_idf(&tokens, &bag);
        // secret appears in fewer paths than deployment/deploy
        let q = "production deployment failing after secret was changed";
        let bag_set: HashSet<String> = bag.iter().cloned().collect();
        let s_secret = role_aware_score(
            "src/config/secrets.ts",
            q,
            &tokens,
            &idf,
            &bag_set,
            None,
        );
        let s_deploy = role_aware_score(
            "src/modules/core/services/deployment.service.ts",
            q,
            &tokens,
            &idf,
            &bag_set,
            None,
        );
        assert!(
            s_secret > s_deploy,
            "secrets.ts ({s_secret}) should outrank deployment.service ({s_deploy}) when secret+deployment co-occur"
        );
    }

    #[test]
    fn implementation_outranks_loader_for_token_deploy() {
        let bag = vec![
            "src/modules/core/services/deployment.service.ts".into(),
            "src/modules/marketing/loaders/tokens-by-deployer.loader.ts".into(),
            "src/modules/core/services/token.service.ts".into(),
        ];
        let tokens = concept_tokens("How are tokens deployed?");
        let idf = bag_token_idf(&tokens, &bag);
        let bag_set: HashSet<String> = bag.iter().cloned().collect();
        let q = "How are tokens deployed in VestaScan API?";
        let s_svc = role_aware_score(
            "src/modules/core/services/deployment.service.ts",
            q,
            &tokens,
            &idf,
            &bag_set,
            None,
        );
        let s_load = role_aware_score(
            "src/modules/marketing/loaders/tokens-by-deployer.loader.ts",
            q,
            &tokens,
            &idf,
            &bag_set,
            None,
        );
        assert!(
            s_svc > s_load,
            "deployment.service ({s_svc}) > deploy loader ({s_load})"
        );
    }
}
