//! C5.1-R — Retrieval recall expansion.
//!
//! Deterministic seeds/anchors so C5.1 ranking receives the right bag.
//! Ranking is not the bottleneck when gold files never enter candidates.
//!
//! Failure classes addressed:
//! 1. Issue-anchored: issue → PR → merge commit files → message (#N) commits
//! 2. Error-clue / domain: high-signal path fragments for order/auth/redis
//! 3. Flow: multi-stage path fragments when question is flow-shaped
//! 4. Neighborhood: structural neighbors of forced seeds (via investigate)

use anyhow::Result;
use atlas_storage::Store;

#[derive(Debug, Default, Clone)]
pub struct RetrievalExpansion {
    pub extra_anchors: Vec<String>,
    pub seed_files: Vec<String>,
    pub notes: Vec<String>,
}

/// Expand retrieval options from question text and optional explicit issue number.
pub fn expand_retrieval(
    question: &str,
    issue_number: Option<i64>,
    repo_path: &str,
    store: &Store,
) -> Result<RetrievalExpansion> {
    let mut out = RetrievalExpansion::default();
    let q = question.to_lowercase();

    // ── 1) Issue numbers from text + explicit ───────────────────────────────
    let mut issues: Vec<i64> = detect_issue_numbers(question);
    if let Some(n) = issue_number {
        if !issues.contains(&n) {
            issues.push(n);
        }
    }
    for n in issues.iter().take(4) {
        let (anchors, seeds, notes) = seed_from_issue(*n, repo_path, store)?;
        for a in anchors {
            push_unique(&mut out.extra_anchors, a);
        }
        for s in seeds {
            push_unique(&mut out.seed_files, s);
        }
        out.notes.extend(notes);
    }

    // ── 2) Concept-derived path fragments (repo-agnostic; C5.1-E) ───────────
    // Prefer generated fragments from question tokens over hardcoded service names.
    let domain = detect_domain(&q);
    let flow = is_flow_question(&q);
    let mut fragments = crate::role_aware::concept_search_fragments(question);
    // Keep a small domain residual only for RWATP sacred order↔redis disambiguation
    // and flow multi-stage — not VestaScan-specific paths.
    for f in domain_path_fragments(domain, flow, &q) {
        if !fragments.iter().any(|x| x == &f) {
            fragments.push(f);
        }
    }
    for frag in &fragments {
        push_unique(&mut out.extra_anchors, frag.clone());
        // Resolve to concrete paths via file_path search
        if let Ok(matches) = store.search_anchor(frag, repo_path) {
            for m in matches.into_iter().filter(|m| m.source_type == "file_path").take(8) {
                if looks_like_source_path(&m.source_id) {
                    push_unique(&mut out.seed_files, m.source_id);
                }
            }
        }
    }
    if !fragments.is_empty() {
        out.notes.push(format!(
            "c5.1e_concept_fragments domain={domain:?} flow={flow} n={}",
            fragments.len()
        ));
    }

    // Cap seeds so we don't drown ranking
    out.seed_files.truncate(24);
    out.extra_anchors.truncate(16);
    Ok(out)
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.iter().any(|x| x == &s) {
        v.push(s);
    }
}

fn looks_like_source_path(p: &str) -> bool {
    if p.starts_with("UNRESOLVED:") {
        return false;
    }
    let lower = p.to_lowercase();
    if lower.contains("node_modules/") || lower.contains("/dist/") || lower.contains("/build/") {
        return false;
    }
    lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".js")
        || lower.ends_with(".rs")
        || lower.ends_with(".py")
}

