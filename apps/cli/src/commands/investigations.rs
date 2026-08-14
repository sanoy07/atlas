use anyhow::{bail, Result};
use atlas_core;
use atlas_ir::InvestigationDocument;
use atlas_storage::Store;

pub fn run_list(limit: i64) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store   = Store::open(&db_path)?;
    let repo    = super::discover_repo_root()?;

    let records = atlas_core::list_stored_investigations(&repo, &store, limit)?;

    if records.is_empty() {
        println!("No stored investigations for this repository.");
        println!("Run `atlas investigate <anchor>` to create one.");
        return Ok(());
    }

    println!("{:>5}  {:>12}  {:20}  {}", "ID", "GIT HEAD", "RAN AT", "ANCHORS");
    println!("{}", "-".repeat(70));
    for rec in &records {
        let ts = format_timestamp(rec.ran_at);
        println!("{:>5}  {:>12}  {:20}  {}", rec.id, &rec.git_head[..rec.git_head.len().min(12)], ts, rec.anchors_key);
    }
    Ok(())
}

pub fn run_show(id: i64, json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store   = Store::open(&db_path)?;

    let Some(doc) = atlas_core::load_investigation_by_id(id, &store)? else {
        bail!("No investigation found with ID {}", id);
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&doc)?);
    } else {
        super::investigate::render_stored(&doc);
    }
    Ok(())
}

pub fn run_diff(id_a: i64, id_b: i64) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store   = Store::open(&db_path)?;

    let Some(doc_a) = atlas_core::load_investigation_by_id(id_a, &store)? else {
        bail!("No investigation found with ID {}", id_a);
    };
    let Some(doc_b) = atlas_core::load_investigation_by_id(id_b, &store)? else {
        bail!("No investigation found with ID {}", id_b);
    };

    print_diff(&doc_a, id_a, &doc_b, id_b);
    Ok(())
}

fn print_diff(a: &InvestigationDocument, id_a: i64, b: &InvestigationDocument, id_b: i64) {
    let files_a: std::collections::HashSet<&str> = a.core_candidates.iter()
        .chain(a.supporting_artifacts.iter())
        .map(|c| c.file.as_str())
        .collect();
    let files_b: std::collections::HashSet<&str> = b.core_candidates.iter()
        .chain(b.supporting_artifacts.iter())
        .map(|c| c.file.as_str())
        .collect();

    let added:   Vec<&&str> = files_b.iter().filter(|f| !files_a.contains(**f)).collect();
    let removed: Vec<&&str> = files_a.iter().filter(|f| !files_b.contains(**f)).collect();
    let kept:    Vec<&&str> = files_a.iter().filter(|f| files_b.contains(*f)).collect();

    println!("INVESTIGATION DIFF  #{} → #{}", id_a, id_b);
    println!("  Anchors:  {} → {}", a.anchors.join(", "), b.anchors.join(", "));
    println!();

    if !added.is_empty() {
        println!("ADDED ({}):", added.len());
        let mut sorted = added.clone();
        sorted.sort();
        for f in sorted { println!("  + {}", f); }
        println!();
    }

    if !removed.is_empty() {
        println!("REMOVED ({}):", removed.len());
        let mut sorted = removed.clone();
        sorted.sort();
        for f in sorted { println!("  - {}", f); }
        println!();
    }

    if added.is_empty() && removed.is_empty() {
        println!("No changes in candidate set ({} files in both).", kept.len());
    } else {
        println!("Unchanged: {} files", kept.len());
    }

    // Score changes for candidates present in both
    let score_a: std::collections::HashMap<&str, f32> = a.core_candidates.iter()
        .chain(a.supporting_artifacts.iter())
        .map(|c| (c.file.as_str(), c.score.total))
        .collect();
    let score_b: std::collections::HashMap<&str, f32> = b.core_candidates.iter()
        .chain(b.supporting_artifacts.iter())
        .map(|c| (c.file.as_str(), c.score.total))
        .collect();

    let mut score_changes: Vec<(&str, f32, f32)> = kept.iter()
        .filter_map(|&&f| {
            let sa = *score_a.get(f).unwrap_or(&0.0);
            let sb = *score_b.get(f).unwrap_or(&0.0);
            if (sa - sb).abs() > 0.01 { Some((f, sa, sb)) } else { None }
        })
        .collect();
    score_changes.sort_by(|a, b| (b.2 - b.1).abs().partial_cmp(&(a.2 - a.1).abs()).unwrap_or(std::cmp::Ordering::Equal));

    if !score_changes.is_empty() {
        println!();
        println!("SCORE CHANGES:");
        for (f, sa, sb) in &score_changes {
            let arrow = if sb > sa { "↑" } else { "↓" };
            println!("  {} {:.2} → {:.2}  {}", arrow, sa, sb, f);
        }
    }
}

fn format_timestamp(secs: i64) -> String {
    let dt = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs as u64);
    let elapsed = std::time::SystemTime::now()
        .duration_since(dt)
        .unwrap_or_default();
    let secs_elapsed = elapsed.as_secs();
    if secs_elapsed < 60 {
        "just now".to_string()
    } else if secs_elapsed < 3600 {
        format!("{}m ago", secs_elapsed / 60)
    } else if secs_elapsed < 86400 {
        format!("{}h ago", secs_elapsed / 3600)
    } else {
        format!("{}d ago", secs_elapsed / 86400)
    }
}
