use anyhow::{anyhow, Result};
use atlas_core::{
    investigate_cached, options_from_file, options_from_issue, options_from_question,
    run_reasoning_investigation, OllamaProvider, ReasoningOptions,
};
use atlas_ir::{
    ArtifactRole, CandidateReason, ClaimStatus, ConceptExpansion, InvestigationCoverage,
    InvestigationDocument, ReasoningInvestigationResult,
};
use atlas_storage::Store;
use serde_json;

/// `atlas investigate` entry.
///
/// - Anchor mode (default): short terms → InvestigationDocument (+ optional prose AI).
/// - Reasoning mode: quoted question / `--issue` / `--file` → evidence packet + verified claims.
pub fn run(
    anchors: &[String],
    json: bool,
    raw: bool,
    repo_override: Option<&str>,
    issue: Option<i64>,
    file: Option<&str>,
    no_ai: bool,
    max_rounds: u32,
) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;
    let repo = match repo_override {
        Some(r) => super::canonical_repo_path(r),
        None => super::discover_repo_root()?,
    };

    let reasoning_mode = issue.is_some()
        || file.is_some()
        || (anchors.len() == 1 && anchors[0].contains(' '))
        || anchors.iter().any(|a| a.contains(' '));

    if reasoning_mode {
        return run_reasoning(anchors, json, raw, &repo, &store, issue, file, no_ai, max_rounds);
    }

    let anchor_strs: Vec<&str> = anchors.iter().map(String::as_str).collect();
    let doc = investigate_cached(&anchor_strs, &repo, &store)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&doc)?);
        return Ok(());
    }

    if raw || no_ai {
        render(&doc);
        return Ok(());
    }

    let has_candidates = !doc.core_candidates.is_empty() || !doc.supporting_artifacts.is_empty();
    if !has_candidates {
        render(&doc);
        return Ok(());
    }

    eprintln!(
        "Synthesizing with {} (prose mode) …",
        crate::ai::synthesis_model_name()
    );
    let synthesis = crate::ai::synthesize(&doc);

    println!("INVESTIGATION");
    println!("anchors: {}", doc.anchors.join(" · "));
    println!();

    match synthesis {
        Some(ref text) => {
            println!("{}", text);
            println!();
            render_coverage(&doc.coverage);
            println!();
            println!("Run --raw for full evidence · --json for machine-readable output.");
            println!("Tip: quoted questions use structured reasoning:");
            println!("  atlas investigate \"orders timeout under concurrency\"");
        }
        None => {
            eprintln!("(Ollama unavailable — showing raw evidence. Run `ollama serve` to enable synthesis.)");
            eprintln!();
            render_body(&doc);
        }
    }

    Ok(())
}

fn run_reasoning(
    anchors: &[String],
    json: bool,
    raw: bool,
    repo: &str,
    store: &Store,
    issue: Option<i64>,
    file: Option<&str>,
    no_ai: bool,
    max_rounds: u32,
) -> Result<()> {
    let mut opts: ReasoningOptions = if let Some(n) = issue {
        options_from_issue(n, repo, store)?.ok_or_else(|| {
            anyhow!(
                "issue #{} not found in DB — run `atlas ingest . --github` if needed",
                n
            )
        })?
    } else if let Some(f) = file {
        let q = if anchors.is_empty() {
            None
        } else {
            Some(anchors.join(" "))
        };
        options_from_file(f, q.as_deref())
    } else {
        options_from_question(&anchors.join(" "))
    };
    opts.no_ai = no_ai || raw;
    opts.max_rounds = max_rounds.clamp(1, 3);

    let provider;
    let provider_ref = if opts.no_ai {
        None
    } else {
        provider = OllamaProvider::default();
        Some(&provider as &dyn atlas_core::ReasoningProvider)
    };

    if provider_ref.is_some() {
        eprintln!(
            "Reasoning investigation with {} (max {} round{}) …",
            crate::ai::reasoning_model_name(),
            opts.max_rounds,
            if opts.max_rounds == 1 { "" } else { "s" }
        );
    } else {
        eprintln!("Deterministic evidence packet only …");
    }

    let result = run_reasoning_investigation(opts, repo, store, provider_ref)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    render_reasoning(&result);
    Ok(())
}

