use anyhow::Result;
use atlas_core::{find_callees, find_callers, CallersReport};
use atlas_storage::Store;

pub fn run(subject: &str, callees: bool, limit: usize, json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;
    let repo = super::discover_repo_root()?;
    let report = if callees {
        find_callees(subject, &repo, &store, limit)?
    } else {
        find_callers(subject, &repo, &store, limit)?
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render(&report, callees);
    }
    Ok(())
}

fn render(r: &CallersReport, callees_mode: bool) {
    println!("ATLAS CALLERS");
    println!("  subject: {}", r.subject);
    println!("  resolved_as: {}", r.resolved_as);
    println!();

    if !callees_mode {
        println!(
            "PRODUCTION CALLERS  ({})",
            r.production_callers.len()
        );
        if r.production_callers.is_empty() {
            println!("  (none observed)");
        }
        for c in &r.production_callers {
            let sym = c
                .callee_symbol
                .as_deref()
                .or(c.caller_symbol.as_deref())
                .unwrap_or("?");
            let line = c
                .evidence_line
                .map(|l| format!(":{}", l))
                .unwrap_or_default();
            println!(
                "  ← {}  calls {}  ({}{})  [{}]",
                c.caller_file, sym, c.callee_file, line, c.kind
            );
        }
        println!();
        println!("TEST CALLERS  ({})", r.test_callers.len());
        for c in r.test_callers.iter().take(12) {
            let sym = c.callee_symbol.as_deref().unwrap_or("?");
            println!("  ← {}  → {}", c.caller_file, sym);
        }
        if r.test_callers.len() > 12 {
            println!("  … {} more", r.test_callers.len() - 12);
        }
        println!();
    }

    println!("CALLEES  ({})", r.callees.len());
    if r.callees.is_empty() {
        println!("  (none observed from subject)");
    }
    for c in &r.callees {
        let sym = c.callee_symbol.as_deref().unwrap_or("?");
        let line = c
            .evidence_line
            .map(|l| format!(":{}", l))
            .unwrap_or_default();
        println!(
            "  → {}  ({}){}  [{}]",
            sym, c.callee_file, line, c.kind
        );
    }
    println!();
    println!("LIMITATIONS");
    for l in &r.limitations {
        println!("  · {}", l);
    }
}
