use anyhow::Result;
use chrono::Utc;
use std::io::{self, Write};
use std::fs::OpenOptions;

pub fn run(file: &str) -> Result<()> {
    let log_path = std::env::var("ATLAS_FRICTION_LOG")
        .unwrap_or_else(|_| "./atlas-friction.jsonl".to_string());

    println!("Atlas friction log — {file}");
    println!("Press Enter to skip any field.\n");

    let useful         = prompt("Useful:          ")?;
    let missing        = prompt("Missing:         ")?;
    let noise          = prompt("Noise:           ")?;
    let action_changed = prompt("Action changed:  ")?;
    let notes          = prompt("Notes:           ")?;

    let entry = serde_json::json!({
        "timestamp":      Utc::now().to_rfc3339(),
        "subject":        file,
        "useful":         useful,
        "missing":        missing,
        "noise":          noise,
        "action_changed": action_changed,
        "notes":          notes,
    });

    let mut log = OpenOptions::new().create(true).append(true).open(&log_path)?;
    writeln!(log, "{}", entry)?;

    println!("\nLogged to {log_path}");
    Ok(())
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}
