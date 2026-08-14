use anyhow::Result;
use atlas_core::compute_modules;
use atlas_ir::ModulesReport;
use atlas_storage::Store;

pub fn run(path: &str, json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;
    let repo = super::discover_repo_root()?;

    let (subject, note) = super::resolve_modules_path_for_cli(path, &repo, &store)?;
    if let Some(n) = note.as_ref() {
        if !json {
            eprintln!("{n}");
        }
    }

    let report = compute_modules(&subject, &repo, &store)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render(&report);
    }
    Ok(())
}

fn render(r: &ModulesReport) {
    println!("MODULES  (subject: {})", r.subject);
    println!("  discovery: {}", r.discovery_rule);
    println!("  total: {} module director{}", r.total_modules, if r.total_modules == 1 { "y" } else { "ies" });
    println!();

    if r.modules.is_empty() {
        println!("(no immediate child directories under `{}` in the files table)", r.subject);
        return;
    }

    println!(
        "  {:<16} {:>6} {:>8} {:>8} {:>8}  {:>5}  subdirs",
        "module", "files", "commits", "out", "in", "tests"
    );
    for m in &r.modules {
        let tests = if m.has_associated_tests { "yes" } else { "no" };
        let subs = if m.subdirectories.is_empty() {
            "-".to_string()
        } else {
            m.subdirectories.join(",")
        };
        println!(
            "  {:<16} {:>6} {:>8} {:>8} {:>8}  {:>5}  {}",
            m.name,
            m.file_count,
            m.observed_commit_count,
            m.outgoing_edge_count,
            m.incoming_edge_count,
            tests,
            subs
        );
    }

    println!();
    println!("PROVENANCE");
    println!("  file_count / subdirectories: DETERMINISTIC from `files` table");
    println!("  observed_commit_count:       DETERMINISTIC commits ⨝ commit_files (prefix)");
    println!("  edge counts:                 DETERMINISTIC structural_edges (prefix)");
    println!("  has_associated_tests:        DERIVED — see each module's test_association_rule");
    println!("  Language: module names are path segments — not business-domain labels");
}
