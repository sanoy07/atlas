use anyhow::Result;
use atlas_storage::{SiblingEdgeRow, Store};
use std::collections::{HashMap, HashSet};

pub fn run(file: &str, reverse: bool) -> Result<()> {
    let db_path = std::env::var("ATLAS_DB").unwrap_or_else(|_| "./atlas.db".to_string());
    let store   = Store::open(&db_path)?;
    let repo    = super::discover_repo_root()?;

    let outgoing = store.structural_edges_for_file(file, &repo)?;
    let incoming = if reverse {
        store.structural_edges_targeting(file, &repo)?
    } else {
        vec![]
    };

    if outgoing.is_empty() && incoming.is_empty() {
        println!("No structural edges found for '{file}'.");
        println!("Hint: run `atlas ingest . --typescript` first.");
        return Ok(());
    }

    println!("STRUCTURAL OBSERVATION");
    println!("{file}");
    println!();

    // ── Outgoing edges ────────────────────────────────────────────────────────
    let imports:        Vec<_> = outgoing.iter().filter(|e| e.kind == "imports").collect();
    let calls:          Vec<_> = outgoing.iter().filter(|e| e.kind == "calls_static").collect();
    let instance_calls: Vec<_> = outgoing.iter().filter(|e| e.kind == "calls_instance").collect();
    let model_refs:     Vec<_> = outgoing.iter().filter(|e| e.kind == "references_model").collect();

    if !imports.is_empty() {
        println!("IMPORTS");
        for e in &imports {
            println!("  → {}", e.target_file);
        }
        println!();
    }

    if !calls.is_empty() {
        println!("CALLS_STATIC");
        for e in &calls {
            let sym = e.target_symbol.as_deref().unwrap_or("?");
            let loc = e.evidence_line.map(|l| format!(":{l}")).unwrap_or_default();
            println!("  → {}  ({}{})", sym, e.target_file, loc);
        }
        println!();
    }

    if !instance_calls.is_empty() {
        println!("CALLS_INSTANCE");
        for e in &instance_calls {
            let sym = e.target_symbol.as_deref().unwrap_or("?");
            let loc = e.evidence_line.map(|l| format!(":{l}")).unwrap_or_default();
            println!("  → {}  ({}{})", sym, e.target_file, loc);
        }
        println!();
    }

    if !model_refs.is_empty() {
        println!("REFERENCES_MODEL");
        for e in &model_refs {
            let sym = e.target_symbol.as_deref().unwrap_or("?");
            let loc = e.evidence_line.map(|l| format!(":{l}")).unwrap_or_default();
            println!("  → {}  ({}{})", sym, e.target_file, loc);
        }
        println!();
    }

    // ── Peer observations ─────────────────────────────────────────────────────
    // Compare this file's edges against sibling files sharing the same directory
    // and compound suffix (e.g. *.service.ts). Surfaces edges that ≥50% of peers
    // have but this file lacks. Only applies to compound suffixes — plain .ts/.js
    // would match too broadly.
    if let Some((dir_prefix, suffix)) = peer_pattern(file) {
        if suffix.contains('.') && suffix != ".ts" && suffix != ".js" {
            let like_pattern = format!("{}%{}", dir_prefix, suffix);

            let import_rows = store.sibling_edges_by_pattern(&repo, &like_pattern, file, "imports")?;
            let scall_rows  = store.sibling_edges_by_pattern(&repo, &like_pattern, file, "calls_static")?;
            let icall_rows  = store.sibling_edges_by_pattern(&repo, &like_pattern, file, "calls_instance")?;
            let model_rows  = store.sibling_edges_by_pattern(&repo, &like_pattern, file, "references_model")?;

            // Peer count = union of all sibling source files seen across any kind.
            let mut seen_peers: HashSet<String> = HashSet::new();
            for r in import_rows.iter()
                .chain(&scall_rows)
                .chain(&icall_rows)
                .chain(&model_rows)
            {
                seen_peers.insert(r.sibling_file.clone());
            }
            let peer_count = seen_peers.len();

            if peer_count >= 2 {
                let own_import_files: HashSet<&str> =
                    imports.iter().map(|e| e.target_file.as_str()).collect();
                let import_gaps = file_target_gaps(&import_rows, peer_count, &own_import_files);

                let own_scall_keys: HashSet<(String, String)> = calls.iter()
                    .map(|e| (e.target_file.clone(), e.target_symbol.as_deref().unwrap_or("").to_string()))
                    .collect();
                let scall_gaps = call_target_gaps(&scall_rows, peer_count, &own_scall_keys);

                let own_icall_keys: HashSet<(String, String)> = instance_calls.iter()
                    .map(|e| (e.target_file.clone(), e.target_symbol.as_deref().unwrap_or("").to_string()))
                    .collect();
                let icall_gaps = call_target_gaps(&icall_rows, peer_count, &own_icall_keys);

                let own_model_files: HashSet<&str> =
                    model_refs.iter().map(|e| e.target_file.as_str()).collect();
                let model_gaps = file_target_gaps(&model_rows, peer_count, &own_model_files);

                if !import_gaps.is_empty() || !scall_gaps.is_empty()
                    || !icall_gaps.is_empty() || !model_gaps.is_empty()
                {
                    let dir_display = dir_prefix.trim_end_matches('/');
                    println!("PEER OBSERVATIONS");
                    println!("  Family: *{}  ({} peers in {})", suffix, peer_count, dir_display);
                    println!();

                    if !import_gaps.is_empty() {
                        println!("  Imports present in peers but absent here:");
                        for (target, count) in &import_gaps {
                            println!("  ✗ {}  ({} of {} peers)", target, count, peer_count);
                        }
                        println!();
                    }

                    if !scall_gaps.is_empty() {
                        println!("  Static calls present in peers but absent here:");
                        for (display, count) in &scall_gaps {
                            println!("  ✗ {}  ({} of {} peers)", display, count, peer_count);
                        }
                        println!();
                    }

                    if !icall_gaps.is_empty() {
                        println!("  Instance calls present in peers but absent here:");
                        for (display, count) in &icall_gaps {
                            println!("  ✗ {}  ({} of {} peers)", display, count, peer_count);
                        }
                        println!();
                    }

                    if !model_gaps.is_empty() {
                        println!("  Model references present in peers but absent here:");
                        for (target, count) in &model_gaps {
                            println!("  ✗ {}  ({} of {} peers)", target, count, peer_count);
                        }
                        println!();
                    }
                }
            }
        }
    }

    // ── Incoming edges (reverse) ──────────────────────────────────────────────
    if !incoming.is_empty() {
        println!("IMPORTED BY");
        for e in &incoming {
            println!("  ← {}", e.source_file);
        }
        println!();
    }

    // ── Coverage ──────────────────────────────────────────────────────────────
    println!("COVERAGE");
    println!("  ES imports      ✓ analyzed  (typescript-es-import)");
    println!("  Static calls    ✓ analyzed  (typescript-static-call)");
    println!("  Instance calls  ✓ analyzed  (typescript-instance-call)");
    println!("  Model refs      ✓ analyzed  (typescript-mongoose-ref)");
    println!("  Dynamic calls   ✗ not analyzed");
    println!("  Runtime DI      ✗ not analyzed");

    Ok(())
}

