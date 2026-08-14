use anyhow::Result;
use atlas_core::detect_peer_structure;
use atlas_ir::PeerStructureReport;
use atlas_storage::Store;

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

    let report = detect_peer_structure(&subject, &repo, &store)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render(&report);
    }
    Ok(())
}

fn render(r: &PeerStructureReport) {
    println!("PEER STRUCTURE  (subject: {})", r.subject);
    println!("  peer_parent: {}", r.peer_parent);
    println!("  PEERS: {}", r.peers.len());
    for p in &r.peers {
        println!("    {}", p);
    }

    if r.peers.is_empty() {
        println!();
        println!("No peer directories found under `{}`.", r.peer_parent);
        return;
    }

    if !r.patterns.is_empty() {
        println!();
        println!("REPEATED STRUCTURAL PATTERNS   (prevalence ≥ 2, sorted; denominator = {} peers)",
            r.peers.len());
        // Column widths: element name padded to a reasonable width.
        let max_elem = r.patterns.iter().map(|p| p.element.chars().count()).max().unwrap_or(0);
        let width = max_elem.max(24);
        for pat in &r.patterns {
            println!("  {:<width$}   {}/{}",
                pat.element, pat.prevalence_num, pat.prevalence_den,
                width = width);
        }
    }

    if !r.singletons.is_empty() {
        println!();
        println!("SINGLETONS   (element present in exactly 1 peer)");
        for pat in &r.singletons {
            let owner = pat.present_in.first().map(|s| s.as_str()).unwrap_or("?");
            println!("  {}   [only in {}]", pat.element, owner);
        }
    }

    if !r.deviations.is_empty() {
        println!();
        println!("DEVIATIONS  (peer lacks element present in > {}/{} of peers — deviation threshold {}/{})",
            r.peers.len() / 2, r.peers.len(),
            r.deviation_threshold_num, r.deviation_threshold_den);
        let mut current_peer = "";
        for dev in &r.deviations {
            if dev.peer != current_peer {
                println!("  {}", dev.peer);
                current_peer = dev.peer.as_str();
            }
            println!("    missing {}", dev.element);
            println!("    peer prevalence: {}/{}", dev.peer_prevalence_num, dev.peer_prevalence_den);
        }
    }

    if let Some(note) = &r.low_complexity_note {
        println!();
        println!("LOW-COMPLEXITY PEERS   [DERIVED — file count < {}]", note.file_count_threshold);
        println!("  (still counted in the peer denominator above; surfaced separately for context)");
        for (peer, n) in &note.low_complexity_peers {
            println!("  {:<20} {} files", peer, n);
        }
    }

    println!();
    println!("PROVENANCE");
    println!("  data source:      `files` table (existence only — no structural or historical evidence used)");
    println!("  peer set:         immediate child directories of peer_parent");
    println!("  denominator:      peers.len() = {} (full peer set; no exclusions)", r.peers.len());
    println!("  deviation rule:   present count * {} > {} * peer count  (strict majority when {}/{}=1/2)",
        r.deviation_threshold_den, r.deviation_threshold_num,
        r.deviation_threshold_num, r.deviation_threshold_den);
}
