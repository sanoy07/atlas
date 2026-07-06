use anyhow::Result;
use atlas_core::build_context;
use atlas_ir::{CoverageStatus, ContextDocument};
use atlas_storage::Store;
use chrono::{DateTime, Utc};
use serde_json;

pub fn run(file: &str, json: bool) -> Result<()> {
    let db_path = std::env::var("ATLAS_DB").unwrap_or_else(|_| "./atlas.db".to_string());
    let store   = Store::open(&db_path)?;

    let doc = build_context(file, ".", &store)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&doc)?);
    } else {
        render(&doc);
    }
    Ok(())
}

fn fmt_date(ts: i64) -> String {
    DateTime::<Utc>::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

fn render(doc: &ContextDocument) {
    println!("ATLAS CONTEXT");
    println!("{}", doc.subject);
    println!();

    // ── IDENTITY ─────────────────────────────────────────────────────────────
    println!("IDENTITY");
    match &doc.identity.first_commit {
        None => println!("  Introduced:    (unknown — run atlas ingest first)"),
        Some(c) => println!("  Introduced:    {}  ({}  {})",
            fmt_date(c.timestamp), c.short_hash, c.message),
    }
    match &doc.identity.last_commit {
        None => {}
        Some(c) => println!("  Last changed:  {}  ({}  {})",
            fmt_date(c.timestamp), c.short_hash, c.message),
    }
    println!("  Total touches: {}", doc.identity.touch_count);
    println!();

    // ── RECENT ACTIVITY ───────────────────────────────────────────────────────
    if doc.recent_activity.is_empty() {
        println!("RECENT ACTIVITY");
        println!("  No commits found.  Run `atlas ingest .` first.");
    } else {
        let shown   = doc.recent_activity.iter().take(5);
        let total   = doc.recent_activity.len();
        let header  = if total > 5 {
            format!("RECENT ACTIVITY ({total} commits, showing 5 most recent)")
        } else {
            format!("RECENT ACTIVITY ({total} commit{})", if total == 1 { "" } else { "s" })
        };
        println!("{header}");
        for c in shown {
            println!("  {}  {}  {}", c.short_hash, fmt_date(c.timestamp), c.message);
        }
        if total > 5 {
            println!("  … and {} earlier commit{}", total - 5, if total - 5 == 1 { "" } else { "s" });
        }
    }
    println!();

    // ── RELATED HISTORY ───────────────────────────────────────────────────────
    let has_history = !doc.related_history.pull_requests.is_empty()
        || !doc.related_history.issues.is_empty();
    if has_history {
        println!("RELATED HISTORY");
        for pr in &doc.related_history.pull_requests {
            println!("  PR #{}  {}  [{}]", pr.number, pr.title, pr.state.to_uppercase());
            if let Some(sha) = &pr.merge_commit_sha {
                println!("    merge commit: {}", &sha[..7.min(sha.len())]);
            }
            if !pr.linked_issues.is_empty() {
                let nums: Vec<String> = pr.linked_issues.iter().map(|n| format!("#{n}")).collect();
                println!("    closes: {}", nums.join(", "));
            }
        }
        if !doc.related_history.pull_requests.is_empty() && !doc.related_history.issues.is_empty() {
            println!();
        }
        for issue in &doc.related_history.issues {
            println!("  Issue #{}  {}  [{}]", issue.number, issue.title, issue.state.to_uppercase());
        }
        println!();
    }

    // ── HISTORICAL COUPLING ───────────────────────────────────────────────────
    if !doc.coupling.is_empty() {
        println!("HISTORICAL COUPLING");
        for entry in &doc.coupling {
            println!("  {:>3}×  {}", entry.change_count, entry.file_path);
        }
        println!();
    }

    // ── ARCHITECTURAL CONTEXT ─────────────────────────────────────────────────
    if !doc.documentary.is_empty() {
        println!("ARCHITECTURAL CONTEXT");
        for entry in &doc.documentary {
            println!("  {:>3}×  {}", entry.change_count, entry.file_path);
            println!("         changed alongside this component");
        }
        println!();
    }

    // ── CURRENT SIGNIFICANCE ──────────────────────────────────────────────────
    if let Some(sig) = &doc.significance {
        println!("CURRENT SIGNIFICANCE");
        println!("  Ranked #{} of {} most-modified files in repository ({} touches).",
            sig.rank, sig.total_files, sig.touch_count);
        println!();
    }

    // ── COVERAGE ──────────────────────────────────────────────────────────────
    println!("COVERAGE");
    println!("  Git history       {}", fmt_coverage(&doc.coverage.git_history, Some("path-scoped")));
    println!("  Rename tracking   {}", fmt_coverage(&doc.coverage.rename_tracking, None));
    println!("  GitHub PRs        {}", fmt_coverage(&doc.coverage.github_prs, None));
    println!("  GitHub issues     {}", fmt_coverage(&doc.coverage.github_issues, None));
    println!("  Documentation     {}", fmt_coverage(&doc.coverage.documentation, Some("co-change proximity")));
    println!("  Working tree      {}", fmt_coverage(&doc.coverage.working_tree, None));
    println!();

    // ── EVIDENCE ──────────────────────────────────────────────────────────────
    println!("EVIDENCE");
    println!("  {} commit{}  ·  {} PR{}  ·  {} issue{}  ·  {} co-change{}",
        doc.evidence.commits,   if doc.evidence.commits   == 1 { "" } else { "s" },
        doc.evidence.prs,       if doc.evidence.prs       == 1 { "" } else { "s" },
        doc.evidence.issues,    if doc.evidence.issues    == 1 { "" } else { "s" },
        doc.evidence.co_changes, if doc.evidence.co_changes == 1 { "" } else { "s" },
    );
    println!("  {} deterministic fact{}  ·  no inferred claims",
        doc.evidence.total_facts,
        if doc.evidence.total_facts == 1 { "" } else { "s" },
    );
}

fn fmt_coverage(status: &CoverageStatus, partial_note: Option<&str>) -> String {
    match status {
        CoverageStatus::Available    => "✓ available".to_string(),
        CoverageStatus::NotIngested  => "✗ not ingested".to_string(),
        CoverageStatus::CoChangeOnly => format!("△ {}", partial_note.unwrap_or("co-change only")),
        CoverageStatus::PathScoped   => format!("△ {}", partial_note.unwrap_or("path-scoped")),
    }
}
