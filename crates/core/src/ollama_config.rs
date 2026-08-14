//! Shared local-Ollama settings for Atlas.
//!
//! All CLI synthesis paths and the reasoning provider read from the same
//! environment variables so a single export configures the whole tool.
//!
//! Defaults are tuned for a 6GB laptop GPU (RTX 3050 class):
//! - **Reasoning / investigate loop:** `qwen3:4b` — tools + thinking, full GPU
//!   at `num_ctx=12288` (see docs/research/2026-08-10-qwen3-4b-thinking.md)
//! - **Prose synthesis / plan / snippets:** `qwen2.5-coder:7b-instruct` —
//!   code-shaped drafts over a sealed evidence packet
//!
//! | Variable | Default |
//! |----------|---------|
//! | `ATLAS_OLLAMA_URL` | `http://localhost:11434` |
//! | `ATLAS_OLLAMA_MODEL` | `qwen3:4b` (reasoning) |
//! | `ATLAS_OLLAMA_SYNTHESIS_MODEL` | `qwen2.5-coder:7b-instruct` |
//! | `ATLAS_OLLAMA_NUM_CTX` | `12288` |
//! | `ATLAS_OLLAMA_NUM_PREDICT` | model-dependent |
//! | `ATLAS_OLLAMA_TIMEOUT` | `180` |
//! | `ATLAS_OLLAMA_THINK` | auto (on for qwen3*) |

use std::env;
use std::process::{Command, Stdio};

/// Resolved Ollama connection + generation parameters.
#[derive(Debug, Clone)]
pub struct OllamaConfig {
    pub base_url: String,
    /// Model for structured reasoning investigation (`atlas investigate "…"`).
    pub reasoning_model: String,
    /// Model for prose synthesis and implementation plans.
    pub synthesis_model: String,
    pub timeout_secs: u64,
    pub num_ctx: u32,
    pub reasoning_num_predict: u32,
    pub synthesis_num_predict: u32,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

impl OllamaConfig {
    pub fn from_env() -> Self {
        let reasoning_model =
            env::var("ATLAS_OLLAMA_MODEL").unwrap_or_else(|_| "qwen3:4b".into());

        let synthesis_model = env::var("ATLAS_OLLAMA_SYNTHESIS_MODEL")
            .unwrap_or_else(|_| "qwen2.5-coder:7b-instruct".into());

        let reasoning_num_predict = env::var("ATLAS_OLLAMA_NUM_PREDICT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| {
                if reasoning_model.to_lowercase().contains("qwen3") {
                    4096
                } else {
                    1200
                }
            });

        let synthesis_num_predict = env::var("ATLAS_OLLAMA_SYNTHESIS_NUM_PREDICT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(800);

        let timeout_secs = env::var("ATLAS_OLLAMA_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(180);

        // 12288: largest window keeping all layers on a 6GB GPU for qwen3:4b.
        let num_ctx = env::var("ATLAS_OLLAMA_NUM_CTX")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(12288);

        let base_url =
            env::var("ATLAS_OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".into());

        Self {
            base_url,
            reasoning_model,
            synthesis_model,
            timeout_secs,
            num_ctx,
            reasoning_num_predict,
            synthesis_num_predict,
        }
    }

    /// Whether thinking should be enabled for a given model name.
    pub fn think_for_model(model: &str) -> bool {
        match env::var("ATLAS_OLLAMA_THINK") {
            Ok(s) => {
                let s = s.to_lowercase();
                !(s == "0" || s == "false" || s == "no")
            }
            Err(_) => model.to_lowercase().contains("qwen3"),
        }
    }

    pub fn chat_url(&self) -> String {
        format!("{}/api/chat", self.base_url.trim_end_matches('/'))
    }

    pub fn tags_url(&self) -> String {
        format!("{}/api/tags", self.base_url.trim_end_matches('/'))
    }
}

/// Probe whether Ollama is reachable and which models are installed.
pub fn probe_ollama(cfg: &OllamaConfig) -> OllamaProbe {
    let mut probe = OllamaProbe {
        reachable: false,
        models: vec![],
        has_reasoning: false,
        has_synthesis: false,
        error: None,
    };

    let output = Command::new("curl")
        .args(["-s", "-m", "3", "-f", &cfg.tags_url()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();

    let Ok(output) = output else {
        probe.error = Some("curl not available".into());
        return probe;
    };

    if !output.status.success() {
        probe.error = Some(format!(
            "not reachable at {} (is `ollama serve` running?)",
            cfg.base_url
        ));
        return probe;
    }

    let body = String::from_utf8_lossy(&output.stdout);
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) else {
        probe.error = Some("invalid /api/tags response".into());
        return probe;
    };

    probe.reachable = true;
    if let Some(arr) = json.get("models").and_then(|m| m.as_array()) {
        for m in arr {
            if let Some(name) = m.get("name").and_then(|n| n.as_str()) {
                probe.models.push(name.to_string());
            }
        }
    }

    probe.has_reasoning = model_installed(&probe.models, &cfg.reasoning_model);
    probe.has_synthesis = model_installed(&probe.models, &cfg.synthesis_model);
    probe
}

fn model_installed(installed: &[String], want: &str) -> bool {
    let want_l = want.to_lowercase();
    let want_base = want_l.split(':').next().unwrap_or(&want_l);
    let want_has_tag = want_l.contains(':');
    installed.iter().any(|m| {
        let m = m.to_lowercase();
        let m_base = m.split(':').next().unwrap_or(&m);
        m == want_l
            || m.starts_with(&format!("{want_l}:"))
            || m.starts_with(&format!("{want_l}-"))
            // want without tag → any tag of that family counts
            || (!want_has_tag && m_base == want_base)
            // want with tag → installed must start with full want
            || (want_has_tag && m.starts_with(&want_l))
    })
}

#[derive(Debug, Clone)]
pub struct OllamaProbe {
    pub reachable: bool,
    pub models: Vec<String>,
    pub has_reasoning: bool,
    pub has_synthesis: bool,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_match_exact_and_prefix() {
        let installed = vec![
            "qwen3:4b".into(),
            "qwen2.5-coder:7b-instruct".into(),
            "nomic-embed-text:latest".into(),
        ];
        assert!(model_installed(&installed, "qwen3:4b"));
        assert!(model_installed(&installed, "qwen2.5-coder:7b-instruct"));
        assert!(model_installed(&installed, "nomic-embed-text"));
        assert!(!model_installed(&installed, "deepseek-r1:8b"));
    }
}
