//! C4-ER — Evidence Reasoning Engine.
//!
//! Ranking, temporal supersession, and hard claim entailment.
//! Existence of a reference is NOT support for a causal claim.
//!
//! Sacred regression: "orders timeout" must never mark Redis as SUPPORTED cause.

use atlas_ir::{
    ChronologyEvent, ClaimStatus, EvidenceDimensions, EvidencePacket, EvidenceRef, Hypothesis,
    ProposedClaim, RankedEvidenceItem, SupersessionNote,
};

// ─── Causal / entailment language ───────────────────────────────────────────

const CAUSAL_MARKERS: &[&str] = &[
    "cause",
    "causes",
    "caused",
    "because",
    "due to",
    "leads to",
    "leading to",
    "results in",
    "resulting in",
    "responsible for",
    "explains",
    "explain the timeout",
    "timeout is related",
    "related to the",
    "is related to",
    "prevent",
    "prevents",
    "root cause",
    "is configured to prevent",
    "during order processing",
    "affect order",
    "affects order",
    "order processing",
];

/// Domains that must not be casually linked without structural co-evidence.
const CROSS_DOMAIN_PAIRS: &[(&[&str], &[&str])] = &[
    (
        &["order", "orders", "payment", "checkout"],
        &["redis", "rate-limit", "rate_limit", "ratelimit", "cache", "otel", "opentelemetry"],
    ),
    (
        &["order", "orders"],
        &["image-processor", "smtp", "email"],
    ),
];

pub fn statement_is_causal(statement: &str) -> bool {
    let s = statement.to_lowercase();
    CAUSAL_MARKERS.iter().any(|m| s.contains(m))
        || s.contains("timeout") && (s.contains("redis") || s.contains("rate"))
}

pub fn claim_kind_is_causal(kind: &str) -> bool {
    matches!(
        kind.to_lowercase().as_str(),
        "causal" | "intent" | "root_cause" | "diagnosis"
    )
}

/// Extract lightweight subject tokens from question for relevance scoring.
pub fn subject_tokens(question: &str) -> Vec<String> {
    question
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && c != '/')
        .map(|t| t.to_lowercase())
        .filter(|t| t.len() >= 3)
        .filter(|t| {
            ![
                "the", "and", "for", "with", "from", "that", "this", "are", "was",
                "investigate", "issue", "error", "about", "under", "when", "what",
                "why", "how", "need", "modify", "sometimes", "occasionally",
            ]
            .contains(&t.as_str())
        })
        .take(16)
        .collect()
}

