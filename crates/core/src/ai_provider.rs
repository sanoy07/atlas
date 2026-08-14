//! Local AI provider abstraction.
//!
//! Cloud providers are out of scope for this module.  Local HTTP (Ollama)
//! is the default implementation.  Tests use `FakeReasoningProvider`.
//!
//! Atlas remains authoritative for evidence; the provider only reasons.

use crate::ollama_config::OllamaConfig;
use anyhow::{anyhow, Result};
use atlas_ir::{AiReasoningResponse, EvidencePacket};
use std::io::Write;
use std::process::{Command, Stdio};

/// Metadata about a reasoning model invocation.
#[derive(Debug, Clone)]
pub struct ProviderMeta {
    pub name: String,
    pub model: String,
}

/// Pluggable local reasoning backend.
pub trait ReasoningProvider: Send {
    fn meta(&self) -> ProviderMeta;

    /// Reason over a bounded evidence packet.  Must return structured JSON
    /// parsable as `AiReasoningResponse` when possible; free text may be
    /// placed in `explanation` with empty hypotheses.
    fn reason(&self, system: &str, user: &str) -> Result<AiReasoningResponse>;
}

// ─── Ollama HTTP (local) ────────────────────────────────────────────────────

/// Local Ollama chat API (`http://localhost:11434` by default).
pub struct OllamaProvider {
    pub base_url: String,
    pub model: String,
    pub timeout_secs: u64,
    pub num_predict: u32,
    /// Ollama's default context is far smaller than an Atlas evidence packet,
    /// and an overflowing packet is silently truncated rather than rejected.
    pub num_ctx: u32,
}

impl Default for OllamaProvider {
    fn default() -> Self {
        let cfg = OllamaConfig::from_env();
        Self {
            base_url: cfg.base_url,
            model: cfg.reasoning_model,
            timeout_secs: cfg.timeout_secs,
            num_predict: cfg.reasoning_num_predict,
            num_ctx: cfg.num_ctx,
        }
    }
}

impl ReasoningProvider for OllamaProvider {
    fn meta(&self) -> ProviderMeta {
        ProviderMeta {
            name: "ollama".into(),
            model: self.model.clone(),
        }
    }

    fn reason(&self, system: &str, user: &str) -> Result<AiReasoningResponse> {
        let text = call_ollama_chat(
            &self.base_url,
            &self.model,
            system,
            user,
            self.timeout_secs,
            self.num_predict,
            self.num_ctx,
        )
        .ok_or_else(|| anyhow!("local Ollama provider unavailable or returned empty response"))?;
        Ok(parse_reasoning_response(&text))
    }
}

