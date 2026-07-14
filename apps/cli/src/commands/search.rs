use anyhow::Result;
use atlas_core::search;
use atlas_ir::{AnchorMatch, EvidenceType, MatchSource, SearchDocument};
use atlas_storage::Store;
use serde_json;

pub fn run(anchors: &[String], json: bool) -> Result<()> {
    let db_path = std::env::var("ATLAS_DB").unwrap_or_else(|_| "./atlas.db".to_string());
    let store   = Store::open(&db_path)?;

    let repo = super::discover_repo_root()?;
    let anchor_strs: Vec<&str> = anchors.iter().map(String::as_str).collect();
    let doc = search(&anchor_strs, &repo, &store)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&doc)?);
    } else {
        render(&doc);
    }
    Ok(())
}

fn render(doc: &SearchDocument) {
    println!("ATLAS SEARCH");
    println!("anchors: {}", doc.anchors.join(" · "));
    println!();

    if doc.matches.is_empty() {
        println!("No matches found.");
        println!();
        render_coverage(doc);
        return;
    }

    let observed:    Vec<&AnchorMatch> = doc.matches.iter().filter(|m| m.evidence_type == EvidenceType::Observed).collect();
    let documentary: Vec<&AnchorMatch> = doc.matches.iter().filter(|m| m.evidence_type == EvidenceType::Documentary).collect();
    let engineering: Vec<&AnchorMatch> = doc.matches.iter().filter(|m| m.evidence_type == EvidenceType::Engineering).collect();
    let historical:  Vec<&AnchorMatch> = doc.matches.iter().filter(|m| m.evidence_type == EvidenceType::Historical).collect();

    if !observed.is_empty() {
        render_section("OBSERVED", "code artifacts", &observed, &doc.anchors);
    }
    if !documentary.is_empty() {
        render_section("DOCUMENTARY", "PRs · issues", &documentary, &doc.anchors);
    }
    if !engineering.is_empty() {
        render_section("ENGINEERING DECISIONS", "decision records · ADRs", &engineering, &doc.anchors);
    }
    if !historical.is_empty() {
        render_section("HISTORICAL", "commit messages", &historical, &doc.anchors);
    }

    render_coverage(doc);
}

fn render_section(header: &str, subtitle: &str, matches: &[&AnchorMatch], all_anchors: &[String]) {
    println!("{header}  ({subtitle})");

    // Group by source_id so that a PR with both a title and body match
    // shows the body snippet after the title, not as a separate item.
    let mut last_id = String::new();
    for m in matches {
        let label = fmt_source_label(m);
        if m.source_id != last_id {
            // New source — print the identifier line.
            let anchor_note = anchor_tags(&m.source_id, matches, all_anchors);
            println!("  {}  {}", label, anchor_note);
            last_id = m.source_id.clone();
        }
        // Always print the snippet (title / path / excerpt).
        let indent = match m.source {
            MatchSource::PrBody | MatchSource::IssueBody => "      ",
            _                                            => "    ",
        };
        println!("{}\"{}\"", indent, m.snippet.chars().take(120).collect::<String>());
    }
    println!();
}

/// Build the short label shown before the snippet: path, "commit abc123", "PR #11", etc.
fn fmt_source_label(m: &AnchorMatch) -> String {
    match m.source {
        MatchSource::FilePath      => m.source_id.clone(),
        MatchSource::CommitMessage => format!("commit {}", &m.source_id[..7.min(m.source_id.len())]),
        MatchSource::PrTitle       => format!("PR #{}", m.source_id),
        MatchSource::PrBody        => format!("PR #{} body", m.source_id),
        MatchSource::IssueTitle    => format!("Issue #{}", m.source_id),
        MatchSource::IssueBody     => format!("Issue #{} body", m.source_id),
        MatchSource::DecisionBody  => m.source_id.clone(),
    }
}

/// Collect which anchors matched this source_id within the current section.
fn anchor_tags(source_id: &str, section_matches: &[&AnchorMatch], _all: &[String]) -> String {
    let mut anchors: Vec<&str> = section_matches
        .iter()
        .filter(|m| m.source_id == source_id)
        .map(|m| m.anchor.as_str())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    anchors.sort();
    format!("[{}]", anchors.join(", "))
}

fn render_coverage(doc: &SearchDocument) {
    println!("COVERAGE");
    println!("  File paths      {}", tick(doc.coverage.file_paths));
    println!("  Commit history  {}", tick(doc.coverage.commit_history));
    println!("  Pull requests   {}", tick(doc.coverage.pull_requests));
    println!("  Issues          {}", tick(doc.coverage.issues));
    println!("  Eng. decisions  {}", tick(doc.coverage.engineering_decisions));
    println!("  Source code     ✗ not yet (v0.5b)");
    println!("  Working tree    ✗ not ingested");
}

fn tick(b: bool) -> &'static str {
    if b { "✓ searched" } else { "✗ not ingested" }
}