fn render_reasoning(r: &ReasoningInvestigationResult) {
    println!("ATLAS INVESTIGATION");
    println!();
    println!("Question:");
    println!("  {}", r.question);
    println!();
    println!(
        "Mode: {}{}",
        r.mode,
        r.model
            .as_ref()
            .map(|m| format!("  model={m}"))
            .unwrap_or_default()
    );
    println!();

    if !r.likely_area.is_empty() {
        println!("LIKELY AREA");
        for a in &r.likely_area {
            println!("  · {}", a);
        }
        println!();
    }

    // C4-ER: ranked evidence drives conclusions; bag dumps do not.
    if !r.packet.ranked_evidence.is_empty() {
        println!("RANKED EVIDENCE  (weight · semantics · ref)");
        for item in r.packet.ranked_evidence.iter().take(12) {
            println!(
                "  #{:<2} {:>4.2}  {:<14}  [{}] {} — {}",
                item.rank,
                item.weight,
                item.event_semantics,
                item.ref_.kind,
                item.ref_.id,
                item.ref_.summary
            );
        }
        if r.packet.ranked_evidence.len() > 12 {
            println!("  … {} more", r.packet.ranked_evidence.len() - 12);
        }
        println!();
    }
    if !r.packet.supersession.is_empty() {
        println!("SUPERSESSION  (not mere recency)");
        for s in r.packet.supersession.iter().take(6) {
            println!(
                "  {} → {}  ({})",
                s.earlier_id, s.later_id, s.relationship
            );
        }
        if r.packet.supersession.len() > 6 {
            println!("  … {} more", r.packet.supersession.len() - 6);
        }
        println!();
    }

    for (i, h) in r.hypotheses.iter().enumerate() {
        println!("HYPOTHESIS {}", i + 1);
        println!("  {}", h.statement);
        println!("  STATUS: {}", status_label(&h.status));
        if !h.supporting.is_empty() {
            println!("  Supporting evidence:");
            for e in &h.supporting {
                println!("    - [{}] {} — {}", e.kind, e.id, e.summary);
            }
        }
        if !h.contradicting.is_empty() {
            println!("  Contradicting evidence:");
            for e in &h.contradicting {
                println!("    - [{}] {} — {}", e.kind, e.id, e.summary);
            }
        }
        println!();
    }

    if !r.claims.is_empty() {
        println!("VERIFIED CLAIMS");
        for c in &r.claims {
            println!("  [{}] {} — {}", status_label(&c.status), c.id, c.statement);
            for e in &c.evidence_refs {
                println!("    evidence: [{}] {}", e.kind, e.id);
            }
        }
        println!();
    }

    if !r.chronology.is_empty() {
        println!("CHRONOLOGY  (intent vs implementation)");
        for ev in r.chronology.iter().take(20) {
            println!("  {:>10}  {:<14}  {}  {}", ev.timestamp, ev.role, ev.id, ev.summary);
        }
        if r.chronology.len() > 20 {
            println!("  … {} more", r.chronology.len() - 20);
        }
        println!();
    }

    if !r.affected_components.is_empty() {
        println!("AFFECTED COMPONENTS  (retrieval neighborhood)");
        for c in &r.affected_components {
            println!("  · {}", c);
        }
        println!();
    }

    if !r.relevant_issues_prs.is_empty() {
        println!("RELEVANT ISSUES / PRS");
        for d in &r.relevant_issues_prs {
            println!("  · {}", d);
        }
        println!();
    }

    if let Some(ex) = &r.explanation {
        if !ex.is_empty() {
            println!("EXPLANATION  (local AI — not repository fact)");
            println!("{ex}");
            println!();
        }
    }

    println!("WHAT ATLAS KNOWS");
    for k in &r.what_atlas_knows {
        println!("  · {k}");
    }
    println!();
    println!("WHAT ATLAS DOES NOT KNOW");
    for k in &r.what_atlas_does_not_know {
        println!("  · {k}");
    }
    println!();
    if !r.packet.verification_policy.is_empty() {
        println!("VERIFICATION POLICY  (C4-ER)");
        for v in &r.packet.verification_policy {
            println!("  · {v}");
        }
        println!();
    }
    if !r.next_investigation.is_empty() {
        println!("NEXT INVESTIGATION");
        for n in &r.next_investigation {
            println!("  · {n}");
        }
        println!();
    }
    println!("Use --json for the full evidence packet and claim structures.");
}

fn status_label(s: &ClaimStatus) -> &'static str {
    match s {
        ClaimStatus::Supported => "SUPPORTED",
        ClaimStatus::Contradicted => "CONTRADICTED",
        ClaimStatus::Plausible => "PLAUSIBLE",
        ClaimStatus::Unresolved => "UNRESOLVED",
    }
}