fn token_matches_text(token: &str, text: &str) -> bool {
    if text.contains(token) {
        return true;
    }
    if token.len() > 4 && token.ends_with('s') {
        let stem = &token[..token.len() - 1];
        if text.contains(stem) {
            return true;
        }
    }
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

fn path_relevance(path: &str, tokens: &[String]) -> f32 {
    let p = path.to_lowercase();
    if tokens.is_empty() {
        return 0.3;
    }
    let hits = tokens
        .iter()
        .filter(|t| token_matches_text(t, &p))
        .count();
    if hits == 0 {
        0.15
    } else {
        (hits as f32 / tokens.len() as f32).clamp(0.35, 1.0)
    }
}

fn text_relevance(text: &str, tokens: &[String]) -> f32 {
    let p = text.to_lowercase();
    if tokens.is_empty() {
        return 0.3;
    }
    let hits = tokens
        .iter()
        .filter(|t| token_matches_text(t, &p))
        .count();
    if hits == 0 {
        0.1
    } else {
        (0.3 + 0.7 * hits as f32 / tokens.len() as f32).min(1.0)
    }
}

// ─── Ranking ────────────────────────────────────────────────────────────────

/// Build ranked evidence list from a packet (candidates + chronology + docs).
///
/// When `pagerank_by_file` is provided (C5.1), file weights blend question
/// relevance with personalized structural PageRank. Ranking selects *where to
/// look*; C4 still decides claim support.
pub fn rank_evidence(packet: &EvidencePacket) -> Vec<RankedEvidenceItem> {
    rank_evidence_with_pagerank(packet, None)
}

pub fn rank_evidence_with_pagerank(
    packet: &EvidencePacket,
    pagerank_by_file: Option<&std::collections::HashMap<String, f32>>,
) -> Vec<RankedEvidenceItem> {
    let tokens = subject_tokens(&packet.question);
    let mut items: Vec<RankedEvidenceItem> = Vec::new();

    // Core + supporting files
    let file_candidates: Vec<_> = packet
        .investigation
        .core_candidates
        .iter()
        .enumerate()
        .map(|(i, c)| (i, c, true))
        .chain(
            packet
                .investigation
                .supporting_artifacts
                .iter()
                .enumerate()
                .map(|(i, c)| (i + 100, c, false)),
        )
        .collect();

    for (i, c, is_core) in file_candidates {
        let rel = path_relevance(&c.file, &tokens);
        let structural = if packet
            .investigation
            .observed_structure
            .iter()
            .any(|o| o.file == c.file || o.outgoing.iter().any(|e| e.file == c.file))
        {
            0.8
        } else {
            0.35
        };
        let pr = pagerank_by_file
            .and_then(|m| m.get(&c.file).copied())
            .unwrap_or(0.0);
        // Path class + compound subject stems (C5.1-S/E): production + exact op_store
        // must beat demos/assets and generic *store* flood under PageRank.
        let class_mult =
            crate::path_class::class_rank_multiplier(crate::path_class::classify_path(&c.file), &packet.question);
        let stem_b = crate::subject_resolve::subject_stem_boost(&c.file, &packet.question);
        let stem_n = (stem_b / 24.0).clamp(0.0, 1.0);
        let prior = c.score.total.clamp(0.0, 1.0);
        // Blend: subject stem + class + lexical prior + moderate PageRank (not PR-only).
        let weight = if pr > 0.0 {
            (0.22 * rel
                + 0.12 * structural
                + 0.28 * pr
                + 0.18 * stem_n
                + 0.12 * prior
                + 0.08 * (if is_core { 1.0 } else { 0.4 }))
                * class_mult.clamp(0.15, 1.5)
        } else {
            (0.40 * rel
                + 0.20 * structural
                + 0.20 * stem_n
                + 0.12 * prior
                + 0.08 * (1.0 - (i as f32 * 0.05).min(0.5)))
                * class_mult.clamp(0.15, 1.5)
        };
        let mut notes = vec![if is_core {
            format!("core_candidate rank_index={i}")
        } else {
            format!("supporting rank_index={i}")
        }];
        if pr > 0.0 {
            notes.push(format!("c5.1_pagerank={pr:.3}"));
        }
        if stem_b > 0.0 {
            notes.push(format!("c5.1s_stem_boost={stem_b:.1}"));
        }
        if class_mult < 0.9 {
            notes.push(format!("path_class_mult={class_mult:.2}"));
        }
        if rel < 0.35 {
            notes.push("low subject_relevance to question tokens".into());
        }
        items.push(RankedEvidenceItem {
            rank: 0,
            ref_: EvidenceRef {
                kind: "file".into(),
                id: c.file.clone(),
                summary: format!("candidate score≈{:.2}", c.score.total),
                timestamp: None,
            },
            event_semantics: "implementation".into(),
            dimensions: EvidenceDimensions {
                subject_relevance: rel,
                temporal_recency: 0.0,
                structural_connectivity: if pr > 0.0 { pr.max(structural) } else { structural },
                historical_cochange: 0.0,
                corroboration: 0.4,
                provenance_note: if pr > 0.0 {
                    "C5.1 personalized structural PageRank + investigate".into()
                } else {
                    "files + investigate ranking".into()
                },
            },
            weight,
            ranking_notes: notes,
        });
    }

    // Chronology events
    let max_ts = packet
        .chronology
        .iter()
        .map(|e| e.timestamp)
        .max()
        .unwrap_or(0)
        .max(1);
    let min_ts = packet
        .chronology
        .iter()
        .map(|e| e.timestamp)
        .filter(|t| *t > 0)
        .min()
        .unwrap_or(0);
    for ev in &packet.chronology {
        let rel = text_relevance(&format!("{} {}", ev.id, ev.summary), &tokens);
        let recency = if max_ts > min_ts && ev.timestamp > 0 {
            (ev.timestamp - min_ts) as f32 / (max_ts - min_ts) as f32
        } else {
            0.0
        };
        // Intent evidence is valuable but must not outrank current implementation by recency alone
        let semantics_boost = match ev.role.as_str() {
            "implementation" => 0.15,
            "intent" => 0.05,
            _ => 0.0,
        };
        // Chronology is context, not the primary localization signal (C5.1).
        let weight = 0.35 * rel + 0.20 * recency + 0.15 * semantics_boost + 0.10 * 0.5;
        let mut notes = vec![format!("chronology role={}", ev.role)];
        if rel < 0.25 {
            notes.push("weak token overlap with question — demoted".into());
        }
        if ev.role == "intent" {
            notes.push(
                "intent evidence describes desire/design, not necessarily current code".into(),
            );
        }
        items.push(RankedEvidenceItem {
            rank: 0,
            ref_: EvidenceRef {
                kind: ev.kind.clone(),
                id: ev.id.clone(),
                summary: ev.summary.clone(),
                timestamp: Some(ev.timestamp),
            },
            event_semantics: ev.role.clone(),
            dimensions: EvidenceDimensions {
                subject_relevance: rel,
                temporal_recency: recency,
                structural_connectivity: 0.0,
                historical_cochange: 0.0,
                corroboration: if ev.role == "implementation" { 0.5 } else { 0.3 },
                provenance_note: format!("chronology:{}", ev.role),
            },
            weight,
            ranking_notes: notes,
        });
    }

    // Documentary
    for d in &packet.investigation.documentary {
        let text = format!("{} {} {}", d.kind, d.number, d.title);
        let rel = text_relevance(&text, &tokens);
        let weight = 0.55 * rel + 0.20 * 0.3 + 0.25 * 0.4;
        items.push(RankedEvidenceItem {
            rank: 0,
            ref_: EvidenceRef {
                kind: d.kind.clone(),
                id: format!("{}#{}", d.kind, d.number),
                summary: d.title.clone(),
                timestamp: None,
            },
            event_semantics: "intent".into(),
            dimensions: EvidenceDimensions {
                subject_relevance: rel,
                temporal_recency: 0.0,
                structural_connectivity: 0.0,
                historical_cochange: 0.0,
                corroboration: 0.35,
                provenance_note: "documentary title/body match".into(),
            },
            weight,
            ranking_notes: vec![
                "documentary is intent-class unless linked to implementation commits".into(),
            ],
        });
    }

    items.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.ref_.id.cmp(&b.ref_.id))
    });
    // Drop very weak chronology noise when we have strong candidates
    let has_strong_file = items
        .iter()
        .any(|i| i.ref_.kind == "file" && i.weight >= 0.45);
    if has_strong_file {
        items.retain(|i| {
            if i.event_semantics == "intent" && i.dimensions.subject_relevance < 0.28 {
                false
            } else {
                true
            }
        });
    }
    items.truncate(30);
    for (i, it) in items.iter_mut().enumerate() {
        it.rank = (i + 1) as u32;
    }
    items
}

