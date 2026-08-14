use anyhow::Result;
use atlas_core::compute_anomalies;
use atlas_ir::{AnomaliesReport, AnomalyKind, EvidenceClass};
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

    let report = compute_anomalies(&subject, &repo, &store)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render(&report);
    }
    Ok(())
}

fn render(r: &AnomaliesReport) {
    println!("ANOMALIES  (subject: {})", r.subject);
    println!("  total: {}", r.total_anomalies);
    println!();

    println!("METHODOLOGY");
    for m in &r.methodology {
        println!("  · {}", m);
    }
    println!();

    if r.anomalies.is_empty() {
        println!("(no deviations from observed patterns under documented rules)");
        return;
    }

    for a in &r.anomalies {
        let kind = match a.kind {
            AnomalyKind::PeerStructureDeviation => "peer_structure_deviation",
            AnomalyKind::MissingAssociatedTests => "missing_associated_tests",
            AnomalyKind::DeclaredDependencyUnobserved => "declared_dependency_unobserved",
        };
        let cls = match a.evidence_class {
            EvidenceClass::Deterministic => "DETERMINISTIC",
            EvidenceClass::Derived => "DERIVED",
        };
        println!("[{}]  {}  ({})", cls, kind, a.subject);
        println!("  observation: {}", a.observation);
        println!("  expected:    {}", a.expected);
        println!("  threshold:   {}", a.threshold_note);
        for e in &a.evidence {
            println!("  evidence:    {}", e);
        }
        println!();
    }

    println!("Language: 'deviates from observed peer pattern' — not 'bad architecture'");
}