fn call_ollama_chat(
    base_url: &str,
    model: &str,
    system: &str,
    user: &str,
    timeout_secs: u64,
    num_predict: u32,
    num_ctx: u32,
) -> Option<String> {
    let url = format!("{}/api/chat", base_url.trim_end_matches('/'));
    // Qwen3: thinking channel is the point of the "Thinking" experiment.
    // ATLAS_OLLAMA_THINK=0|1 overrides; default true for qwen3*, false otherwise
    // (structured JSON often lands in content when think=false; thinking models
    // often leave content empty — we fall back to message.thinking).
    let think = OllamaConfig::think_for_model(model);
    // Sampling must follow the mode. Qwen documents that thinking models
    // degrade — and fall into endless repetition — under near-greedy decoding,
    // so a thinking call uses Qwen's published thinking preset rather than the
    // low temperature that makes non-thinking models emit stabler JSON.
    let options = if think {
        serde_json::json!({
            "temperature": 0.6,
            "top_p": 0.95,
            "top_k": 20,
            "min_p": 0.0,
            "num_predict": num_predict,
            "num_ctx": num_ctx,
        })
    } else {
        serde_json::json!({
            "temperature": 0.1,
            "num_predict": num_predict,
            "num_ctx": num_ctx,
        })
    };
    let payload = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
        "stream": false,
        "think": think,
        "options": options
    });
    let payload_str = serde_json::to_string(&payload).ok()?;
    let mut child = Command::new("curl")
        .args([
            "-s",
            "-m",
            &timeout_secs.to_string(),
            "-X",
            "POST",
            &url,
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
    let msg = &json["message"];
    let content = msg["content"].as_str().unwrap_or("").trim();
    let thinking = msg["thinking"].as_str().unwrap_or("").trim();
    // Prefer content (final answer). If empty, fall back to thinking chain
    // (may embed a JSON object). Never drop content when present.
    let text = if !content.is_empty() {
        content.to_string()
    } else if !thinking.is_empty() {
        thinking.to_string()
    } else {
        return None;
    };
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Parse model output: prefer fenced JSON; else whole body as JSON; else explanation-only.
///
/// Tolerates loose Qwen3 shapes (hypotheses/claims as strings; mixed arrays) so the
/// investigator loop can still verify and expand — strict schema is ideal, not assumed.
pub fn parse_reasoning_response(text: &str) -> AiReasoningResponse {
    if let Some(json_str) = extract_json_block(text) {
        if let Some(r) = parse_reasoning_json(json_str) {
            return r;
        }
    }
    if let Some(r) = parse_reasoning_json(text.trim()) {
        return r;
    }
    AiReasoningResponse {
        explanation: text.trim().to_string(),
        ..Default::default()
    }
}

fn parse_reasoning_json(json_str: &str) -> Option<AiReasoningResponse> {
    if let Ok(r) = serde_json::from_str::<AiReasoningResponse>(json_str) {
        return Some(r);
    }
    // Loose value coercion for small local models
    let v: serde_json::Value = serde_json::from_str(json_str).ok()?;
    if !v.is_object() {
        return None;
    }
    let mut out = AiReasoningResponse::default();
    if let Some(arr) = v.get("hypotheses").and_then(|x| x.as_array()) {
        for (i, item) in arr.iter().enumerate() {
            if let Some(s) = item.as_str() {
                out.hypotheses.push(atlas_ir::Hypothesis {
                    id: format!("h{}", i + 1),
                    statement: s.to_string(),
                    status: atlas_ir::ClaimStatus::Plausible,
                    supporting: vec![],
                    contradicting: vec![],
                    claims: vec![],
                });
            } else if let Ok(h) = serde_json::from_value::<atlas_ir::Hypothesis>(item.clone()) {
                out.hypotheses.push(h);
            } else if let Some(obj) = item.as_object() {
                let statement = obj
                    .get("statement")
                    .and_then(|x| x.as_str())
                    .or_else(|| obj.get("text").and_then(|x| x.as_str()))
                    .unwrap_or("")
                    .to_string();
                if !statement.is_empty() {
                    out.hypotheses.push(atlas_ir::Hypothesis {
                        id: obj
                            .get("id")
                            .and_then(|x| x.as_str())
                            .unwrap_or(&format!("h{}", i + 1))
                            .to_string(),
                        statement,
                        status: atlas_ir::ClaimStatus::Plausible,
                        supporting: vec![],
                        contradicting: vec![],
                        claims: vec![],
                    });
                }
            }
        }
    }
    if let Some(arr) = v.get("requested_subjects").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(s) = item.as_str() {
                if !s.is_empty() {
                    out.requested_subjects.push(s.to_string());
                }
            }
        }
    }
    if let Some(arr) = v.get("questions").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(s) = item.as_str() {
                out.questions.push(s.to_string());
            }
        }
    }
    if let Some(arr) = v
        .get("proposed_claims")
        .or_else(|| v.get("claims"))
        .and_then(|x| x.as_array())
    {
        for (i, item) in arr.iter().enumerate() {
            if let Some(s) = item.as_str() {
                out.proposed_claims.push(atlas_ir::ProposedClaim {
                    id: format!("c{}", i + 1),
                    subject: String::new(),
                    statement: s.to_string(),
                    kind: "structural".into(),
                    evidence_refs: vec![],
                    method: "model".into(),
                    temporal_scope: String::new(),
                    limitations: vec![],
                    status: atlas_ir::ClaimStatus::Unresolved,
                });
            } else if let Ok(c) = serde_json::from_value::<atlas_ir::ProposedClaim>(item.clone()) {
                out.proposed_claims.push(c);
            }
        }
    }
    if let Some(s) = v.get("explanation").and_then(|x| x.as_str()) {
        out.explanation = s.to_string();
    }
    if out.hypotheses.is_empty()
        && out.proposed_claims.is_empty()
        && out.requested_subjects.is_empty()
        && out.explanation.is_empty()
    {
        return None;
    }
    Some(out)
}