// ─── Temporal supersession ──────────────────────────────────────────────────

/// Detect supersession among chronology events sharing subject tokens.
pub fn compute_supersession(chronology: &[ChronologyEvent], question: &str) -> Vec<SupersessionNote> {
    let tokens = subject_tokens(question);
    let mut notes = Vec::new();
    let relevant: Vec<&ChronologyEvent> = chronology
        .iter()
        .filter(|e| {
            let rel = text_relevance(&format!("{} {}", e.id, e.summary), &tokens);
            rel >= 0.28 || e.role == "implementation"
        })
        .collect();

    for i in 0..relevant.len() {
        for j in (i + 1)..relevant.len() {
            let a = relevant[i];
            let b = relevant[j];
            if a.timestamp == 0 || b.timestamp == 0 {
                continue;
            }
            if b.timestamp <= a.timestamp {
                continue;
            }
            // later implementation supersedes earlier implementation on same topic
            if a.role == "implementation"
                && b.role == "implementation"
                && share_topic(&a.summary, &b.summary, &tokens)
            {
                notes.push(SupersessionNote {
                    earlier_id: a.id.clone(),
                    later_id: b.id.clone(),
                    relationship: "implementation_supersedes_implementation".into(),
                    note: format!(
                        "Later implementation {} is more recent than {}; older commit may not describe current behavior.",
                        b.id, a.id
                    ),
                });
            }
            // later implementation supersedes earlier intent on same topic
            if a.role == "intent"
                && b.role == "implementation"
                && share_topic(&a.summary, &b.summary, &tokens)
            {
                notes.push(SupersessionNote {
                    earlier_id: a.id.clone(),
                    later_id: b.id.clone(),
                    relationship: "implementation_may_supersede_intent".into(),
                    note: format!(
                        "Implementation {} postdates intent {}; intent remains useful for original desire but does not automatically describe current code.",
                        b.id, a.id
                    ),
                });
            }
            // later intent does NOT supersede earlier implementation by default
            if a.role == "implementation" && b.role == "intent" {
                notes.push(SupersessionNote {
                    earlier_id: a.id.clone(),
                    later_id: b.id.clone(),
                    relationship: "intent_does_not_override_implementation".into(),
                    note: format!(
                        "Later intent {} does not automatically override earlier implementation {}; desired change ≠ deployed behavior.",
                        b.id, a.id
                    ),
                });
            }
        }
    }
    notes.truncate(20);
    notes
}

