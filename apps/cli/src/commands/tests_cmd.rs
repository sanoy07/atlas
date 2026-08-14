use anyhow::Result;
use atlas_core::compute_test_module_links;
use atlas_ir::{EvidenceClass, TestLinkageKind, TestModuleReport};
use atlas_storage::Store;

pub fn run(modules_subject: &str, path: Option<&str>, json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;
    let repo = super::discover_repo_root()?;

    let (subject, note) = super::resolve_modules_path_for_cli(modules_subject, &repo, &store)?;
    if let Some(n) = note.as_ref() {
        if !json {
            eprintln!("{n}");
        }
    }

    let report = compute_test_module_links(&subject, path, &repo, &store)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render(&report);
    }
    Ok(())
}

fn render(r: &TestModuleReport) {
    println!("TEST ↔ MODULE LINKAGE  (modules subject: {})", r.modules_subject);
    if let Some(f) = &r.path_filter {
        println!("  path filter: {}", f);
    }
    println!("  modules: {}", r.modules.join(", "));
    println!("  test files considered: {}", r.total_test_files);
    println!("  links: {}", r.total_links);
    println!();

    println!("LINKAGE RULES");
    for rule in &r.linkage_rules {
        println!("  · {}", rule);
    }
    println!();

    if r.links.is_empty() {
        println!("(no links established under documented rules)");
    } else {
        println!("LINKS");
        for link in &r.links {
            let kind = match link.linkage_kind {
                TestLinkageKind::DirectPathPrefix => "direct",
                TestLinkageKind::ConventionalTestsDir => "tests_dir",
            };
            let cls = match link.evidence_class {
                EvidenceClass::Deterministic => "DETERMINISTIC",
                EvidenceClass::Derived => "DERIVED",
            };
            println!(
                "  {}  →  {}  [{} / {}]",
                link.test_path, link.module_name, kind, cls
            );
            println!("      rule: {}", link.rule);
        }
    }

    if !r.unlinked_tests.is_empty() {
        println!();
        println!("UNLINKED TESTS  (no rule matched — not claimed as orphaned ownership)");
        for t in &r.unlinked_tests {
            println!("  {}", t);
        }
    }

    println!();
    println!("PROVENANCE");
    println!("  test classification: path heuristic only (no content analysis)");
    println!("  no ownership or expertise claim is made");
}
