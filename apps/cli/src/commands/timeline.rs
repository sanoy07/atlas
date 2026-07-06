use anyhow::Result;
use atlas_storage::Store;
use chrono::{DateTime, Utc};

pub fn run(file: &str) -> Result<()> {
    let db_path = std::env::var("ATLAS_DB").unwrap_or_else(|_| "./atlas.db".to_string());
    let store = Store::open(&db_path)?;

    let commits = store.commits_for_file_asc(file, ".")?;
    let prs     = store.prs_for_file(file, ".")?;
    let issues  = store.issues_for_file(file, ".")?;

    if commits.is_empty() && prs.is_empty() && issues.is_empty() {
        println!("No data found for '{}'.", file);
        println!("Hint: run `atlas ingest .` first.");
        return Ok(());
    }

    println!("Timeline for {}\n{}\n", file, "─".repeat(60));

    // ── Issues (motivation) ─────────────────────────────────────────────────
    if !issues.is_empty() {
        println!("Issues:");
        for issue in &issues {
            let ts = issue.created_at.map(fmt_ts).unwrap_or_else(|| "date unknown".into());
            println!("  [{ts}] #{} [{}] {}", issue.number, issue.state, issue.title);
        }
        println!();
    }

    // ── Commits (execution) ─────────────────────────────────────────────────
    if !commits.is_empty() {
        println!("Commits (oldest → newest):");

        // Build a map from merge_commit_sha short hash → PR numbers for annotation.
        let merge_commit_prs: std::collections::HashMap<String, Vec<i64>> = {
            let mut m: std::collections::HashMap<String, Vec<i64>> = std::collections::HashMap::new();
            for pr in &prs {
                if let Some(sha) = &pr.merge_commit_sha {
                    let short = sha.chars().take(7).collect::<String>();
                    m.entry(short).or_default().push(pr.number);
                }
            }
            m
        };

        for c in &commits {
            let ts = fmt_ts(c.timestamp);
            let annotation = merge_commit_prs.get(&c.short_hash)
                .map(|prs| {
                    let ids: Vec<String> = prs.iter().map(|n| format!("#{n}")).collect();
                    format!("  ← merge commit for PR {}", ids.join(", "))
                })
                .unwrap_or_default();

            println!("  [{ts}] {} — {} ({}){annotation}",
                c.short_hash, c.message, c.author_name);
        }
        println!();
    }

    // ── PRs (delivery) ──────────────────────────────────────────────────────
    if !prs.is_empty() {
        println!("Pull Requests:");
        for pr in &prs {
            let ts = pr.merged_at
                .or(pr.created_at)
                .map(fmt_ts)
                .unwrap_or_else(|| "date unknown".into());

            let closed: Vec<String> = issues.iter()
                .map(|i| format!("#{}", i.number))
                .collect();
            let closes_note = if closed.is_empty() {
                String::new()
            } else {
                format!("  closes {}", closed.join(", "))
            };

            println!("  [{ts}] #{} [{}] {}{closes_note}",
                pr.number, pr.state, pr.title);
        }
    }

    Ok(())
}

fn fmt_ts(unix: i64) -> String {
    DateTime::<Utc>::from_timestamp(unix, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| unix.to_string())
}