fn share_topic(a: &str, b: &str, tokens: &[String]) -> bool {
    let al = a.to_lowercase();
    let bl = b.to_lowercase();
    if tokens.iter().any(|t| al.contains(t) && bl.contains(t)) {
        return true;
    }
    // crude: same path fragment
    for part in al.split(|c: char| !c.is_alphanumeric()) {
        if part.len() >= 5 && bl.contains(part) {
            return true;
        }
    }
    false
}

// ─── Hard claim verification (entailment) ───────────────────────────────────

/// Policy strings exposed on the packet for transparency.
pub fn verification_policy() -> Vec<String> {
    vec![
        "Existence of an evidence ref is necessary but not sufficient for SUPPORTED.".into(),
        "Causal claims (cause/because/related-to/timeout-is/…) default to PLAUSIBLE max unless multi-source same-subject structural+historical support.".into(),
        "Cross-domain causal links (e.g. order↔redis) require structural co-evidence; otherwise PLAUSIBLE/UNRESOLVED, never SUPPORTED.".into(),
        "Intent evidence cannot alone SUPPORT claims about current runtime behavior.".into(),
        "Implementation evidence is preferred for current-behavior claims; intent remains historical context.".into(),
    ]
}

pub fn hard_verify_claim(claim: &ProposedClaim, packet: &EvidencePacket) -> ClaimStatus {
    // 1) Empty refs
    if claim.evidence_refs.is_empty() {
        return ClaimStatus::Unresolved;
    }

    // 2) Resolve refs
    let mut ok = 0usize;
    let mut bad = 0usize;
    for r in &claim.evidence_refs {
        if crate::reasoning::evidence_resolves_pub(r, packet) {
            ok += 1;
        } else {
            bad += 1;
        }
    }
    if bad > 0 && ok == 0 {
        return ClaimStatus::Contradicted;
    }

    let causal = claim_kind_is_causal(&claim.kind) || statement_is_causal(&claim.statement);
    let tokens = subject_tokens(&packet.question);
    let claim_blob = format!(
        "{} {} {}",
        claim.subject,
        claim.statement,
        claim
            .evidence_refs
            .iter()
            .map(|r| format!("{} {}", r.id, r.summary))
            .collect::<Vec<_>>()
            .join(" ")
    )
    .to_lowercase();

    // 3) Cross-domain causal ban → never SUPPORTED
    if causal && is_cross_domain_causal(&packet.question, &claim_blob) {
        return if ok > 0 {
            ClaimStatus::Plausible
        } else {
            ClaimStatus::Unresolved
        };
    }

    // 4) Causal requires multi-evidence same-subject support
    if causal {
        let strong = count_strong_same_subject_support(claim, packet, &tokens);
        if strong >= 2 {
            // still not full runtime proof
            return ClaimStatus::Plausible;
        }
        if ok > 0 {
            return ClaimStatus::Plausible;
        }
        return ClaimStatus::Unresolved;
    }

    // 5) Non-causal structural observation with all refs OK → Supported
    if bad > 0 {
        return ClaimStatus::Plausible;
    }
    if ok > 0 {
        // intent-only support for "current behavior" wording
        if statement_is_about_current_behavior(&claim.statement)
            && claim_refs_are_intent_only(claim, packet)
        {
            return ClaimStatus::Plausible;
        }
        return ClaimStatus::Supported;
    }
    ClaimStatus::Unresolved
}

