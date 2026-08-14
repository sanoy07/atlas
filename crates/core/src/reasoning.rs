//! Evidence packet assembly, claim verification, and multi-round investigation loop.
//!
//! Deterministic retrieval stays in Atlas.  Optional local AI proposes hypotheses;
//! verification never promotes AI text to repository truth.

use crate::ai_provider::{
    investigation_system_prompt, packet_prompt_summary, ReasoningProvider,
};
use crate::{
    compute_modules, extract_issue_anchors, investigate, InvestigationDocument,
};
use anyhow::Result;
use atlas_ir::{
    ArtifactRole, CandidateArtifact, CandidateReason, ChronologyEvent, ClaimStatus,
    EvidencePacket, EvidenceRef, Hypothesis, InvestigationRound, ProposedClaim,
    ReasoningInvestigationResult, ScoreBreakdown,
};
use atlas_storage::Store;

const PACKET_SCHEMA: u32 = 2;
const RESULT_SCHEMA: u32 = 1;
/// Raised slightly so C5.1-R multi-stage seeds (flow/issue) survive the bag cap.
const MAX_CORE_FILES: usize = 16;
const MAX_SUPPORT_FILES: usize = 8;
const MAX_CHRONOLOGY: usize = 40;
const MAX_ROUNDS: u32 = 3;

fn git_head_short(repo_path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["-C", repo_path, "rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

// ─── Anchor extraction from natural language ────────────────────────────────

const STOP: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "if", "in", "on", "at", "to", "for",
    "of", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should", "may", "might",
    "with", "from", "by", "as", "into", "about", "this", "that", "these", "those",
    "it", "its", "we", "you", "they", "i", "me", "my", "our", "your", "their",
    "when", "where", "what", "which", "who", "how", "why", "not", "no", "so",
    "than", "then", "too", "very", "can", "just", "also", "some", "any", "all",
    "please", "help", "explain", "investigate", "look", "find", "show", "get",
    "make", "need", "seems", "sometimes", "always", "intermittently", "during",
];

/// Extract retrieval anchors from a free-text question (deterministic).
pub fn anchors_from_question(question: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in question.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && c != '/') {
        let t = raw.trim().to_lowercase();
        if t.len() < 3 {
            continue;
        }
        if STOP.contains(&t.as_str()) {
            continue;
        }
        // skip pure numbers unless issue-like handled elsewhere
        if t.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if !out.iter().any(|x| x == &t) {
            out.push(t);
        }
        if out.len() >= 12 {
            break;
        }
    }
    if out.is_empty() {
        // fallback: whole question as single soft anchor won't match — use generic
        out.push("src".into());
    }
    out
}

// ─── Evidence packet ────────────────────────────────────────────────────────

pub struct PacketOptions {
    pub question: String,
    pub anchors: Vec<String>,
    /// Extra file paths to force into the investigation neighborhood.
    pub seed_files: Vec<String>,
    pub max_rounds_hint: u32,
}

