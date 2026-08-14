use anyhow::Result;
use atlas_core::compute_dependency_linkage;
use atlas_ir::{DependencyLinkageReport, DependencySection};
use atlas_storage::Store;

pub fn run(json: bool, limit: usize) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;
    let repo = super::discover_repo_root()?;

    let report = compute_dependency_linkage(&repo, &store)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render(&report, limit);
    }
    Ok(())
}

fn render(r: &DependencyLinkageReport, limit: usize) {
    println!("NPM DEPENDENCY ↔ SOURCE LINKAGE");
    println!("  declaration source: {}", r.declaration_source);
    println!("  provenance:         {}", r.declaration_provenance);
    println!(
        "  declared={}  observed={}  both={}  declared-only={}  observed-undeclared={}",
        r.total_declared,
        r.total_observed,
        r.declared_and_observed,
        r.declared_unobserved,
        r.observed_undeclared
    );
    println!();

    println!("METHODOLOGY");
    for m in &r.methodology {
        println!("  · {}", m);
    }
    println!();

    let show = if limit == 0 { r.packages.len() } else { limit };
    println!("PACKAGES  (sorted by observation_count desc; showing up to {})", show);
    for p in r.packages.iter().take(show) {
        let section = match p.section {
            DependencySection::Dependencies => "dependencies",
            DependencySection::DevDependencies => "devDependencies",
            DependencySection::OptionalDependencies => "optionalDependencies",
            DependencySection::PeerDependencies => "peerDependencies",
            DependencySection::Undeclared => "undeclared",
        };
        let flags = format!(
            "{}{}",
            if p.is_declared { "DECLARED" } else { "" },
            if p.is_observed {
                if p.is_declared { "+OBSERVED" } else { "OBSERVED" }
            } else if p.is_declared {
                "+UNOBSERVED"
            } else {
                ""
            }
        );
        println!(
            "  {:<32}  {:<18}  {:<20}  obs={} files={}",
            p.package_name, section, flags, p.observation_count, p.distinct_source_files
        );
        if p.is_declared && !p.declared_version.is_empty() {
            println!("      version: {}", p.declared_version);
        }
        for o in p.observations.iter().take(5) {
            println!(
                "      · {}  --[{}]-->  {}",
                o.source_file, o.edge_kind, o.target_spec
            );
        }
        if p.observations.len() > 5 {
            println!("      … {} more observations", p.observations.len() - 5);
        }
    }
    if r.packages.len() > show {
        println!("  … {} more packages (use --json or --limit 0)", r.packages.len() - show);
    }

    println!();
    println!("PROVENANCE");
    println!("  DECLARED = package.json fields; OBSERVED = structural_edges only");
    println!("  OBSERVED is static import evidence — not a runtime-usage claim");
}