fn statement_is_about_current_behavior(s: &str) -> bool {
    let s = s.to_lowercase();
    s.contains("currently")
        || s.contains("current behavior")
        || s.contains("is configured")
        || s.contains("the code does")
        || s.contains("during order")
}

fn claim_refs_are_intent_only(claim: &ProposedClaim, packet: &EvidencePacket) -> bool {
    claim.evidence_refs.iter().all(|r| {
        let k = r.kind.to_lowercase();
        k == "issue" || k == "pr" || {
            packet
                .ranked_evidence
                .iter()
                .find(|x| x.ref_.id == r.id)
                .map(|x| x.event_semantics == "intent")
                .unwrap_or(false)
        }
    })
}

fn is_cross_domain_causal(question: &str, claim_blob: &str) -> bool {
    let q = question.to_lowercase();
    for (domain_a, domain_b) in CROSS_DOMAIN_PAIRS {
        let q_has_a = domain_a.iter().any(|t| q.contains(t));
        let q_has_b = domain_b.iter().any(|t| q.contains(t));
        let c_has_a = domain_a.iter().any(|t| claim_blob.contains(t));
        let c_has_b = domain_b.iter().any(|t| claim_blob.contains(t));
        // Question is about A, claim blames B (or mixes A+B without structure)
        if q_has_a && !q_has_b && c_has_b {
            return true;
        }
        if c_has_a && c_has_b {
            return true;
        }
    }
    false
}

fn count_strong_same_subject_support(
    claim: &ProposedClaim,
    packet: &EvidencePacket,
    tokens: &[String],
) -> usize {
    let mut n = 0usize;
    for r in &claim.evidence_refs {
        let rel = path_relevance(&r.id, tokens).max(text_relevance(
            &format!("{} {}", r.id, r.summary),
            tokens,
        ));
        if rel < 0.4 {
            continue;
        }
        // structural file in core candidates
        if r.kind == "file"
            && packet
                .investigation
                .core_candidates
                .iter()
                .any(|c| c.file == r.id || r.id.ends_with(&c.file))
        {
            n += 1;
        }
        if r.kind == "commit" {
            n += 1;
        }
        if r.kind == "structural" || r.kind == "structural_edge" {
            n += 1;
        }
    }
    n
}

pub fn hard_verify_claims(claims: &[ProposedClaim], packet: &EvidencePacket) -> Vec<ProposedClaim> {
    claims
        .iter()
        .map(|c| {
            let mut out = c.clone();
            out.status = hard_verify_claim(c, packet);
            // Annotate limitations when demoted from naive support
            if statement_is_causal(&c.statement) || claim_kind_is_causal(&c.kind) {
                if !out.limitations.iter().any(|l| l.contains("Causal claims")) {
                    out.limitations.push(
                        "Causal claims cannot be SUPPORTED by ref existence alone (C4-ER).".into(),
                    );
                }
            }
            out
        })
        .collect()
}

pub fn hard_verify_hypotheses(
    hyps: &[Hypothesis],
    packet: &EvidencePacket,
) -> Vec<Hypothesis> {
    hyps.iter()
        .map(|h| {
            let mut out = h.clone();
            out.claims = hard_verify_claims(&h.claims, packet);
            // Hypothesis status: if statement causal, max plausible
            let causal = statement_is_causal(&h.statement);
            let sup_ok = h
                .supporting
                .iter()
                .filter(|r| crate::reasoning::evidence_resolves_pub(r, packet))
                .count();
            let sup_bad = h.supporting.len().saturating_sub(sup_ok);
            out.supporting = h
                .supporting
                .iter()
                .filter(|r| crate::reasoning::evidence_resolves_pub(r, packet))
                .cloned()
                .collect();
            out.contradicting = h
                .contradicting
                .iter()
                .filter(|r| crate::reasoning::evidence_resolves_pub(r, packet))
                .cloned()
                .collect();

            out.status = if h.supporting.is_empty() && h.claims.is_empty() {
                ClaimStatus::Unresolved
            } else if sup_bad > 0 && sup_ok == 0 {
                ClaimStatus::Contradicted
            } else if causal {
                // sacred: never SUPPORTED for causal hyp from existence
                ClaimStatus::Plausible
            } else if is_cross_domain_causal(
                &packet.question,
                &format!("{} {:?}", h.statement, h.supporting),
            ) {
                ClaimStatus::Plausible
            } else if sup_ok > 0 {
                ClaimStatus::Plausible // still interpretive association
            } else {
                ClaimStatus::Unresolved
            };
            out
        })
        .collect()
}

