use anyhow::Result;
use atlas_core::{compute_config_inventory, compute_config_provenance};
use atlas_ir::{ConfigArtifactReport, ConfigInventoryReport};
use atlas_storage::Store;
use chrono::{DateTime, Utc};

pub fn run(path: Option<&str>, json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;
    let repo = super::discover_repo_root()?;

    if let Some(p) = path {
        let report = compute_config_provenance(p, &repo, &store)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            render_one(&report);
        }
    } else {
        let report = compute_config_inventory(&repo, &store)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            render_inventory(&report);
        }
    }
    Ok(())
}

fn render_inventory(r: &ConfigInventoryReport) {
    println!("CONFIGURATION ARTIFACTS");
    println!("  total: {}", r.total_artifacts);
    for p in &r.provenance {
        println!("  · {}", p);
    }
    println!();

    if r.artifacts.is_empty() {
        println!("(no configuration_artifacts rows for this repository)");
        println!("Run `atlas ingest .` to capture recognised root config files.");
        return;
    }

    println!("  {:<28} {:<18} {:>8}  {}", "path", "kind", "commits", "sha256");
    for a in &r.artifacts {
        println!(
            "  {:<28} {:<18} {:>8}  {}",
            a.file_path,
            a.artifact_kind,
            a.touching_commit_count,
            &a.sha256[..12.min(a.sha256.len())]
        );
    }
}

fn render_one(r: &ConfigArtifactReport) {
    if let Some(rn) = &r.redirect_note {
        eprintln!(
            "note: `{}` is a historical path — showing config provenance for `{}` (identity {})",
            rn.original_subject, rn.current_path, rn.identity_id
        );
    }

    println!("CONFIGURATION PROVENANCE  (path: {})", r.file_path);
    println!(
        "  artifact_present: {}",
        if r.artifact_present { "yes" } else { "no" }
    );
    if let Some(k) = &r.artifact_kind {
        println!("  artifact_kind:    {}", k);
    }
    if let Some(s) = &r.sha256 {
        println!("  sha256:           {}", s);
    }
    if let Some(t) = r.ingested_at {
        println!("  ingested_at:      {} ({})", t, format_ts(t));
    }
    if let Some(n) = r.content_byte_len {
        println!("  content_bytes:    {}", n);
    }
    if let Some(id) = r.identity_id {
        println!("  file_identity_id: {}", id);
    }
    if let Some(n) = r.identity_commit_count {
        println!("  identity commits: {}", n);
    }

    println!();
    println!(
        "TOUCHING COMMITS  ({})  [DETERMINISTIC — commits ⨝ commit_files / identity]",
        r.touching_commit_count
    );
    if r.touching_commits.is_empty() {
        println!("  (none observed)");
    } else {
        if let (Some(f), Some(l)) = (r.first_touch, r.last_touch) {
            println!("  first_touch: {}  last_touch: {}", format_ts(f), format_ts(l));
        }
        for c in r.touching_commits.iter().take(20) {
            println!(
                "  {}  {}  {}  {}",
                &c.short_hash,
                format_ts(c.timestamp),
                c.author_name,
                truncate(&c.message, 60)
            );
        }
        if r.touching_commits.len() > 20 {
            println!("  … {} more", r.touching_commits.len() - 20);
        }
    }

    println!();
    println!("LIMITATIONS");
    for l in &r.limitations {
        println!("  · {}", l);
    }
    println!();
    println!("PROVENANCE");
    for p in &r.provenance {
        println!("  · {}", p);
    }
}

fn format_ts(unix: i64) -> String {
    match DateTime::<Utc>::from_timestamp(unix, 0) {
        Some(dt) => dt.format("%Y-%m-%d").to_string(),
        None => "-".into(),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{}…", t)
    }
}