// ── Public for investigations show ───────────────────────────────────────────

pub fn render_stored(doc: &InvestigationDocument) {
    render(doc);
}

// ── Raw render (--raw flag or Ollama fallback) ────────────────────────────────

fn render(doc: &InvestigationDocument) {
    println!("INVESTIGATION");
    println!("anchors: {}", doc.anchors.join(" · "));
    println!();

    let total = doc.core_candidates.len() + doc.supporting_artifacts.len();
    if total == 0 {
        if doc.related_decisions.is_empty() {
            println!("No candidates found. Run `atlas ingest .` first.");
            println!();
        } else {
            println!("No file candidates found — but engineering decisions matched.");
            println!();
        }
        render_engineering_decisions(doc);
        render_coverage(&doc.coverage);
        return;
    }

    render_body(doc);
}

fn render_body(doc: &InvestigationDocument) {
    // ── CONCEPT RESOLUTION ────────────────────────────────────────────────────
    if !doc.concept_expansions.is_empty() {
        println!("CONCEPT RESOLUTION  ({} expansion{})",
            doc.concept_expansions.len(),
            if doc.concept_expansions.len() == 1 { "" } else { "s" });
        for exp in &doc.concept_expansions {
            render_concept_expansion(exp);
        }
        println!();
    }

    // ── CORE IMPLEMENTATION NEIGHBORHOOD ─────────────────────────────────────
    if !doc.core_candidates.is_empty() {
        println!("CORE IMPLEMENTATION NEIGHBORHOOD  ({} files)", doc.core_candidates.len());
        for c in &doc.core_candidates {
            println!("  {}", c.file);
            render_candidate_reasons(&c.reasons);
        }
        println!();
    }

    // ── SUPPORTING ARTIFACTS ──────────────────────────────────────────────────
    if !doc.supporting_artifacts.is_empty() {
        println!("SUPPORTING ARTIFACTS  ({} files)", doc.supporting_artifacts.len());
        for c in &doc.supporting_artifacts {
            let label = role_label(&c.role);
            println!("  {}  {}", c.file, label);
            render_candidate_reasons(&c.reasons);
        }
        println!();
    }

    // ── OBSERVED STRUCTURE ────────────────────────────────────────────────────
    let non_empty_obs: Vec<_> = doc.observed_structure.iter()
        .filter(|o| !o.outgoing.is_empty() || !o.incoming.is_empty())
        .collect();

    if !non_empty_obs.is_empty() {
        println!("OBSERVED STRUCTURE");
        for obs in non_empty_obs {
            println!("  {}", obs.file);
            for e in &obs.outgoing {
                let sym = e.symbol.as_deref().unwrap_or("");
                let sym_str = if sym.is_empty() { String::new() } else { format!("  [{}]", sym) };
                println!("    →  {}  ({}{})", e.kind.to_uppercase(), e.file, sym_str);
            }
            for e in &obs.incoming {
                println!("    ←  {}  ({})", e.kind.to_uppercase(), e.file);
            }
        }
        println!();
    }

    // Core candidates with zero structural connections within the candidate set.
    let core_files: std::collections::HashSet<&str> =
        doc.core_candidates.iter().map(|c| c.file.as_str()).collect();
    let isolated: Vec<_> = doc.observed_structure.iter()
        .filter(|o| core_files.contains(o.file.as_str()))
        .filter(|o| o.outgoing.is_empty() && o.incoming.is_empty())
        .collect();
    if !isolated.is_empty() {
        println!("STRUCTURALLY ISOLATED  (no connections to other candidates)");
        for obs in isolated {
            println!("  {}  — no edges observed", obs.file);
        }
        println!();
    }

    // ── DOCUMENTARY EVIDENCE ──────────────────────────────────────────────────
    if !doc.documentary.is_empty() {
        println!("DOCUMENTARY EVIDENCE  ({} items)", doc.documentary.len());
        for ev in &doc.documentary {
            let kind_label = if ev.kind == "pr" { "PR" } else { "Issue" };
            let anchors_str = ev.matched_anchors.join(", ");
            println!("  {} #{}  [{}]", kind_label, ev.number, anchors_str);
            if !ev.title.is_empty() {
                println!("    {}", ev.title.chars().take(100).collect::<String>());
            }
            for snippet in ev.snippets.iter().take(2) {
                println!("    \"{}\"", snippet.chars().take(120).collect::<String>());
            }
        }
        println!();
    }

    // ── ENGINEERING DECISIONS ─────────────────────────────────────────────────
    render_engineering_decisions(doc);

    // ── HISTORICAL EVIDENCE ───────────────────────────────────────────────────
    let historical_with_data: Vec<_> = doc.historical.iter()
        .filter(|h| h.touch_count > 0)
        .collect();
    if !historical_with_data.is_empty() {
        println!("HISTORICAL EVIDENCE");
        for h in &historical_with_data {
            let co_note = if h.co_changed_candidates.is_empty() {
                String::new()
            } else {
                format!("  co-changes: {}", h.co_changed_candidates.join(", "))
            };
            println!("  {}×  {}{}", h.touch_count, h.file, co_note);
        }
        println!();
    }

    // ── UNRESOLVED CONNECTIONS ────────────────────────────────────────────────
    if !doc.unresolved.is_empty() {
        println!("UNRESOLVED CONNECTIONS");
        for u in &doc.unresolved {
            println!("  {}", u.subject);
            if let Some(ref ind) = u.documentary_indication {
                println!("    Documentary: {}", ind.chars().take(120).collect::<String>());
            }
            println!("    Structural:  {}", u.observation.chars().take(120).collect::<String>());
        }
        println!();
    }

    render_coverage(&doc.coverage);
}

