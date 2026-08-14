use atlas_core::OllamaConfig;
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

/// Read the first `max_lines` lines of a repo-relative file.
/// Returns None if the file does not exist (e.g. it is a new file the plan must create).
fn read_file_head(repo_path: &str, rel_path: &str, max_lines: usize) -> Option<String> {
    let full = format!("{}/{}", repo_path.trim_end_matches('/'), rel_path);
    let content = std::fs::read_to_string(&full).ok()?;
    let head: Vec<&str> = content.lines().take(max_lines).collect();
    Some(head.join("\n"))
}

/// Send the InvestigationDocument to the local Ollama model and return the synthesis.
/// Returns None if Ollama is not running, the model is unavailable, or the call times out.
pub fn synthesize(doc: &InvestigationDocument) -> Option<String> {
    let cfg = OllamaConfig::from_env();
    let user_content = build_prompt(doc);
    call_ollama(
        &cfg,
        &cfg.synthesis_model,
        "You analyze software engineering evidence collected by Atlas, \
         a deterministic code intelligence tool. \
         Output only what the provided evidence supports. \
         Do not read file contents, do not guess behavior beyond what \
         structural connections explicitly show.",
        &user_content,
        cfg.synthesis_num_predict.min(600),
    )
}

/// Name of the synthesis model currently configured (for progress messages).
pub fn synthesis_model_name() -> String {
    OllamaConfig::from_env().synthesis_model
}

/// Name of the reasoning model currently configured.
pub fn reasoning_model_name() -> String {
    OllamaConfig::from_env().reasoning_model
}

/// Generate an implementation plan for a GitHub issue from Atlas evidence.
/// Snippets must match names present in the evidence; no invented modules.
pub fn synthesize_plan(
    issue_number: i64,
    title: &str,
    body: &str,
    doc: &InvestigationDocument,
    repo_path: &str,
) -> Option<String> {
    let cfg = OllamaConfig::from_env();
    let mut evidence = build_prompt(doc);

    // Attach short heads of the top core files so the model can match real APIs.
    let mut heads = String::new();
    for c in doc.core_candidates.iter().take(5) {
        if let Some(head) = read_file_head(repo_path, &c.file, 40) {
            heads.push_str(&format!(
                "\n--- {} (first 40 lines) ---\n{}\n",
                c.file, head
            ));
        }
    }
    if !heads.is_empty() {
        evidence.push_str("\nFILE HEADS (for naming only — do not invent APIs beyond these):\n");
        evidence.push_str(&heads);
    }

    let body_trim = if body.len() > 2000 {
        format!("{}…", &body[..2000])
    } else {
        body.to_string()
    };

    let user = format!(
        "GitHub Issue #{issue_number}: {title}\n\n\
         ISSUE BODY:\n{body_trim}\n\n\
         ATLAS EVIDENCE (only source of truth for paths and structure):\n{evidence}\n\n\
         Write an implementation plan a human will execute manually.\n\
         Rules:\n\
         - Only reference files that appear in CORE FILES / STRUCTURAL CONNECTIONS / FILE HEADS.\n\
         - Prefer small steps and checklists over large rewrites.\n\
         - Include a short test checklist grounded in existing test patterns if any.\n\
         - Do not claim root cause unless structural edges support it.\n\
         - Snippets must use identifiers visible in FILE HEADS when possible.\n\n\
         Use this exact structure:\n\n\
         ## Implementation Plan\n\n\
         ### Summary\n\
         - 3-5 bullets of what to do\n\n\
         ### Steps\n\
         For each step:\n\
         **File:** `path`\n\
         **Function/Class/Location:** where\n\
         **Code to Add or Change:**\n\
         ```lang\n\
         // minimal sketch\n\
         ```\n\n\
         ### Test Checklist\n\
         - [ ] concrete cases\n\n\
         ### Risks / Gaps\n\
         - what evidence does not cover\n"
    );

    call_ollama(
        &cfg,
        &cfg.synthesis_model,
        "You are a senior engineer writing an implementation plan from Atlas \
         evidence. Atlas is the source of truth. Never invent file paths. \
         The human will implement the plan manually — you do not apply patches.",
        &user,
        cfg.synthesis_num_predict.max(1200),
    )
}

/// Send a prompt to Ollama and return the response text.
/// Shared by synthesize() and synthesize_plan().
fn call_ollama(cfg: &OllamaConfig, model: &str, system: &str, user: &str, num_predict: u32) -> Option<String> {
    // Synthesis is non-thinking by default — we want stable structured prose,
    // not a long CoT that crowds out the packet.
    let think = false;
    let payload = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user",   "content": user   }
        ],
        "stream": false,
        "think": think,
        "options": {
            "temperature": 0.1,
            "num_predict": num_predict,
            "num_ctx": cfg.num_ctx,
        }
    });
    let payload_str = serde_json::to_string(&payload).ok()?;

    let mut child = Command::new("curl")
        .args([
            "-s",
            "-m",
            &cfg.timeout_secs.to_string(),
            "-X",
            "POST",
            &cfg.chat_url(),
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
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
    let text = json["message"]["content"].as_str()?.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}
