use anyhow::Result;
use atlas_core::inspect;
use atlas_ir::{
    InspectionCoverage, InspectionDocument, InspectionEdge, InspectionSubjectKind, TreeNodeKind,
};
use atlas_storage::Store;
use chrono::{DateTime, Utc};

pub fn run(path: &str, json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store   = Store::open(&db_path)?;
    let repo    = super::discover_repo_root()?;

    let doc = inspect(path, &repo, &store)?;
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

fn header_label(doc: &InspectionDocument) -> String {
    let path = if doc.relative_path.is_empty() { ".".to_string() } else { doc.relative_path.clone() };
    let kind = match doc.kind {
        InspectionSubjectKind::File      => "file",
        InspectionSubjectKind::Directory => "directory",
    };
    let mut label = format!("{}  [{}]", path, kind);
    if !doc.exists_on_disk {
        label.push_str("  (not present on disk)");
    }
    label
}

fn render(doc: &InspectionDocument) {
    println!("ATLAS INSPECT");
    println!("{}", header_label(doc));
    if let Some(role) = &doc.role {
        println!("Role: {:?}", role);
    }
    println!();

    // ── IDENTITY (file only) ────────────────────────────────────────────
    if let Some(id) = &doc.identity {
        println!("IDENTITY");
        if id.is_historical_path {
            if let Some(cp) = &id.current_path {
                println!("  ! Historical path — current: {}", cp);
            }
        }
        match &id.first_commit {
            None => println!("  Introduced:    (unknown)"),
            Some(c) => println!("  Introduced:    {}  {}  {}", fmt_date(c.timestamp), c.short_hash, c.message),
        }
        if let Some(c) = &id.last_commit {
            println!("  Last changed:  {}  {}  {}", fmt_date(c.timestamp), c.short_hash, c.message);
        }
        println!("  Total touches: {}", id.touch_count);
        println!();
    }

    // ── STRUCTURE (directory only) ───────────────────────────────────────
    if matches!(doc.kind, InspectionSubjectKind::Directory) && !doc.children.is_empty() {
        println!("CONTAINS ({} immediate {})", doc.children.len(),
            if doc.children.len() == 1 { "child" } else { "children" });
        for child in &doc.children {
            let label = match child.kind {
                TreeNodeKind::Directory => format!("{}/", child.name),
                TreeNodeKind::File      => child.name.clone(),
            };
            println!("  {}", label);
        }
        println!();
    }

    // ── RECENT ACTIVITY ─────────────────────────────────────────────────
    if !doc.recent_activity.is_empty() {
        let shown = doc.recent_activity.iter().take(5);
        let header = if doc.touch_count as usize > doc.recent_activity.len() {
            format!("RECENT ACTIVITY ({} commits, showing 5 most recent)", doc.touch_count)
        } else if doc.recent_activity.len() > 5 {
            format!("RECENT ACTIVITY ({} commits, showing 5 most recent)", doc.recent_activity.len())
        } else {
            format!("RECENT ACTIVITY ({} commit{})", doc.recent_activity.len(),
                if doc.recent_activity.len() == 1 { "" } else { "s" })
        };
        println!("{}", header);
        for c in shown {
            println!("  {}  {}  {}", c.short_hash, fmt_date(c.timestamp), c.message);
        }
        println!();
    } else if matches!(doc.kind, InspectionSubjectKind::Directory) && doc.exists_on_disk {
        println!("RECENT ACTIVITY");
        println!("  (no commits recorded for this subtree)");
        println!();
    }

    // ── RELATED HISTORY ─────────────────────────────────────────────────
    let has_history = !doc.related_history.pull_requests.is_empty()
        || !doc.related_history.issues.is_empty();
    if has_history {
        println!("RELATED HISTORY");
        for pr in doc.related_history.pull_requests.iter().take(10) {
            println!("  PR #{}  {}  [{}]", pr.number, pr.title, pr.state.to_uppercase());
            if !pr.linked_issues.is_empty() {
                let nums: Vec<String> = pr.linked_issues.iter().map(|n| format!("#{n}")).collect();
                println!("      closes: {}", nums.join(", "));
            }
        }
        for issue in doc.related_history.issues.iter().take(10) {
            println!("  Issue #{}  {}  [{}]", issue.number, issue.title, issue.state.to_uppercase());
        }
        println!();
    }

    // ── HOT FILES WITHIN (directory only) ────────────────────────────────
    if !doc.hot_files_within.is_empty() {
        println!("HOT FILES WITHIN");
        for entry in &doc.hot_files_within {
            println!("  {:>4}×  {}", entry.change_count, entry.file_path);
        }
        println!();
    }

    // ── STRUCTURAL EDGES ────────────────────────────────────────────────
    let internal_n   = doc.structural_internal.len();
    let depends_n    = doc.structural_depends_on.len();
    let used_by_n    = doc.structural_used_by.len();
    if internal_n + depends_n + used_by_n > 0 {
        println!("STRUCTURAL EDGES");
        println!(
            "  Depends on:  {} boundary edge{}  →  {} distinct external target{}",
            depends_n, plural(depends_n),
            distinct_files(&doc.structural_depends_on, /*use_target*/ true),
            plural(distinct_files(&doc.structural_depends_on, true)),
        );
        println!(
            "  Used by:     {} boundary edge{}  ←  {} distinct external source{}",
            used_by_n, plural(used_by_n),
            distinct_files(&doc.structural_used_by, /*use_target*/ false),
            plural(distinct_files(&doc.structural_used_by, false)),
        );
        println!(
            "  Internal:    {} edge{} within the subtree (cohesion signal only, not listed)",
            internal_n, plural(internal_n),
        );
        println!();

        if depends_n > 0 {
            println!("DEPENDS ON");
            for e in doc.structural_depends_on.iter().take(20) {
                println!("  {}  →  {}{}",
                    e.source_file, e.target_file,
                    e.target_symbol.as_deref().map(|s| format!("::{}", s)).unwrap_or_default());
                println!("      {}", e.kind);
            }
            if depends_n > 20 {
                println!("  … and {} more", depends_n - 20);
            }
            println!();
        }

        if used_by_n > 0 {
            println!("USED BY");
            for e in doc.structural_used_by.iter().take(20) {
                println!("  {}  →  {}{}",
                    e.source_file, e.target_file,
                    e.target_symbol.as_deref().map(|s| format!("::{}", s)).unwrap_or_default());
                println!("      {}", e.kind);
            }
            if used_by_n > 20 {
                println!("  … and {} more", used_by_n - 20);
            }
            println!();
        }
    }

    // ── COUPLING (file only, from build_context) ─────────────────────────
    if !doc.coupling.is_empty() {
        println!("HISTORICAL COUPLING");
        for entry in doc.coupling.iter().take(10) {
            println!("  {:>3}×  {}", entry.change_count, entry.file_path);
        }
        println!();
    }

    // ── DOCUMENTS ───────────────────────────────────────────────────────
    if !doc.documents.is_empty() {
        println!("DOCUMENTS INSIDE SUBJECT");
        for d in &doc.documents {
            println!("  [{}]  {}", d.doc_type, d.file_path);
            if d.title != d.file_path && !d.title.is_empty() {
                println!("        {}", d.title);
            }
        }
        println!();
    }

    // ── PROFILE ─────────────────────────────────────────────────────────
    if !doc.profile_claims.is_empty() {
        println!("PROFILE");
        for c in &doc.profile_claims {
            println!("  {:?}: {}", c.kind, c.value);
        }
        println!();
    }

    // ── COVERAGE ────────────────────────────────────────────────────────
    println!("COVERAGE");
    print_coverage(&doc.coverage);
}

fn plural(n: usize) -> &'static str { if n == 1 { "" } else { "s" } }

fn distinct_files(edges: &[InspectionEdge], use_target: bool) -> usize {
    let mut set = std::collections::HashSet::new();
    for e in edges {
        set.insert(if use_target { e.target_file.as_str() } else { e.source_file.as_str() });
    }
    set.len()
}

fn print_coverage(c: &InspectionCoverage) {
    let mark = |b: bool| if b { "✓" } else { "✗" };
    println!("  {} Git history        {} Structural edges   {} Documentation",
        mark(c.git_history), mark(c.structural_edges), mark(c.documentation));
    println!("  {} GitHub PRs         {} GitHub issues      {} Profile claims",
        mark(c.github_prs), mark(c.github_issues), mark(c.profile_claims));
    println!("  {} Working tree", mark(c.working_tree));
}