/// Build a bounded evidence packet using existing deterministic investigation.
pub fn build_evidence_packet(
    opts: &PacketOptions,
    repo_path: &str,
    store: &Store,
) -> Result<EvidencePacket> {
    let mut anchors = opts.anchors.clone();
    let mut seed_files = opts.seed_files.clone();
    let mut expand_notes: Vec<String> = Vec::new();

    // C5.1-S: free-text → concrete subjects BEFORE keyword expansion.
    // "operation store" → lib/src/op_store.rs (+ structural neighborhood).
    match crate::subject_resolve::resolve_subjects(&opts.question, repo_path, store) {
        Ok(sub) => {
            for s in sub.seed_files {
                if !seed_files.iter().any(|x| x == &s) {
                    seed_files.push(s.clone());
                }
                if !anchors.iter().any(|a| a == &s) {
                    anchors.push(s);
                }
            }
            for a in sub.anchors {
                if !anchors.iter().any(|x| x == &a) {
                    anchors.push(a);
                }
            }
            expand_notes.extend(sub.notes);
        }
        Err(e) => {
            expand_notes.push(format!("c5.1s_error: {e}"));
        }
    }

    // C5.1-R: expand retrieval before investigate so gold-relevant files enter the bag.
    crate::retrieval_expand::apply_expansion(
        &opts.question,
        &mut anchors,
        &mut seed_files,
        &mut expand_notes,
        None,
        repo_path,
        store,
    )?;
    for f in &seed_files {
        if !anchors.iter().any(|a| a == f) {
            anchors.push(f.clone());
        }
    }
    let anchor_refs: Vec<&str> = anchors.iter().map(String::as_str).collect();
    let mut inv = investigate(&anchor_refs, repo_path, store)?;

    // Force C5.1-R seed files to the front of the bag (must survive cap).
    // Previously append-then-truncate dropped seeds when investigate returned ≥12 hubs.
    let mut forced: Vec<CandidateArtifact> = Vec::new();
    for f in &seed_files {
        if forced.iter().any(|c| &c.file == f) {
            continue;
        }
        // Prefer existing candidate entry (keeps scores/reasons) if present
        if let Some(pos) = inv.core_candidates.iter().position(|c| &c.file == f) {
            forced.push(inv.core_candidates.remove(pos));
            continue;
        }
        if let Some(pos) = inv.supporting_artifacts.iter().position(|c| &c.file == f) {
            forced.push(inv.supporting_artifacts.remove(pos));
            continue;
        }
        forced.push(CandidateArtifact {
            file: f.clone(),
            role: ArtifactRole::ProductionSource,
            reasons: vec![CandidateReason::AnchorMatch {
                anchor: f.clone(),
                via: "c5.1r_seed_file".into(),
            }],
            score: ScoreBreakdown::default(),
        });
    }
    // Remaining investigate candidates follow seeds
    let mut rest = std::mem::take(&mut inv.core_candidates);
    rest.extend(std::mem::take(&mut inv.supporting_artifacts));
    // Dedupe rest against forced
    rest.retain(|c| !forced.iter().any(|f| f.file == c.file));
    let mut combined = forced;
    combined.extend(rest);
    // C5.1-L: identifier-weighted lexical re-rank + structure-aware dedup (GrepRAG-style)
    let seed_paths: Vec<String> = seed_files.clone();
    combined = crate::lexical_relevance::rerank_candidates(
        combined,
        &opts.question,
        &anchors,
        &seed_paths,
    );
    // C5.1-E: role-aware primacy (entrypoint/implementation vs satellite) + bag IDF
    combined = crate::role_aware::apply_role_aware_rerank(
        combined,
        &opts.question,
        Some((store, repo_path)),
    );
    // Path/file class soft ranking: production/library over demo/asset/CI/notebook
    combined = apply_path_class_rerank(combined, &opts.question);
    // Re-apply the seed ordering the `forced` pass above established.
    //
    // The three rerank stages re-sort the whole bag. When they cannot separate
    // candidates they all return the same score, and ordering collapses onto
    // their shared `file.cmp()` tiebreak — i.e. alphabetical path order. Seeds
    // then land wherever their path happens to sort, which for a deep module
    // path is past MAX_CORE_FILES, and the truncate below deletes them. Seeds
    // are restored in `seed_files` order (retrieval strength), non-seeds keep
    // their reranked order behind them.
    if !seed_files.is_empty() {
        let (seeds, rest): (Vec<CandidateArtifact>, Vec<CandidateArtifact>) = combined
            .into_iter()
            .partition(|c| seed_files.iter().any(|s| s == &c.file));
        let mut ordered: Vec<CandidateArtifact> = Vec::with_capacity(seeds.len() + rest.len());
        for f in &seed_files {
            if let Some(pos) = seeds.iter().position(|c| &c.file == f) {
                ordered.push(seeds[pos].clone());
            }
        }
        for c in seeds {
            if !ordered.iter().any(|o| o.file == c.file) {
                ordered.push(c);
            }
        }
        ordered.extend(rest);
        combined = ordered;
    }
    inv.core_candidates = combined;
    // Cap candidates for model context (high lexical/seed already first).
    inv.core_candidates.truncate(MAX_CORE_FILES);
    inv.supporting_artifacts.clear();

    let chronology = build_chronology(&inv, repo_path, store)?;
    let modules_subject = crate::section_c::resolve_modules_subject(repo_path, store)
        .unwrap_or_else(|_| "src".into());
    let modules_present = compute_modules(&modules_subject, repo_path, store)
        .map(|r| r.modules.into_iter().map(|m| m.name).collect())
        .unwrap_or_default();

    let mut limitations = vec![
        "Structural edges are working-tree snapshot, not historical structure.".into(),
        "Git history may be HEAD-only depending on last ingest scope.".into(),
        "No runtime scheduling, production traffic, or dynamic DI graph.".into(),
        "AI synthesis is optional and never persisted as repository truth.".into(),
    ];
    limitations.extend(expand_notes);
    if !inv.coverage.github_prs {
        limitations.push("GitHub PRs not ingested for this repository.".into());
    }
    if !inv.coverage.github_issues {
        limitations.push("GitHub issues not ingested for this repository.".into());
    }
    if !inv.coverage.es_imports {
        limitations.push("No structural edges present (run ingest --typescript if applicable).".into());
    }
    if inv.core_candidates.is_empty() {
        limitations.push("No core file candidates matched the anchors.".into());
    }

    let bounds = vec![
        format!("core_candidates cap {}", MAX_CORE_FILES),
        format!("supporting_artifacts cap {}", MAX_SUPPORT_FILES),
        format!("chronology cap {}", MAX_CHRONOLOGY),
        format!("investigation loop max rounds {}", opts.max_rounds_hint),
    ];

    let git_head = git_head_short(repo_path);

    let packet = EvidencePacket {
        schema_version: PACKET_SCHEMA,
        question: opts.question.clone(),
        repo_path: repo_path.to_string(),
        git_head,
        anchors,
        investigation: inv,
        chronology,
        modules_present,
        limitations,
        bounds,
        ranked_evidence: vec![],
        supersession: vec![],
        verification_policy: vec![],
    };
    // C4-ER + C5.1: rank (with personalized structural PageRank when edges exist),
    // temporal supersession, verification policy.
    Ok(crate::evidence_reasoning::enrich_packet_with_store(
        packet,
        Some(store),
    ))
}

