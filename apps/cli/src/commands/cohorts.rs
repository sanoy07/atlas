use anyhow::Result;
use atlas_core::compute_directory_cohorts;
use atlas_ir::CohortsReport;
use atlas_storage::Store;

pub fn run(path: &str, threshold: Option<usize>, json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;
    let repo = super::discover_repo_root()?;

    let (subject, note) = super::resolve_modules_path_for_cli(path, &repo, &store)?;
    if let Some(n) = note.as_ref() {
        if !json {
            eprintln!("{n}");
        }
    }

    let report = compute_directory_cohorts(&subject, threshold, &repo, &store)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render(&report);
    }
    Ok(())
}

fn render(r: &CohortsReport) {
    println!("DIRECTORY CO-CHANGE COHORTS  (subject: {})", r.subject);
    println!("  candidate directories: {}", r.directories.len());
    println!("  co-change threshold:   ≥ {} shared commits", r.cochange_threshold);
    println!();

    println!("METHODOLOGY");
    for m in &r.methodology {
        println!("  · {}", m);
    }
    println!();

    if r.pairs.is_empty() {
        println!("PAIRS: (none)");
    } else {
        println!("PAIRS  [DETERMINISTIC — co-change commit counts]");
        println!("  {:>6}  {}  ×  {}", "count", "dir_a", "dir_b");
        for p in &r.pairs {
            let mark = if p.cochange_commit_count >= r.cochange_threshold {
                "*"
            } else {
                " "
            };
            println!(
                " {}{:>5}  {}  ×  {}",
                mark, p.cochange_commit_count, p.directory_a, p.directory_b
            );
        }
        println!("  (* = meets threshold {})", r.cochange_threshold);
    }

    println!();
    if r.cohorts.is_empty() {
        println!("COHORTS: (none above threshold)");
    } else {
        println!("COHORTS  [DERIVED — connected components of threshold edges]");
        for (i, c) in r.cohorts.iter().enumerate() {
            println!(
                "  #{}  members=[{}]  min_edge={}  total_edge={}",
                i + 1,
                c.members.join(", "),
                c.min_edge_cochange,
                c.total_edge_cochange
            );
        }
    }

    if !r.singletons.is_empty() {
        println!();
        println!("SINGLETONS  (no pair ≥ threshold — listed, not discarded)");
        for s in &r.singletons {
            println!("  {}", s);
        }
    }

    println!();
    println!("Language: co-change cohorts are NOT business-domain clusters");
}