/// Gap analysis keyed by target file (used for imports and model references).
/// Returns sorted (target_file, count) pairs present in ≥50% of peers but absent here.
/// Counts distinct sibling files per target — a single peer referencing the same
/// model via multiple methods (findById, findOne, …) still counts as 1.
fn file_target_gaps(
    sibling_rows: &[SiblingEdgeRow],
    peer_count: usize,
    own_files: &HashSet<&str>,
) -> Vec<(String, usize)> {
    let mut sibling_sets: HashMap<String, HashSet<&str>> = HashMap::new();
    for row in sibling_rows {
        sibling_sets
            .entry(row.target_file.clone())
            .or_default()
            .insert(&row.sibling_file);
    }
    let mut gaps: Vec<(String, usize)> = sibling_sets
        .into_iter()
        .map(|(target, siblings)| (target, siblings.len()))
        .filter(|(target, count)| {
            *count >= 2
                && *count * 2 >= peer_count
                && !target.starts_with("UNRESOLVED:external:")
                && !own_files.contains(target.as_str())
        })
        .collect();
    gaps.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    gaps
}

/// Gap analysis keyed by (target_file, target_symbol) (used for static and instance calls).
/// Returns sorted ("symbol  (file)", count) pairs present in ≥50% of peers but absent here.
/// Counts distinct sibling files per (target, symbol) pair.
fn call_target_gaps(
    sibling_rows: &[SiblingEdgeRow],
    peer_count: usize,
    own_keys: &HashSet<(String, String)>,
) -> Vec<(String, usize)> {
    let mut sibling_sets: HashMap<(String, String), HashSet<String>> = HashMap::new();
    for row in sibling_rows {
        let key = (
            row.target_file.clone(),
            row.target_symbol.as_deref().unwrap_or("").to_string(),
        );
        sibling_sets.entry(key).or_default().insert(row.sibling_file.clone());
    }
    let mut gaps: Vec<(String, usize)> = sibling_sets
        .into_iter()
        .filter_map(|((file, sym), siblings)| {
            let count = siblings.len();
            if count >= 2
                && count * 2 >= peer_count
                && !file.starts_with("UNRESOLVED:external:")
                && !own_keys.contains(&(file.clone(), sym.clone()))
            {
                let display = if sym.is_empty() {
                    file
                } else {
                    format!("{}  ({})", sym, file)
                };
                Some((display, count))
            } else {
                None
            }
        })
        .collect();
    gaps.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    gaps
}

/// Extract (directory_prefix, compound_suffix) from a file path.
///
/// For `src/modules/core/services/currency.service.ts`:
///   dir_prefix = "src/modules/core/services/"
///   suffix     = ".service.ts"
///
/// Returns `None` if the filename has no directory component or no dot.
fn peer_pattern(file: &str) -> Option<(String, String)> {
    let slash = file.rfind('/')?;
    let dir_prefix = &file[..=slash];              // "src/…/services/"
    let filename   = &file[slash + 1..];           // "currency.service.ts"
    let dot        = filename.find('.')?;
    let suffix     = &filename[dot..];             // ".service.ts"
    Some((dir_prefix.to_string(), suffix.to_string()))
}