fn build_chronology(
    inv: &InvestigationDocument,
    repo_path: &str,
    store: &Store,
) -> Result<Vec<ChronologyEvent>> {
    let mut events: Vec<ChronologyEvent> = Vec::new();

    // Documentary first (intent), then commits (implementation) for top files.
    for d in inv.documentary.iter().take(10) {
        let (ts, summary) = if d.kind == "pr" {
            if let Some(pr) = store.pr_by_number(d.number, repo_path)? {
                (
                    pr.merged_at.or(pr.created_at).unwrap_or(0),
                    format!("PR #{}: {}", d.number, d.title),
                )
            } else {
                (0, format!("PR #{}: {}", d.number, d.title))
            }
        } else if let Some((title, _)) = store.get_issue(d.number, repo_path)? {
            let issue = store.issue_by_number(d.number, repo_path)?;
            let ts = issue.and_then(|i| i.created_at).unwrap_or(0);
            (ts, format!("Issue #{}: {}", d.number, title))
        } else {
            (0, format!("{} #{}: {}", d.kind, d.number, d.title))
        };
        events.push(ChronologyEvent {
            timestamp: ts,
            kind: d.kind.clone(),
            id: format!("{}#{}", d.kind, d.number),
            summary,
            role: "intent".into(),
        });
    }

    let mut files_for_history: Vec<String> = inv
        .core_candidates
        .iter()
        .take(6)
        .map(|c| c.file.clone())
        .collect();
    for a in &inv.anchors {
        if a.contains('/') && !files_for_history.iter().any(|f| f == a) {
            files_for_history.push(a.clone());
        }
    }
    for f in files_for_history.iter().take(8) {
        let commits = store.commits_for_file(f, repo_path)?;
        for row in commits.into_iter().take(5) {
            events.push(ChronologyEvent {
                timestamp: row.timestamp,
                kind: "commit".into(),
                id: row.hash.clone(),
                summary: format!(
                    "{} — {} ({})",
                    &row.short_hash,
                    truncate(&row.message, 80),
                    f
                ),
                role: "implementation".into(),
            });
        }
    }

    events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then(a.id.cmp(&b.id)));
    // Dedup by id
    let mut seen = std::collections::HashSet::new();
    events.retain(|e| seen.insert(e.id.clone()));
    if events.len() > MAX_CHRONOLOGY {
        // keep earliest and latest samples
        let mut kept = Vec::new();
        kept.extend(events.iter().take(MAX_CHRONOLOGY / 2).cloned());
        kept.extend(
            events
                .iter()
                .rev()
                .take(MAX_CHRONOLOGY / 2)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev(),
        );
        events = kept;
        events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    }
    Ok(events)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