/// Detect issue numbers: #19, issue #19, issue 19.
///
/// Iterates by Unicode scalar values so multi-byte punctuation (e.g. em-dash
/// U+2014 in engineer questions) never panics on mid-char byte indices.
pub fn detect_issue_numbers(text: &str) -> Vec<i64> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < text.len() {
        // Guaranteed char boundary: i advances by char width or #NNN span.
        let rest = &text[i..];
        let lower = rest.to_lowercase();
        if lower.starts_with("issue") {
            // "issue" is ASCII; skip 5 bytes then trim_start is char-safe.
            let after = rest[5..].trim_start();
            let after = after.strip_prefix('#').unwrap_or(after);
            if let Some(n) = parse_leading_int(after) {
                if n > 0 && !out.contains(&n) {
                    out.push(n);
                }
            }
        }
        if bytes[i] == b'#' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if let Ok(n) = std::str::from_utf8(&bytes[i + 1..j])
                .unwrap_or("")
                .parse::<i64>()
            {
                if n > 0 && !out.contains(&n) {
                    out.push(n);
                }
            }
            i = j;
            continue;
        }
        // Advance one Unicode scalar value (not one byte).
        i += text[i..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
    }
    out
}

fn parse_leading_int(s: &str) -> Option<i64> {
    let t = s.trim_start();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn seed_from_issue(
    n: i64,
    repo_path: &str,
    store: &Store,
) -> Result<(Vec<String>, Vec<String>, Vec<String>)> {
    let mut anchors = Vec::new();
    let mut seeds = Vec::new();
    let mut notes = Vec::new();

    let (title, body) = match store.get_issue(n, repo_path)? {
        Some(tb) => tb,
        None => {
            notes.push(format!("c5.1r_issue#{n}_not_in_db"));
            // Still try commit message refs
            collect_files_for_github_number(n, repo_path, store, &mut seeds)?;
            return Ok((anchors, seeds, notes));
        }
    };

    anchors.push(format!("issue#{n}"));
    for a in crate::extract_issue_anchors(&title, &body) {
        push_unique(&mut anchors, a);
    }
    notes.push(format!("c5.1r_issue_anchored #{n}"));

    // Linked PRs (closing references)
    let prs = store.prs_closing_issue(n, repo_path).unwrap_or_default();
    for prn in prs.iter().take(8) {
        push_unique(&mut anchors, format!("pr#{prn}"));
        if let Ok(Some(pr)) = store.pr_by_number(*prn, repo_path) {
            if let Some(sha) = pr.merge_commit_sha {
                if let Ok(files) = store.commit_changed_files(&sha, repo_path) {
                    for f in files.into_iter().take(30) {
                        if looks_like_source_path(&f) {
                            push_unique(&mut seeds, f);
                        }
                    }
                }
            }
        }
    }
    if !prs.is_empty() {
        notes.push(format!("c5.1r_issue_prs {:?}", prs));
    }

    // Commits / PRs mentioning #N in message or title
    collect_files_for_github_number(n, repo_path, store, &mut seeds)?;

    // Compound path fragments from title (share class → share-class)
    for frag in compound_title_fragments(&title) {
        push_unique(&mut anchors, frag.clone());
        if let Ok(matches) = store.search_anchor(&frag, repo_path) {
            for m in matches
                .into_iter()
                .filter(|m| m.source_type == "file_path")
                .take(10)
            {
                if looks_like_source_path(&m.source_id) {
                    push_unique(&mut seeds, m.source_id);
                }
            }
        }
    }

    // Path fragments from issue title tokens (Redis, Timeout, Rate, …)
    for tok in crate::extract_issue_anchors(&title, "") {
        if tok.len() >= 4 {
            if let Ok(matches) = store.search_anchor(&tok, repo_path) {
                for m in matches
                    .into_iter()
                    .filter(|m| m.source_type == "file_path")
                    .take(6)
                {
                    if looks_like_source_path(&m.source_id) {
                        push_unique(&mut seeds, m.source_id);
                    }
                }
            }
        }
    }

    // Prefer seeds that match issue domain + title tokens over merge noise.
    let domain = detect_domain(&format!("{title} {body}").to_lowercase());
    seeds = prefer_domain_paths(seeds, domain);
    seeds = rank_seeds_by_title_overlap(seeds, &title);

    Ok((anchors, seeds, notes))
}

/// Put title-overlapping paths first so they survive the core candidate cap.
fn rank_seeds_by_title_overlap(mut seeds: Vec<String>, title: &str) -> Vec<String> {
    let title_l = title.to_lowercase();
    let mut tokens: Vec<String> = title_l
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .map(|s| s.to_string())
        .filter(|s| s.len() >= 4)
        .collect();
    // Compound cues
    if title_l.contains("share class") || title_l.contains("share-class") {
        tokens.push("share-class".into());
        tokens.push("shareclass".into());
    }
    seeds.sort_by(|a, b| {
        seed_title_score(b, &tokens)
            .cmp(&seed_title_score(a, &tokens))
            .then_with(|| a.cmp(b))
    });
    seeds
}

fn seed_title_score(path: &str, tokens: &[String]) -> i32 {
    let p = path.to_lowercase().replace('_', "-");
    let mut s = 0i32;
    for t in tokens {
        let tt = t.replace('_', "-");
        if p.contains(&tt) {
            s += 10;
        }
        // share + class both present
        if tt == "share" && p.contains("share") {
            s += 3;
        }
        if tt == "class" && p.contains("class") {
            s += 3;
        }
    }
    if p.contains("share-class") {
        s += 40;
    }
    if p.contains("/services/") || p.contains("/models/") || p.contains("resolvers") {
        s += 5;
    }
    if p.contains("/contracts/") || p.contains("common/commands/") {
        s -= 20;
    }
    s
}

/// Multi-word title cues → path-friendly search fragments (generic joins, not service names).
fn compound_title_fragments(title: &str) -> Vec<String> {
    // Reuse concept fragment generator (hyphenated bigrams, token.service, …)
    crate::role_aware::concept_search_fragments(title)
}

fn prefer_domain_paths(seeds: Vec<String>, domain: Domain) -> Vec<String> {
    let mut preferred = Vec::new();
    let mut other = Vec::new();
    for s in seeds {
        if path_matches_domain(&s, domain) {
            preferred.push(s);
        } else {
            other.push(s);
        }
    }
    preferred.extend(other);
    preferred
}

fn path_matches_domain(path: &str, domain: Domain) -> bool {
    let p = path.to_lowercase();
    match domain {
        Domain::Order => {
            p.contains("order") || p.contains("payment") || p.contains("settlement")
        }
        Domain::Auth => {
            p.contains("auth") || p.contains("identity") || p.contains("permission")
        }
        Domain::Redis => {
            p.contains("redis")
                || p.contains("rate-limit")
                || p.contains("rate_limit")
                || p.contains("/caching/")
        }
        Domain::Generic => {
            // Prefer module services/models over contracts noise for feature issues
            p.contains("src/modules/") && !p.contains("/contracts/")
        }
    }
}

fn collect_files_for_github_number(
    n: i64,
    repo_path: &str,
    store: &Store,
    seeds: &mut Vec<String>,
) -> Result<()> {
    for pattern in [format!("#{n}"), format!("({n})")] {
        if let Ok(matches) = store.search_anchor(&pattern, repo_path) {
            for m in matches {
                if m.source_type == "commit_message" {
                    if let Ok(files) = store.commit_changed_files(&m.source_id, repo_path) {
                        for f in files.into_iter().take(40) {
                            if looks_like_source_path(&f) {
                                push_unique(seeds, f);
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Domain {
    Order,
    Auth,
    Redis,
    Generic,
}

fn detect_domain(q: &str) -> Domain {
    let redis = ["redis", "rate-limit", "rate limit", "cache timeout", "grey failure"];
    let auth = [
        "auth",
        "login",
        "password",
        "session",
        "sign-in",
        "signin",
        "firebase",
        "siwe",
        "authentication",
    ];
    let order = [
        "order",
        "orders",
        "checkout",
        "payment",
        "settlement",
        "quoteorder",
        "fulfillment",
    ];
    let has_redis = redis.iter().any(|t| q.contains(t));
    let has_auth = auth.iter().any(|t| q.contains(t));
    let has_order = order.iter().any(|t| q.contains(t));

    // Issue #19 style: redis is primary when named even if "timeout" co-occurs with orders
    if has_redis && !has_order {
        return Domain::Redis;
    }
    if has_redis && (q.contains("configure redis") || q.contains("redis command")) {
        return Domain::Redis;
    }
    if has_auth && !has_order {
        return Domain::Auth;
    }
    if has_order {
        return Domain::Order;
    }
    if has_redis {
        return Domain::Redis;
    }
    if has_auth {
        return Domain::Auth;
    }
    Domain::Generic
}

fn is_flow_question(q: &str) -> bool {
    q.contains("flow")
        || q.contains("how does")
        || q.contains("what happens")
        || q.contains("explain")
        || q.contains("end to end")
        || q.contains("through the")
        || q.contains("pipeline")
        || q.contains("lifecycle")
}

/// High-signal path fragments (not full paths) — resolved via search_anchor.
fn domain_path_fragments(domain: Domain, flow: bool, q: &str) -> Vec<String> {
    let mut v = Vec::new();
    match domain {
        Domain::Order => {
            v.extend([
                "order.service",
                "order.model",
                "order.resolvers",
                "order-history",
                "expire-orders",
            ]);
            if flow || q.contains("creat") || q.contains("process") {
                v.extend([
                    "order-eligibility",
                    "pricing-engine",
                    "payment.service",
                    "signing.service",
                    "settlement.service",
                    "order-fulfillment",
                    "order.validation",
                ]);
            }
            if q.contains("error") || q.contains("fail") {
                v.extend(["errorCodes", "createError", "order.validation"]);
            }
        }
        Domain::Auth => {
            v.extend([
                "AuthService",
                "FirebaseAuth",
                "auth-resolver",
                "user.service",
                "user.model",
                "identity.resolvers",
                "InvestorAuth",
                "permission.service",
            ]);
        }
        Domain::Redis => {
            v.extend([
                "redis-rate-limiter",
                "redis-cache",
                "redis/connection",
                "rate-limiter.factory",
                "cache.manager",
                "cache-keys",
            ]);
        }
        Domain::Generic => {}
    }
    v.into_iter().map(String::from).collect()
}

/// Merge expansion into reasoning packet options (anchors + seeds).
pub fn apply_expansion(
    question: &str,
    anchors: &mut Vec<String>,
    seed_files: &mut Vec<String>,
    limitations: &mut Vec<String>,
    issue_number: Option<i64>,
    repo_path: &str,
    store: &Store,
) -> Result<()> {
    let exp = expand_retrieval(question, issue_number, repo_path, store)?;
    for a in exp.extra_anchors {
        if !anchors.iter().any(|x| x == &a) {
            anchors.push(a);
        }
    }
    for s in exp.seed_files {
        // File-path anchors force investigate FilePath matches
        if !anchors.iter().any(|x| x == &s) {
            anchors.push(s.clone());
        }
        if !seed_files.iter().any(|x| x == &s) {
            seed_files.push(s);
        }
    }
    for n in exp.notes {
        limitations.push(n);
    }
    // Cap anchors to keep investigate bounded
    if anchors.len() > 24 {
        anchors.truncate(24);
    }
    if seed_files.len() > 24 {
        seed_files.truncate(24);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_issue_hash_and_words() {
        assert_eq!(detect_issue_numbers("fix issue #19 please"), vec![19]);
        assert_eq!(detect_issue_numbers("see issue 12 and #14"), vec![12, 14]);
        assert!(detect_issue_numbers("orders timeout").is_empty());
        // Multi-byte punctuation must not panic (cross-repo eval jj-op-log).
        assert!(detect_issue_numbers(
            "How does the operation log work — the immutable record of mutations?"
        )
        .is_empty());
        assert_eq!(
            detect_issue_numbers("see #42 — and also issue #7"),
            vec![42, 7]
        );
    }

    #[test]
    fn domain_order_vs_redis() {
        assert_eq!(
            detect_domain("orders timeout under concurrent activity"),
            Domain::Order
        );
        assert_eq!(
            detect_domain("configure redis command timeout to prevent grey failures"),
            Domain::Redis
        );
        assert_eq!(
            detect_domain("users cannot log in authentication fails"),
            Domain::Auth
        );
    }

    #[test]
    fn flow_question_detection() {
        assert!(is_flow_question("explain the order flow"));
        assert!(is_flow_question("what happens when an order is created"));
        assert!(!is_flow_question("orders timeout"));
    }
}