// ─── Enrich packet ──────────────────────────────────────────────────────────

pub fn enrich_packet(packet: EvidencePacket) -> EvidencePacket {
    enrich_packet_with_store(packet, None)
}

/// C4 + C5.1: supersession, verification policy, and optional personalized structural rank.
pub fn enrich_packet_with_store(
    mut packet: EvidencePacket,
    store: Option<&atlas_storage::Store>,
) -> EvidencePacket {
    let pagerank_map = store.and_then(|s| compute_packet_pagerank(&packet, s));
    packet.ranked_evidence =
        rank_evidence_with_pagerank(&packet, pagerank_map.as_ref());
    packet.supersession = compute_supersession(&packet.chronology, &packet.question);
    packet.verification_policy = verification_policy();
    if pagerank_map.is_some() {
        packet.bounds.push(
            "C5.1 question-personalized structural PageRank applied to file evidence".into(),
        );
    }
    // Filter chronology noise: keep high-ranked event ids + all implementation on top files
    let keep_ids: std::collections::HashSet<String> = packet
        .ranked_evidence
        .iter()
        .take(20)
        .map(|r| r.ref_.id.clone())
        .collect();
    let tokens = subject_tokens(&packet.question);
    packet.chronology.retain(|e| {
        keep_ids.contains(&e.id)
            || e.role == "implementation"
                && text_relevance(&format!("{} {}", e.id, e.summary), &tokens) >= 0.25
            || text_relevance(&format!("{} {}", e.id, e.summary), &tokens) >= 0.45
    });
    // Reorder core candidates to match blended ranked_evidence (not pure PageRank).
    // Pure PR previously buried exact subject stems under high-degree *store* hubs.
    {
        let order: std::collections::HashMap<String, usize> = packet
            .ranked_evidence
            .iter()
            .filter(|r| r.ref_.kind == "file")
            .enumerate()
            .map(|(i, r)| (r.ref_.id.clone(), i))
            .collect();
        if !order.is_empty() {
            packet.investigation.core_candidates.sort_by(|a, b| {
                let sa = order.get(&a.file).copied().unwrap_or(999);
                let sb = order.get(&b.file).copied().unwrap_or(999);
                sa.cmp(&sb).then_with(|| a.file.cmp(&b.file))
            });
        }
    }
    packet.schema_version = packet.schema_version.max(2);
    packet
}

fn compute_packet_pagerank(
    packet: &EvidencePacket,
    store: &atlas_storage::Store,
) -> Option<std::collections::HashMap<String, f32>> {
    use crate::personalized_rank::{
        collect_links_for_files, personalized_file_ranks, PersonalizedRankInput,
    };
    let mut candidates: Vec<String> = packet
        .investigation
        .core_candidates
        .iter()
        .chain(packet.investigation.supporting_artifacts.iter())
        .map(|c| c.file.clone())
        .collect();
    // Include one-hop observed structure targets
    for o in &packet.investigation.observed_structure {
        if !candidates.iter().any(|c| c == &o.file) {
            candidates.push(o.file.clone());
        }
        for e in o.outgoing.iter().chain(o.incoming.iter()) {
            if !e.file.starts_with("UNRESOLVED:") && !candidates.iter().any(|c| c == &e.file) {
                candidates.push(e.file.clone());
            }
        }
    }
    if candidates.is_empty() {
        return None;
    }
    let seeds: Vec<String> = packet
        .investigation
        .core_candidates
        .iter()
        .take(6)
        .map(|c| c.file.clone())
        .collect();
    let mut links = collect_links_for_files(&candidates, &packet.repo_path, store);
    // Also materialize observed_structure edges
    for o in &packet.investigation.observed_structure {
        for e in &o.outgoing {
            if e.file.starts_with("UNRESOLVED:") {
                continue;
            }
            links.push(crate::personalized_rank::StructuralLink {
                from: o.file.clone(),
                to: e.file.clone(),
                kind: e.kind.clone(),
            });
        }
        for e in &o.incoming {
            if e.file.starts_with("UNRESOLVED:") {
                continue;
            }
            links.push(crate::personalized_rank::StructuralLink {
                from: e.file.clone(),
                to: o.file.clone(),
                kind: e.kind.clone(),
            });
        }
    }
    if links.is_empty() && seeds.is_empty() {
        return None;
    }
    let ranks = personalized_file_ranks(&PersonalizedRankInput {
        question: &packet.question,
        seed_files: &seeds,
        candidate_files: &candidates,
        edges: &links,
    });
    if ranks.is_empty() {
        return None;
    }
    Some(ranks.into_iter().map(|r| (r.file, r.score)).collect())
}