// ─── Verification ───────────────────────────────────────────────────────────

/// Verify AI-proposed claims against the evidence packet (deterministic).
///
/// C4-ER: uses hard claim entailment — existence of a ref is not causal support.
pub fn verify_claims(
    claims: &[ProposedClaim],
    packet: &EvidencePacket,
) -> Vec<ProposedClaim> {
    crate::evidence_reasoning::hard_verify_claims(claims, packet)
}

/// Public ref-resolution check (used by C4-ER hard verify and tests).
pub fn evidence_resolves_pub(r: &EvidenceRef, packet: &EvidencePacket) -> bool {
    evidence_resolves(r, packet)
}

fn evidence_resolves(r: &EvidenceRef, packet: &EvidencePacket) -> bool {
    let id = r.id.as_str();
    let kind = r.kind.to_lowercase();
    match kind.as_str() {
        "file" | "path" => {
            let in_candidates = packet
                .investigation
                .core_candidates
                .iter()
                .chain(packet.investigation.supporting_artifacts.iter())
                .any(|c| c.file == id || c.file.ends_with(id) || id.ends_with(&c.file));
            in_candidates
                || packet.anchors.iter().any(|a| a == id || id.contains(a.as_str()))
                || packet.chronology.iter().any(|e| e.summary.contains(id))
                || packet.investigation.observed_structure.iter().any(|o| {
                    o.file == id
                        || o.outgoing.iter().any(|e| e.file == id)
                        || o.incoming.iter().any(|e| e.file == id)
                })
        }
        "commit" => packet.chronology.iter().any(|e| e.id == id || e.id.starts_with(id)),
        "pr" => {
            let n = id.trim_start_matches("pr#").trim_start_matches('#');
            packet
                .investigation
                .documentary
                .iter()
                .any(|d| d.kind == "pr" && d.number.to_string() == n)
                || packet.chronology.iter().any(|e| e.id == format!("pr#{n}") || e.id == format!("pr#{id}"))
        }
        "issue" => {
            let n = id.trim_start_matches("issue#").trim_start_matches('#');
            packet
                .investigation
                .documentary
                .iter()
                .any(|d| d.kind == "issue" && d.number.to_string() == n)
        }
        "module" => packet.modules_present.iter().any(|m| m == id),
        "document" => packet
            .investigation
            .related_decisions
            .iter()
            .any(|d| d.path == id),
        "structural_edge" | "structural" => {
            // id may be "src→tgt" or a file that appears in structure
            packet.investigation.observed_structure.iter().any(|o| {
                o.file == id
                    || o.outgoing.iter().any(|e| e.file == id)
                    || o.incoming.iter().any(|e| e.file == id)
            })
        }
        _ => {
            packet
                .investigation
                .core_candidates
                .iter()
                .any(|c| c.file.contains(id))
                || packet.anchors.iter().any(|a| a.contains(id))
        }
    }
}

pub fn verify_hypotheses(
    hyps: &[Hypothesis],
    packet: &EvidencePacket,
) -> Vec<Hypothesis> {
    crate::evidence_reasoning::hard_verify_hypotheses(hyps, packet)
}