// ── Shared helpers ────────────────────────────────────────────────────────────

fn render_concept_expansion(exp: &ConceptExpansion) {
    println!("  {} (no direct file-path match)", exp.original_term);
    println!("    Bridge: {}:  \"{}\"",
        exp.bridge_source,
        exp.bridge_snippet.chars().take(110).collect::<String>());
    for v in &exp.verified_expansions {
        println!("    Verified expansion: {}  →  {}", v.term, v.verified_in);
    }
    let added: Vec<&str> = exp.verified_expansions.iter().map(|v| v.term.as_str()).collect();
    println!("    Added to investigation: {}", added.join(", "));
}

fn role_label(role: &ArtifactRole) -> &'static str {
    match role {
        ArtifactRole::ProductionSource => "",
        ArtifactRole::Test             => "[test]",
        ArtifactRole::Migration        => "[migration]",
        ArtifactRole::Seeder           => "[seeder]",
        ArtifactRole::Script           => "[script]",
        ArtifactRole::Example          => "[example]",
        ArtifactRole::Schema           => "[schema]",
        ArtifactRole::Validation       => "[validation]",
        ArtifactRole::Permission       => "[permission]",
        ArtifactRole::Documentation    => "[documentation]",
        ArtifactRole::Generated        => "[generated]",
        ArtifactRole::Unknown          => "[unknown]",
    }
}

fn render_candidate_reasons(reasons: &[CandidateReason]) {
    for reason in reasons {
        match reason {
            CandidateReason::AnchorMatch { anchor, via } => {
                println!("    ← anchor match \"{}\" ({})", anchor, via);
            }
            CandidateReason::StructuralNeighbor { from_file, kind, direction } => {
                let arrow = if direction == "outgoing" { "→" } else { "←" };
                println!("    ← structural neighbor  {} {}  {}", kind, arrow, from_file);
            }
        }
    }
}

fn render_engineering_decisions(doc: &InvestigationDocument) {
    if !doc.related_decisions.is_empty() {
        println!("ENGINEERING DECISIONS  ({} records)", doc.related_decisions.len());
        for d in &doc.related_decisions {
            println!("  {}  ({})", d.title, d.path);
            println!("    \"{}\"", d.snippet.chars().take(120).collect::<String>());
        }
        println!();
    }
}

fn render_coverage(cov: &InvestigationCoverage) {
    println!("COVERAGE");
    println!("  Git history      {}", tick(cov.git_history));
    println!("  GitHub PRs       {}", tick(cov.github_prs));
    println!("  GitHub issues    {}", tick(cov.github_issues));
    println!("  File paths       {}", tick(cov.file_paths));
    println!("  ES imports       {}", tick(cov.es_imports));
    println!("  Static calls     {}", tick(cov.static_calls));
    println!("  Model refs       {}", tick(cov.model_refs));
    println!("  Dynamic dispatch ✗ not analyzed");
    println!("  Runtime DI       ✗ not analyzed");
    println!("  Working tree     ✗ not ingested");
}

fn tick(b: bool) -> &'static str {
    if b { "✓" } else { "✗ not ingested" }
}

