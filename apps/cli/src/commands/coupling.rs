use anyhow::Result;
use atlas_core::compute_module_coupling;
use atlas_ir::ModuleCouplingReport;
use atlas_storage::Store;

/// Cap on the number of modules for which we render the ASCII matrix.
/// Above this, the matrix becomes unreadable and we skip it — the sparse
/// cell list is the canonical representation and always shown.
const MATRIX_MODULE_CAP: usize = 20;

pub fn run(path: &str, json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store   = Store::open(&db_path)?;
    let repo    = super::discover_repo_root()?;

    let (subject, note) = super::resolve_modules_path_for_cli(path, &repo, &store)?;
    if let Some(n) = note.as_ref() {
        if !json {
            eprintln!("{n}");
        }
    }

    let report = compute_module_coupling(&subject, &repo, &store)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render(&report);
    }
    Ok(())
}

fn render(r: &ModuleCouplingReport) {
    println!("MODULE COUPLING  (subject: {})", r.subject);
    println!("  candidates: {} immediate child directories", r.modules.len());
    if r.modules.is_empty() {
        println!("\nNo modules found under `{}`.", r.subject);
        return;
    }
    print!("  modules: ");
    for (i, m) in r.modules.iter().enumerate() {
        if i > 0 { print!(" · "); }
        print!("{}", m);
    }
    println!();

    // Matrix — only rendered for small module counts.
    if r.modules.len() <= MATRIX_MODULE_CAP {
        println!();
        render_matrix(r);
    } else {
        println!();
        println!("(matrix skipped — {} modules exceeds render cap of {}; use the CELL DETAIL list below or --json)",
            r.modules.len(), MATRIX_MODULE_CAP);
    }

    // Canonical: sparse cell list.
    println!();
    println!("CELL DETAIL   (sparse — one entry per non-zero source→target coupling, sorted by edge count desc)");
    if r.cells.is_empty() {
        println!("  (no cross-module edges observed)");
    } else {
        for c in &r.cells {
            println!("  {} → {}", c.source_module, c.target_module);
            println!("    {} observed structural edges", c.edge_count);
            println!("    {} distinct source file{}, {} distinct target file{}",
                c.distinct_source_files, plural(c.distinct_source_files),
                c.distinct_target_files, plural(c.distinct_target_files));
            for k in &c.kinds {
                println!("      {:<18} {}", k.kind, k.edge_count);
            }
            println!();
        }
    }

    // Derived aggregates.
    if !r.fan_out.is_empty() {
        println!("OUTGOING SUMMARY   [DERIVED — fan-out per source module]");
        println!("  {:<20} {:>8}  {}", "module", "edges", "fan-out (modules reached)");
        for f in &r.fan_out {
            println!("  {:<20} {:>8}  {}", f.module, f.edges, f.fan);
        }
        println!();
    }
    if !r.fan_in.is_empty() {
        println!("INCOMING SUMMARY   [DERIVED — fan-in per target module]");
        println!("  {:<20} {:>8}  {}", "module", "edges", "fan-in (modules reaching)");
        for f in &r.fan_in {
            println!("  {:<20} {:>8}  {}", f.module, f.edges, f.fan);
        }
        println!();
    }

    // External + platform sections.
    if !r.external_dependencies.is_empty() {
        println!("EXTERNAL DEPENDENCIES   [DETERMINISTIC — UNRESOLVED:external:* targets]");
        for e in r.external_dependencies.iter().take(30) {
            println!("  {:<15} {:<45} {} edges from {} source file{}",
                e.source_module, e.external_target,
                e.edge_count, e.distinct_source_files, plural(e.distinct_source_files));
        }
        if r.external_dependencies.len() > 30 {
            println!("  … and {} more", r.external_dependencies.len() - 30);
        }
        println!();
    }
    if !r.platform_usage.is_empty() {
        println!("PLATFORM-LAYER USAGE   [DETERMINISTIC — edges from modules to non-module repo files, aggregated by first two path segments]");
        for p in &r.platform_usage {
            println!("  {:<15} → {:<25} {} edges  ({} → {})",
                p.source_module, p.platform_target,
                p.edge_count, p.distinct_source_files, p.distinct_target_files);
        }
        println!();
    }

    println!("PROVENANCE");
    println!("  data source:      `structural_edges` table (all kinds: imports, calls_static, calls_instance, references_model)");
    println!("  candidates:       immediate child directories of subject");
    println!("  cell semantics:   directed edges from source-module files to target-module files");
    println!("  internal edges:   NOT in matrix or cells (both endpoints in the same module)");
    println!("  external policy:  UNRESOLVED:external:* reported separately");
    println!("  platform policy:  edges to non-module repo files reported separately");
    println!("  language note:    cell values are OBSERVED EDGE COUNTS — not a claim about coupling tightness");
}

fn render_matrix(r: &ModuleCouplingReport) {
    // Column headers — short prefix of each module name, min 3 chars, pad to same width.
    let col_labels: Vec<String> = r.modules.iter()
        .map(|m| short_label(m, 5))
        .collect();
    let col_w = col_labels.iter().map(|s| s.chars().count()).max().unwrap_or(3).max(3);
    let row_w = r.modules.iter().map(|s| s.chars().count()).max().unwrap_or(3);

    // Dense look-up from sparse cells for rendering.
    let mut lookup: std::collections::HashMap<(&str, &str), usize> = std::collections::HashMap::new();
    for c in &r.cells {
        lookup.insert((c.source_module.as_str(), c.target_module.as_str()), c.edge_count);
    }

    println!("MATRIX   (source ↓ target →)   cells = edge count; . = 0; - = self)");
    // Header line
    print!("  {:<row_w$}", "", row_w = row_w);
    for label in &col_labels {
        print!("  {:>col_w$}", label, col_w = col_w);
    }
    println!();

    for src in &r.modules {
        print!("  {:<row_w$}", src, row_w = row_w);
        for tgt in &r.modules {
            let cell = if src == tgt {
                "-".to_string()
            } else {
                match lookup.get(&(src.as_str(), tgt.as_str())) {
                    Some(n) => n.to_string(),
                    None    => ".".to_string(),
                }
            };
            print!("  {:>col_w$}", cell, col_w = col_w);
        }
        println!();
    }
}

/// Compact label for matrix column headers.  Prefer the first `n` chars,
/// falling back to the module name when shorter than `n`.
fn short_label(module: &str, n: usize) -> String {
    module.chars().take(n).collect()
}

fn plural(n: usize) -> &'static str { if n == 1 { "" } else { "s" } }

#[cfg(test)]
mod tests {
    use super::short_label;
    #[test]
    fn short_label_truncates() {
        assert_eq!(short_label("blockchain", 3), "blo");
        assert_eq!(short_label("core", 5), "core");
    }
}