// ─── Investigation loop ─────────────────────────────────────────────────────

pub struct ReasoningOptions {
    pub question: String,
    pub anchors: Vec<String>,
    pub seed_files: Vec<String>,
    pub max_rounds: u32,
    /// When true, never call the provider.
    pub no_ai: bool,
}

/// Run evidence assembly + optional local AI rounds + verification.
pub fn run_reasoning_investigation(
    opts: ReasoningOptions,
    repo_path: &str,
    store: &Store,
    provider: Option<&dyn ReasoningProvider>,
) -> Result<ReasoningInvestigationResult> {
    let max_rounds = opts.max_rounds.clamp(1, MAX_ROUNDS);
    let mut packet_opts = PacketOptions {
        question: opts.question.clone(),
        anchors: opts.anchors.clone(),
        seed_files: opts.seed_files.clone(),
        max_rounds_hint: max_rounds,
    };

    let mut packet = build_evidence_packet(&packet_opts, repo_path, store)?;
    let mut rounds: Vec<InvestigationRound> = Vec::new();
    let mut hypotheses: Vec<Hypothesis> = Vec::new();
    let mut claims: Vec<ProposedClaim> = Vec::new();
    let mut explanation: Option<String> = None;
    let mut model_name: Option<String> = None;
    let mut mode = "deterministic_only".to_string();

    let use_ai = !opts.no_ai && provider.is_some();

    if use_ai {
        if let Some(prov) = provider {
            mode = "local_ai".into();
            model_name = Some(prov.meta().model.clone());
            let system = investigation_system_prompt();

            for round in 1..=max_rounds {
                let purpose = match round {
                    1 => "symptom → candidates and initial hypotheses",
                    2 => "expand requested subjects; history/structure",
                    _ => "contradiction resolution and synthesis",
                };
                let user = packet_prompt_summary(&packet, MAX_CORE_FILES);
                match prov.reason(system, &user) {
                    Ok(raw) => {
                        let verified_claims = verify_claims(&raw.proposed_claims, &packet);
                        let verified_hyps = verify_hypotheses(&raw.hypotheses, &packet);
                        claims = verified_claims.clone();
                        hypotheses = verified_hyps;
                        if !raw.explanation.is_empty() {
                            explanation = Some(raw.explanation.clone());
                        }
                        rounds.push(InvestigationRound {
                            round,
                            purpose: purpose.into(),
                            ai_invoked: true,
                            model: Some(prov.meta().model.clone()),
                            raw_ai_response: Some(raw.clone()),
                            verified_claims,
                        });

                        // Expand packet with requested subjects for next round
                        let mut expanded = false;
                        for sub in raw.requested_subjects.iter().take(6) {
                            let s = sub.trim();
                            if s.is_empty() {
                                continue;
                            }
                            if !packet_opts.seed_files.iter().any(|f| f == s)
                                && !packet_opts.anchors.iter().any(|a| a == s)
                            {
                                packet_opts.seed_files.push(s.to_string());
                                expanded = true;
                            }
                        }
                        if expanded && round < max_rounds {
                            packet = build_evidence_packet(&packet_opts, repo_path, store)?;
                        } else if raw.requested_subjects.is_empty() || round >= 2 {
                            // no more expansion needed
                            break;
                        }
                    }
                    Err(_) => {
                        rounds.push(InvestigationRound {
                            round,
                            purpose: format!("{purpose} (AI unavailable)"),
                            ai_invoked: false,
                            model: None,
                            raw_ai_response: None,
                            verified_claims: vec![],
                        });
                        mode = "deterministic_only".into();
                        model_name = None;
                        break;
                    }
                }
            }
        }
    }

    // Deterministic summary fields always filled from packet
    let likely_area = derive_likely_area(&packet);
    let affected = packet
        .investigation
        .core_candidates
        .iter()
        .map(|c| c.file.clone())
        .chain(
            packet
                .investigation
                .supporting_artifacts
                .iter()
                .filter(|c| c.role == ArtifactRole::Test)
                .map(|c| c.file.clone()),
        )
        .take(20)
        .collect::<Vec<_>>();

    let relevant_issues_prs = packet
        .investigation
        .documentary
        .iter()
        .map(|d| format!("{} #{} — {}", d.kind, d.number, d.title))
        .collect();

    let what_atlas_knows = derive_knows(&packet, &hypotheses);
    let what_atlas_does_not_know = packet.limitations.clone();
    let next_investigation = derive_next(&packet, &hypotheses);

    // If no AI hypotheses, emit a deterministic localization hypothesis.
    // C4 sacred: ranking/association is NEVER auto-SUPPORTED — hard_verify below.
    if hypotheses.is_empty() && !packet.investigation.core_candidates.is_empty() {
        let top = &packet.investigation.core_candidates[0];
        hypotheses.push(Hypothesis {
            id: "det-1".into(),
            statement: format!(
                "Deterministic retrieval associates this question with `{}` and its neighborhood.",
                top.file
            ),
            // Pre-verify status is irrelevant; hard_verify_hypotheses overwrites it.
            status: ClaimStatus::Unresolved,
            supporting: vec![EvidenceRef {
                kind: "file".into(),
                id: top.file.clone(),
                summary: "Top-ranked core candidate from anchor investigation (localization only, not causal support)".into(),
                timestamp: None,
            }],
            contradicting: vec![],
            claims: vec![],
        });
    }

    // C4: every hypothesis — AI or deterministic — passes hard_verify.
    // Ranking existence must not upgrade STATUS to SUPPORTED.
    hypotheses = crate::evidence_reasoning::hard_verify_hypotheses(&hypotheses, &packet);
    if !claims.is_empty() {
        claims = crate::evidence_reasoning::hard_verify_claims(&claims, &packet);
    }

    let chronology = packet.chronology.clone();

    Ok(ReasoningInvestigationResult {
        schema_version: RESULT_SCHEMA,
        question: opts.question,
        mode,
        model: model_name,
        packet,
        rounds,
        hypotheses,
        claims,
        likely_area,
        chronology,
        affected_components: affected,
        relevant_issues_prs,
        what_atlas_knows,
        what_atlas_does_not_know,
        next_investigation,
        explanation,
    })
}