// ─── PR linking from commit messages (C4-B) ─────────────────────────────────

/// Parse GitHub-style references `(#123)` or `#123` from a commit message.
pub fn parse_github_numbers(message: &str) -> Vec<i64> {
    let mut out = Vec::new();
    let bytes = message.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
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
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod sacred_tests {
    use super::*;
    use atlas_ir::{EvidencePacket, InvestigationCoverage, InvestigationDocument};

    fn empty_inv() -> InvestigationDocument {
        InvestigationDocument {
            schema_version: 6,
            anchors: vec!["orders".into(), "timeout".into()],
            effective_anchors: vec!["orders".into(), "timeout".into()],
            lexicon_expansions: vec![],
            concept_expansions: vec![],
            core_candidates: vec![atlas_ir::CandidateArtifact {
                file: "src/modules/core/services/order.service.ts".into(),
                role: atlas_ir::ArtifactRole::ProductionSource,
                reasons: vec![],
                score: Default::default(),
            }],
            supporting_artifacts: vec![],
            observed_structure: vec![],
            documentary: vec![atlas_ir::DocumentaryEvidence {
                kind: "issue".into(),
                number: 19,
                title: "Configure Redis Command Timeout to Prevent Grey Failures".into(),
                matched_anchors: vec!["timeout".into()],
                snippets: vec![],
            }],
            historical: vec![],
            unresolved: vec![],
            related_decisions: vec![],
            coverage: InvestigationCoverage {
                git_history: true,
                github_prs: true,
                github_issues: true,
                file_paths: true,
                es_imports: true,
                static_calls: true,
                model_refs: true,
            },
            deleted_candidates: vec![],
            anchor_redirects: vec![],
        }
    }

    fn packet_orders_timeout() -> EvidencePacket {
        let mut p = EvidencePacket {
            schema_version: 2,
            question: "orders timeout".into(),
            repo_path: "/repo".into(),
            git_head: None,
            anchors: vec!["orders".into(), "timeout".into()],
            investigation: empty_inv(),
            chronology: vec![ChronologyEvent {
                timestamp: 1000,
                kind: "issue".into(),
                id: "issue#19".into(),
                summary: "Issue #19: Configure Redis Command Timeout".into(),
                role: "intent".into(),
            }],
            modules_present: vec!["core".into()],
            limitations: vec![],
            bounds: vec![],
            ranked_evidence: vec![],
            supersession: vec![],
            verification_policy: vec![],
        };
        // Make redis file "resolve" by putting it in supporting
        p.investigation.supporting_artifacts.push(atlas_ir::CandidateArtifact {
            file: "src/infrastructure/rate-limiting/implementations/redis-rate-limiter.ts".into(),
            role: atlas_ir::ArtifactRole::ProductionSource,
            reasons: vec![],
            score: Default::default(),
        });
        p = enrich_packet(p);
        p
    }

    #[test]
    fn sacred_orders_timeout_redis_causal_never_supported() {
        let packet = packet_orders_timeout();
        let claim = ProposedClaim {
            id: "c1".into(),
            subject: "src/infrastructure/rate-limiting/implementations/redis-rate-limiter.ts"
                .into(),
            statement: "The Redis command timeout is configured to prevent grey failures during order processing.".into(),
            kind: "structural".into(),
            evidence_refs: vec![
                EvidenceRef {
                    kind: "file".into(),
                    id: "src/infrastructure/rate-limiting/implementations/redis-rate-limiter.ts"
                        .into(),
                    summary: "rate limiting".into(),
                    timestamp: None,
                },
                EvidenceRef {
                    kind: "issue".into(),
                    id: "#19".into(),
                    summary: "Redis timeouts".into(),
                    timestamp: None,
                },
            ],
            method: "static code analysis and issue review".into(),
            temporal_scope: "".into(),
            limitations: vec![],
            status: ClaimStatus::Unresolved,
        };
        let status = hard_verify_claim(&claim, &packet);
        assert_ne!(
            status,
            ClaimStatus::Supported,
            "C4-ER sacred: existence of Redis issue/file must NOT SUPPORT causal order-timeout claim, got {status:?}"
        );
        assert!(
            matches!(status, ClaimStatus::Plausible | ClaimStatus::Unresolved),
            "expected Plausible/Unresolved, got {status:?}"
        );
    }

    #[test]
    fn sacred_hypothesis_redis_never_supported() {
        let packet = packet_orders_timeout();
        let hyps = vec![Hypothesis {
            id: "h1".into(),
            statement: "The timeout issue is related to the Redis command configuration.".into(),
            status: ClaimStatus::Plausible,
            supporting: vec![EvidenceRef {
                kind: "issue".into(),
                id: "#19".into(),
                summary: "Redis".into(),
                timestamp: None,
            }],
            contradicting: vec![],
            claims: vec![],
        }];
        let v = hard_verify_hypotheses(&hyps, &packet);
        assert_ne!(v[0].status, ClaimStatus::Supported);
    }

    /// Sacred: top-ranked candidate existence must NEVER become SUPPORTED.
    /// C5 localizes; C4 verifies. Ranking ≠ entailment.
    #[test]
    fn sacred_ranking_association_never_supported() {
        let packet = packet_orders_timeout();
        let top = packet
            .investigation
            .core_candidates
            .first()
            .map(|c| c.file.clone())
            .unwrap_or_else(|| "src/modules/core/services/order.service.ts".into());
        let hyps = vec![Hypothesis {
            id: "det-1".into(),
            statement: format!(
                "Deterministic retrieval associates this question with `{top}` and its neighborhood."
            ),
            status: ClaimStatus::Supported, // naive emitter mistake — hard_verify must demote
            supporting: vec![EvidenceRef {
                kind: "file".into(),
                id: top,
                summary: "Top-ranked core candidate from anchor investigation".into(),
                timestamp: None,
            }],
            contradicting: vec![],
            claims: vec![],
        }];
        let v = hard_verify_hypotheses(&hyps, &packet);
        assert_ne!(
            v[0].status,
            ClaimStatus::Supported,
            "C4 sacred: ranking association must not be SUPPORTED, got {:?}",
            v[0].status
        );
        assert!(
            matches!(v[0].status, ClaimStatus::Plausible | ClaimStatus::Unresolved),
            "expected Plausible/Unresolved, got {:?}",
            v[0].status
        );
    }

    #[test]
    fn non_causal_file_presence_can_be_supported() {
        let packet = packet_orders_timeout();
        let claim = ProposedClaim {
            id: "c2".into(),
            subject: "src/modules/core/services/order.service.ts".into(),
            statement: "order.service.ts is among the core candidates for this investigation."
                .into(),
            kind: "structural".into(),
            evidence_refs: vec![EvidenceRef {
                kind: "file".into(),
                id: "src/modules/core/services/order.service.ts".into(),
                summary: "candidate".into(),
                timestamp: None,
            }],
            method: "candidate list".into(),
            temporal_scope: "".into(),
            limitations: vec![],
            status: ClaimStatus::Unresolved,
        };
        assert_eq!(hard_verify_claim(&claim, &packet), ClaimStatus::Supported);
    }

    #[test]
    fn parse_github_numbers_from_message() {
        assert_eq!(
            parse_github_numbers("feat(orders): quoteOrder (#134)"),
            vec![134]
        );
        assert_eq!(parse_github_numbers("merge #11 and #16"), vec![11, 16]);
    }

    #[test]
    fn ranking_promotes_order_file_over_unrelated_intent() {
        let packet = packet_orders_timeout();
        let ranked = rank_evidence(&packet);
        let top_files: Vec<_> = ranked
            .iter()
            .filter(|r| r.ref_.kind == "file")
            .map(|r| r.ref_.id.as_str())
            .collect();
        assert!(
            top_files
                .iter()
                .any(|f| f.contains("order.service")),
            "order.service should rank among files: {top_files:?}"
        );
    }
}