fn extract_json_block(text: &str) -> Option<&str> {
    if let Some(start) = text.find("```json") {
        let after = &text[start + 7..];
        let end = after.find("```")?;
        return Some(after[..end].trim());
    }
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        let after = after.strip_prefix('\n').unwrap_or(after);
        let end = after.find("```")?;
        let block = after[..end].trim();
        if block.starts_with('{') {
            return Some(block);
        }
    }
    // raw object
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end > start {
        Some(&text[start..=end])
    } else {
        None
    }
}

// ─── Fake provider (tests) ──────────────────────────────────────────────────

/// Deterministic provider for tests.  Returns a canned response, optionally
/// capturing the last user prompt.
pub struct FakeReasoningProvider {
    pub response: AiReasoningResponse,
    pub model_name: String,
    pub last_user: std::sync::Mutex<Option<String>>,
    pub fail: bool,
}

impl FakeReasoningProvider {
    pub fn new(response: AiReasoningResponse) -> Self {
        Self {
            response,
            model_name: "fake-local".into(),
            last_user: std::sync::Mutex::new(None),
            fail: false,
        }
    }
}

impl ReasoningProvider for FakeReasoningProvider {
    fn meta(&self) -> ProviderMeta {
        ProviderMeta {
            name: "fake".into(),
            model: self.model_name.clone(),
        }
    }

    fn reason(&self, _system: &str, user: &str) -> Result<AiReasoningResponse> {
        if self.fail {
            return Err(anyhow!("fake provider forced failure"));
        }
        if let Ok(mut g) = self.last_user.lock() {
            *g = Some(user.to_string());
        }
        Ok(self.response.clone())
    }
}