/// Soft re-rank by repository path class (production > demo/asset/CI)
/// plus compound subject-stem promotion (operation store → op_store.rs).
fn apply_path_class_rerank(
    candidates: Vec<CandidateArtifact>,
    question: &str,
) -> Vec<CandidateArtifact> {
    let mut scored: Vec<(f32, CandidateArtifact)> = candidates
        .into_iter()
        .map(|mut c| {
            let base = if c.score.total > 0.0 {
                c.score.total * 20.0
            } else {
                c.score.lexical * 20.0
            };
            let mut adj =
                crate::path_class::apply_class_to_score(&c.file, question, base.max(1.0));
            adj += crate::subject_resolve::subject_stem_boost(&c.file, question);
            // Reflect class into total for transparency
            c.score.total = (adj / 30.0).clamp(0.0, 1.0).max(c.score.total * 0.5);
            // Align ArtifactRole with path class when we demote examples/docs
            let class = crate::path_class::classify_path(&c.file);
            c.role = crate::path_class::to_artifact_role(class);
            (adj, c)
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.file.cmp(&b.1.file))
    });
    scored.into_iter().map(|(_, c)| c).collect()
}

fn derive_likely_area(packet: &EvidencePacket) -> Vec<String> {
    let mut areas: Vec<String> = Vec::new();
    for c in packet.investigation.core_candidates.iter().take(5) {
        // module segment under src/modules/X/
        if let Some(rest) = c.file.strip_prefix("src/modules/") {
            if let Some(m) = rest.split('/').next() {
                if !areas.iter().any(|a| a == m) {
                    areas.push(m.to_string());
                }
            }
        } else {
            let parent = c.file.rsplit_once('/').map(|(p, _)| p).unwrap_or(&c.file);
            if !areas.iter().any(|a| a == parent) {
                areas.push(parent.to_string());
            }
        }
    }
    for m in packet.modules_present.iter().take(3) {
        if packet
            .investigation
            .core_candidates
            .iter()
            .any(|c| c.file.contains(&format!("/{m}/")))
            && !areas.iter().any(|a| a == m)
        {
            areas.push(m.clone());
        }
    }
    areas
}

