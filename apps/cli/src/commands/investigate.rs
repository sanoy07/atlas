use anyhow::Result;
use atlas_core::investigate;
use atlas_ir::{ArtifactRole, CandidateReason, ConceptExpansion, InvestigationDocument, InvestigationCoverage};
use atlas_storage::Store;
use serde_json;

pub fn run(anchors: &[String], json: bool, raw: bool) -> Result<()> {
    let db_path = std::env::var("ATLAS_DB").unwrap_or_else(|_| "./atlas.db".to_string());
    let store   = Store::open(&db_path)?;
    let repo    = super::discover_repo_root()?;

    let anchor_strs: Vec<&str> = anchors.iter().map(String::as_str).collect();
    let doc = investigate(&anchor_strs, &repo, &store)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&doc)?);
        return Ok(());
    }

    if raw {
        render(&doc);
        return Ok(());
    }

    // Default: AI synthesis.
    // Skip synthesis if there are no candidates — nothing useful to synthesize.
    let has_candidates = !doc.core_candidates.is_empty() || !doc.supporting_artifacts.is_empty();
    if !has_candidates {
        render(&doc);
        return Ok(());
    }

    eprintln!("Synthesizing with qwen2.5-coder:7b-instruct …");
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
        }
        None => {
            eprintln!("(Ollama unavailable — showing raw evidence. Run `ollama serve` to enable synthesis.)");
            eprintln!();
            render_body(&doc);
        }
    }

    Ok(())
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