/// Compact packet summary for prompts (bounded size).
pub fn packet_prompt_summary(packet: &EvidencePacket, max_files: usize) -> String {
    let mut p = String::new();
    p.push_str(&format!("QUESTION: {}\n", packet.question));
    p.push_str(&format!("REPO: {}\n", packet.repo_path));
    if let Some(h) = &packet.git_head {
        p.push_str(&format!("GIT_HEAD: {}\n", h));
    }
    p.push_str(&format!("ANCHORS: {}\n\n", packet.anchors.join(", ")));

    p.push_str("CORE_CANDIDATES:\n");
    for c in packet.investigation.core_candidates.iter().take(max_files) {
        p.push_str(&format!("  - {}\n", c.file));
    }
    p.push_str("SUPPORTING:\n");
    for c in packet
        .investigation
        .supporting_artifacts
        .iter()
        .take(max_files / 2 + 1)
    {
        p.push_str(&format!("  - {} ({:?})\n", c.file, c.role));
    }

    p.push_str("\nSTRUCTURAL (sample):\n");
    let mut n = 0usize;
    for obs in &packet.investigation.observed_structure {
        for e in obs.outgoing.iter().take(3) {
            if e.kind == "imports" {
                continue;
            }
            p.push_str(&format!(
                "  {} -[{}]-> {}\n",
                obs.file, e.kind, e.file
            ));
            n += 1;
            if n >= 20 {
                break;
            }
        }
        if n >= 20 {
            break;
        }
    }

    p.push_str("\nDOCUMENTARY:\n");
    for d in packet.investigation.documentary.iter().take(8) {
        p.push_str(&format!(
            "  {} #{}: {}\n",
            d.kind, d.number, d.title
        ));
    }

    p.push_str("\nRANKED_EVIDENCE (highest weight first; prefer these over the bag dump):\n");
    for item in packet.ranked_evidence.iter().take(18) {
        p.push_str(&format!(
            "  #{rank} w={weight:.2} [{sem}] {kind}:{id} — {summary}\n",
            rank = item.rank,
            weight = item.weight,
            sem = item.event_semantics,
            kind = item.ref_.kind,
            id = item.ref_.id,
            summary = item.ref_.summary,
        ));
        for n in item.ranking_notes.iter().take(2) {
            p.push_str(&format!("      note: {n}\n"));
        }
    }

    if !packet.supersession.is_empty() {
        p.push_str("\nSUPERSESSION (event semantics + chronology; not mere recency):\n");
        for s in packet.supersession.iter().take(10) {
            p.push_str(&format!(
                "  {} → {} ({}) — {}\n",
                s.earlier_id, s.later_id, s.relationship, s.note
            ));
        }
    }

    p.push_str("\nCHRONOLOGY (oldest→newest sample):\n");
    for ev in packet.chronology.iter().take(15) {
        p.push_str(&format!(
            "  ts={} {} {} — {}\n",
            ev.timestamp, ev.role, ev.id, ev.summary
        ));
    }

    if !packet.modules_present.is_empty() {
        p.push_str(&format!(
            "\nMODULES: {}\n",
            packet.modules_present.join(", ")
        ));
    }

    p.push_str("\nLIMITATIONS:\n");
    for l in &packet.limitations {
        p.push_str(&format!("  - {}\n", l));
    }

    if !packet.verification_policy.is_empty() {
        p.push_str("\nVERIFICATION_POLICY (Atlas will enforce these after you propose claims):\n");
        for v in &packet.verification_policy {
            p.push_str(&format!("  - {v}\n"));
        }
    }

    p.push_str(
        "\nRespond with a single JSON object (optional ```json fence) matching:\n\
         {\n\
           \"hypotheses\": [{\"id\":\"h1\",\"statement\":\"...\",\"status\":\"plausible\",\
             \"supporting\":[{\"kind\":\"file\",\"id\":\"path\",\"summary\":\"...\"}],\
             \"contradicting\":[],\"claims\":[]}],\n\
           \"requested_subjects\": [\"optional/path.ts\"],\n\
           \"questions\": [],\n\
           \"proposed_claims\": [{\"id\":\"c1\",\"subject\":\"...\",\"statement\":\"...\",\
             \"kind\":\"structural\",\"evidence_refs\":[...],\"method\":\"...\",\
             \"temporal_scope\":\"\",\"limitations\":[],\"status\":\"plausible\"}],\n\
           \"explanation\": \"short synthesis\"\n\
         }\n\
         Rules: Only cite evidence present above. Never invent files, commits, or PRs.\n\
         Prefer status plausible/unresolved when uncertain. Do not claim ownership or root cause.\n\
         Existence of a file/issue is NOT support for a causal claim. Prefer ranked evidence.\n\
         Intent (issues/PRs) describes desire; implementation (commits/code) describes current behavior.\n",
    );
    p
}

/// System prompt for local investigation reasoning.
pub fn investigation_system_prompt() -> &'static str {
    "You are a reasoning investigator for Atlas, a deterministic software-engineering evidence engine. \
     You receive ONLY a bounded evidence packet (ranked files, structure sample, chronology, \
     supersession, verification policy). You do NOT have repository access. \
     Your job: (1) form hypotheses grounded in the packet, (2) identify contradictions and missing \
     evidence, (3) request additional file subjects via requested_subjects when the packet is \
     incomplete for multi-hop questions, (4) propose claims with evidence_refs that already exist \
     in the packet, (5) write a short explanation that separates what is established from what is \
     unknown. Prefer high-ranked evidence. Implementation describes current behavior; issues/PRs \
     are intent and do not automatically override later code. Existence of a file is NOT causal \
     support. Never invent paths, commits, issues, or ownership. \
     Always respond with a single JSON object as instructed (hypotheses, requested_subjects, \
     questions, proposed_claims, explanation). If uncertain, use status plausible or unresolved."
}