fn derive_knows(packet: &EvidencePacket, hyps: &[Hypothesis]) -> Vec<String> {
    let mut v = Vec::new();
    v.push(format!(
        "{} core candidate file(s) from deterministic investigation",
        packet.investigation.core_candidates.len()
    ));
    v.push(format!(
        "{} documentary item(s) (PR/issue matches)",
        packet.investigation.documentary.len()
    ));
    v.push(format!(
        "{} chronology event(s) assembled (intent + implementation)",
        packet.chronology.len()
    ));
    if !hyps.is_empty() {
        v.push(format!(
            "{} hypothesis(es) after verification",
            hyps.len()
        ));
    }
    v
}

fn derive_next(packet: &EvidencePacket, hyps: &[Hypothesis]) -> Vec<String> {
    let mut v = Vec::new();
    if packet.investigation.core_candidates.is_empty() {
        v.push("Ingest GitHub issues/PRs or refine anchors; no file candidates yet.".into());
    }
    if !packet.coverage_structural() {
        v.push("Run `atlas ingest . --typescript` (or language flag) for structural edges.".into());
    }
    for h in hyps.iter().filter(|h| h.status == ClaimStatus::Unresolved).take(2) {
        v.push(format!("Gather more evidence for unresolved hypothesis: {}", h.statement));
    }
    if v.is_empty() {
        v.push("Drill with `atlas show <file>` or `atlas inspect <module>` on likely area.".into());
        v.push("Compare chronology intent (PRs) vs recent implementation commits.".into());
    }
    v
}

trait PacketCov {
    fn coverage_structural(&self) -> bool;
}
impl PacketCov for EvidencePacket {
    fn coverage_structural(&self) -> bool {
        self.investigation.coverage.es_imports
            || self.investigation.coverage.static_calls
            || self.investigation.coverage.model_refs
    }
}

// ─── Issue / file entry helpers ─────────────────────────────────────────────

/// Build reasoning options from a stored GitHub issue.
pub fn options_from_issue(
    issue_number: i64,
    repo_path: &str,
    store: &Store,
) -> Result<Option<ReasoningOptions>> {
    let Some((title, body)) = store.get_issue(issue_number, repo_path)? else {
        return Ok(None);
    };
    let mut anchors = extract_issue_anchors(&title, &body);
    if anchors.is_empty() {
        anchors = anchors_from_question(&format!("{title} {body}"));
    }
    let question = format!("GitHub issue #{issue_number}: {title}");
    let mut seed_files = Vec::new();
    let mut notes = Vec::new();
    // C5.1-R issue-anchored seeds (PR merge files, #N commits, path fragments).
    crate::retrieval_expand::apply_expansion(
        &question,
        &mut anchors,
        &mut seed_files,
        &mut notes,
        Some(issue_number),
        repo_path,
        store,
    )?;
    let _ = notes; // notes re-applied in build_evidence_packet via question text
    Ok(Some(ReasoningOptions {
        question,
        anchors,
        seed_files,
        max_rounds: 3,
        no_ai: false,
    }))
}

pub fn options_from_question(question: &str) -> ReasoningOptions {
    ReasoningOptions {
        question: question.to_string(),
        anchors: anchors_from_question(question),
        seed_files: vec![],
        max_rounds: 3,
        no_ai: false,
    }
}

pub fn options_from_file(path: &str, question: Option<&str>) -> ReasoningOptions {
    let q = question
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("Investigate file {path}"));
    let mut anchors = anchors_from_question(&q);
    if !anchors.iter().any(|a| a == path) {
        anchors.insert(0, path.to_string());
    }
    ReasoningOptions {
        question: q,
        anchors,
        seed_files: vec![path.to_string()],
        max_rounds: 3,
        no_ai: false,
    }
}
