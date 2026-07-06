use anyhow::Result;
use atlas_storage::Store;
use chrono::{DateTime, Utc};

pub fn run(file: &str) -> Result<()> {
    let db_path = std::env::var("ATLAS_DB").unwrap_or_else(|_| "./atlas.db".to_string());
    let store = Store::open(&db_path)?;

    // Oldest-first so the reader sees the story in chronological order.
    let commits = store.commits_for_file_asc(file, ".")?;
    let prs     = store.prs_for_file(file, ".")?;
    let issues  = store.issues_for_file(file, ".")?;

    if commits.is_empty() && prs.is_empty() && issues.is_empty() {
        println!("No data found. Run `atlas ingest .` first.");
        println!("For PR and issue linkage: `atlas ingest --github .`");
        return Ok(());
    }

    println!("=== {} ===\n", file);

    // Build set of merge-commit short hashes so we can annotate commits.
    let merge_commits: std::collections::HashSet<String> = prs.iter()
        .filter_map(|pr| pr.merge_commit_sha.as_ref())
        .map(|sha| sha.chars().take(7).collect())
        .collect();

    // ── Touch history ───────────────────────────────────────────────────────
    println!("Touch history ({} commits):\n", commits.len());
    for c in &commits {
        let date = fmt_date(c.timestamp);
        let merge_note = if merge_commits.contains(&c.short_hash) {
            " [merge commit]"
        } else {
            ""
        };
        println!("  {} {date}  {} — {}{}",
            c.short_hash, c.author_name, c.message, merge_note);
    }

    // ── Pull requests ───────────────────────────────────────────────────────
    if !prs.is_empty() {
        println!("\nPull requests ({}):\n", prs.len());
        for pr in &prs {
            let date = pr.merged_at.or(pr.created_at).map(fmt_date).unwrap_or_default();
            let date_str = if date.is_empty() { String::new() } else { format!(" {date}") };
            let merge_short = pr.merge_commit_sha.as_deref()
                .map(|s| &s[..s.len().min(7)])
                .unwrap_or("?");
            println!("  #{} [{}]{date_str}  {} — {}", pr.number, pr.state, pr.title, pr.author);
            println!("       via commit {merge_short}");
        }
    }

    // ── Issues ──────────────────────────────────────────────────────────────
    if !issues.is_empty() {
        println!("\nIssues ({}):\n", issues.len());
        for issue in &issues {
            let date = issue.created_at.map(fmt_date).unwrap_or_default();
            let date_str = if date.is_empty() { String::new() } else { format!(" {date}") };
            println!("  #{} [{}]{date_str}  {}", issue.number, issue.state, issue.title);
        }
    }

    Ok(())
}

fn fmt_date(unix: i64) -> String {
    DateTime::<Utc>::from_timestamp(unix, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

