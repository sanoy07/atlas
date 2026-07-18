use atlas_ir::InvestigationDocument;
use std::io::Write;
use std::process::{Command, Stdio};

/// Build a structured evidence prompt from an InvestigationDocument.
/// Keeps to the facts Atlas observed — no guessing instructions, no filler.
fn build_prompt(doc: &InvestigationDocument) -> String {
    let mut p = String::new();

    // Core files
    if !doc.core_candidates.is_empty() {
        p.push_str(&format!("CORE FILES ({}):\n", doc.core_candidates.len()));
        for c in &doc.core_candidates {
            p.push_str(&format!("  {}\n", short_name(&c.file)));
        }
        p.push('\n');
    }

    // Structural connections — calls and model references only (skip imports)
    let mut edges: Vec<String> = Vec::new();
    for obs in &doc.observed_structure {
        for e in &obs.outgoing {
            if e.kind == "imports" {
                continue;
            }
            let sym = e.symbol.as_deref().unwrap_or("");
            let sym_part = if sym.is_empty() {
                String::new()
            } else {
                format!("::{}", sym)
            };
            edges.push(format!(
                "  {} → [{}] {}{}",
                short_name(&obs.file),
                e.kind.to_uppercase(),
                short_name(&e.file),
                sym_part,
            ));
        }
    }
    if !edges.is_empty() {
        p.push_str("STRUCTURAL CONNECTIONS:\n");
        for e in &edges {
            p.push_str(e);
            p.push('\n');
        }
        p.push('\n');
    }

    // Documentary — top 5 by number (highest number = most recent)
    let mut docs = doc.documentary.clone();
    docs.sort_by(|a, b| b.number.cmp(&a.number));
    let recent: Vec<_> = docs.iter().take(5).collect();
    if !recent.is_empty() {
        p.push_str(&format!(
            "DOCUMENTARY EVIDENCE ({} total, showing {} most recent):\n",
            doc.documentary.len(),
            recent.len()
        ));
        for d in &recent {
            let kind = if d.kind == "pr" { "PR" } else { "Issue" };
            p.push_str(&format!("  {} #{}: {}\n", kind, d.number, d.title));
        }
        p.push('\n');
    }

    // Unresolved connections
    if !doc.unresolved.is_empty() {
        p.push_str(&format!(
            "UNRESOLVED ({} files — documentary backing but no structural edges):\n",
            doc.unresolved.len()
        ));
        for u in &doc.unresolved {
            p.push_str(&format!("  {}\n", short_name(&u.subject)));
            if let Some(ref ind) = u.documentary_indication {
                p.push_str(&format!("    → {}\n", ind));
            }
        }
        p.push('\n');
    }

    p.push_str(
        "Respond in EXACTLY this format. Use these exact section headers:\n\n\
         WHAT THIS DOES\n\
         [2-3 sentences describing what this code domain does, from the evidence only]\n\n\
         KEY BEHAVIORS\n\
         [bullet list using · of specific behaviors visible in the structural connections]\n\n\
         RECENT CHANGES\n\
         [1 sentence about the most recent documentary evidence]\n\n\
         GAPS\n\
         [Only include if there are UNRESOLVED items. Omit this section entirely if none.]\n",
    );

    p
}

/// Last path component — "src/modules/core/services/listing.service.ts" → "listing.service.ts"
fn short_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Send the InvestigationDocument to the local Ollama model and return the synthesis.
/// Returns None if Ollama is not running, the model is unavailable, or the call times out.
pub fn synthesize(doc: &InvestigationDocument) -> Option<String> {
    let user_content = build_prompt(doc);

    let payload = serde_json::json!({
        "model": "qwen2.5-coder:7b-instruct",
        "messages": [
            {
                "role": "system",
                "content": "You analyze software engineering evidence collected by Atlas, \
                            a deterministic code intelligence tool. \
                            Output only what the provided evidence supports. \
                            Do not read file contents, do not guess behavior beyond what \
                            structural connections explicitly show."
            },
            {
                "role": "user",
                "content": user_content
            }
        ],
        "stream": false,
        "options": {
            "temperature": 0.1,
            "num_predict": 600
        }
    });

    let payload_str = serde_json::to_string(&payload).ok()?;

    let mut child = Command::new("curl")
        .args([
            "-s",
            "-m", "90",
            "-X", "POST",
            "http://localhost:11434/api/chat",
            "-H", "Content-Type: application/json",
            "--data-binary", "@-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload_str.as_bytes()).ok()?;
    }

    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }

    let body = String::from_utf8(output.stdout).ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;

    // /api/chat response: { "message": { "content": "..." } }
    let text = json["message"]["content"].as_str()?.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}
