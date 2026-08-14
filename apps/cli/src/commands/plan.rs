use anyhow::Result;
use atlas_core::{extract_issue_anchors, investigate, plan_from_issue};
use atlas_storage::Store;

const RST:  &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM:  &str = "\x1b[2m";
const CYAN: &str = "\x1b[36m";
const GRN:  &str = "\x1b[32m";
const YLW:  &str = "\x1b[33m";
const BLU:  &str = "\x1b[34m";

pub fn run(issue_number: i64, repo_override: Option<&str>) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store   = Store::open(&db_path)?;
    let repo = match repo_override {
        Some(r) => super::canonical_repo_path(r),
        None    => super::discover_repo_root()?,
    };

    let ctx = plan_from_issue(issue_number, &repo, &store)?;

    let (title, body, anchors, doc) = match ctx {
        Some(c) => (c.title, c.body, c.anchors_used, c.doc),
        None    => {
            eprintln!("Issue #{} not in DB, fetching from GitHub API...", issue_number);
            let (title, body) = fetch_issue_from_github(issue_number, &repo)?;
            let anchors = extract_issue_anchors(&title, &body);
            let anchor_refs: Vec<&str> = anchors.iter().map(String::as_str).collect();
            let doc = investigate(&anchor_refs, &repo, &store)?;
            (title, body, anchors, doc)
        }
    };

    let has_evidence = !doc.core_candidates.is_empty() || !doc.supporting_artifacts.is_empty();

    print_header(issue_number, &title, &anchors);

    eprintln!(
        "{DIM}  synthesising with {}...{RST}",
        crate::ai::synthesis_model_name()
    );
    let plan = crate::ai::synthesize_plan(issue_number, &title, &body, &doc, &repo);

    match plan {
        Some(text) => render_plan(&text),
        None if has_evidence => {
            eprintln!("{YLW}  Ollama unavailable. Run `ollama serve` to enable planning.{RST}");
            println!("  Use `atlas investigate {}` to see raw evidence.", anchors.join(" "));
        }
        None => {
            println!("{YLW}  No evidence found. Run `atlas ingest . --typescript` first.{RST}");
        }
    }

    println!();
    Ok(())
}

fn print_header(issue_number: i64, title: &str, anchors: &[String]) {
    const W: usize = 68;
    let top = format!("{BOLD}{CYAN}╭{}╮{RST}", "─".repeat(W));
    let bot = format!("{BOLD}{CYAN}╰{}╯{RST}", "─".repeat(W));

    let label = format!("  ATLAS PLAN  ·  Issue #{issue_number}");
    let label_cell = format!("{BOLD}{CYAN}│{label:<W$}│{RST}");

    let title_short = if title.len() > W - 2 {
        format!("  {}...", &title[..W - 5])
    } else {
        format!("  {title}")
    };
    let title_cell = format!("{DIM}{CYAN}│{title_short:<W$}│{RST}");

    println!();
    println!("{top}");
    println!("{label_cell}");
    println!("{title_cell}");
    println!("{bot}");
    println!();

    let parts: Vec<String> = anchors.iter()
        .map(|a| format!("{CYAN}{a}{RST}"))
        .collect();
    println!("  {DIM}anchors{RST}  {}", parts.join(&format!(" {DIM}·{RST} ")));
    println!();
}

fn render_plan(text: &str) {
    let mut in_code = false;
    let mut in_verify = false;

    for line in text.lines() {
        // code fence
        if line.starts_with("```") {
            if in_code {
                println!("  {DIM}|{RST}");
                in_code = false;
            } else {
                let lang = line.trim_start_matches('`');
                if lang.is_empty() {
                    println!("  {DIM}|{RST}");
                } else {
                    println!("  {DIM}| {BLU}{lang}{RST}");
                }
                in_code = true;
            }
            continue;
        }
        if in_code {
            println!("  {DIM}|{RST} {line}");
            continue;
        }

        // package verification section
        if line.starts_with("PACKAGE VERIFICATION") {
            in_verify = true;
            println!();
            println!("  {BOLD}{YLW}Package Verification{RST}  {DIM}(Atlas deterministic check){RST}");
            println!("  {YLW}{}{}{}  {RST}", "─".repeat(40), "", "");
            continue;
        }
        if in_verify {
            if line.contains('\u{26A0}') {
                let msg = line.trim_start_matches("  ").trim_start_matches('\u{26A0}').trim();
                println!("  {YLW}  ⚠  {msg}{RST}");
            } else if line.starts_with("Run ") {
                println!("  {DIM}  {line}{RST}");
            } else if !line.is_empty() && !line.starts_with("---") {
                println!("  {DIM}  {line}{RST}");
            }
            continue;
        }

        // section headers
        if let Some(rest) = line.strip_prefix("### ") {
            let content = rest.trim();
            println!();
            println!("  {BOLD}{CYAN}{content}{RST}");
            println!("  {DIM}{}{}  {RST}", "─".repeat(content.len()), "");
            println!();
            continue;
        }
        if line.starts_with("## ") {
            // skip "## Implementation Plan" — we have our header
            continue;
        }

        // labelled fields
        if let Some(rest) = strip_bold_label(line, "File:") {
            let path = rest.trim().trim_matches('`');
            println!("  {BOLD}File{RST}      {GRN}{path}{RST}");
            continue;
        }
        if let Some(rest) = strip_bold_label(line, "Function/Class/Location:")
            .or_else(|| strip_bold_label(line, "Specific Function/Class/Location:"))
        {
            let loc = rest.trim().trim_matches('`');
            println!("  {BOLD}Where{RST}     {DIM}{loc}{RST}");
            continue;
        }
        if strip_bold_label(line, "Code to Add or Change:").is_some() {
            continue;
        }

        // summary bullets
        if let Some(rest) = line.strip_prefix("- **").or_else(|| line.strip_prefix("- ")) {
            println!("  {DIM}*{RST}  {}", render_inline(rest));
            continue;
        }

        // numbered list
        if line.len() > 2
            && line.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
            && line.chars().nth(1) == Some('.')
        {
            println!("  {}", render_inline(line));
            continue;
        }

        // horizontal rule
        if line == "---" || line == "___" {
            println!("  {DIM}{}{}  {RST}", "─".repeat(60), "");
            continue;
        }

        // blank
        if line.is_empty() {
            println!();
            continue;
        }

        // default
        println!("  {}", render_inline(line));
    }
}

fn strip_bold_label<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    line.strip_prefix(&format!("**{label}**"))
}

fn render_inline(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '*' && chars.peek() == Some(&'*') {
            chars.next();
            let mut inner = String::new();
            loop {
                match chars.next() {
                    Some('*') if chars.peek() == Some(&'*') => { chars.next(); break; }
                    Some(ch) => inner.push(ch),
                    None => break,
                }
            }
            out.push_str(&format!("{BOLD}{inner}{RST}"));
        } else if c == '`' {
            let mut inner = String::new();
            loop {
                match chars.next() {
                    Some('`') => break,
                    Some(ch)  => inner.push(ch),
                    None      => break,
                }
            }
            out.push_str(&format!("{DIM}{inner}{RST}"));
        } else {
            out.push(c);
        }
    }
    out
}

fn fetch_issue_from_github(issue_number: i64, repo_path: &str) -> Result<(String, String)> {
    let token = std::env::var("GITHUB_TOKEN")
        .map_err(|_| anyhow::anyhow!(
            "Issue #{} not in DB and GITHUB_TOKEN is not set.\n\
             Run `atlas ingest --github` to populate issues, or set GITHUB_TOKEN.",
            issue_number
        ))?;

    let remote_output = std::process::Command::new("git")
        .args(["-C", repo_path, "remote", "get-url", "origin"])
        .output()?;
    let remote_url = String::from_utf8_lossy(&remote_output.stdout).trim().to_string();

    let (owner, repo) = parse_github_remote(&remote_url)
        .ok_or_else(|| anyhow::anyhow!("Cannot parse GitHub owner/repo from remote: {}", remote_url))?;

    let api_url = format!("https://api.github.com/repos/{}/{}/issues/{}", owner, repo, issue_number);

    let output = std::process::Command::new("curl")
        .args([
            "-s", "-m", "15",
            "-H", &format!("Authorization: token {}", token),
            "-H", "Accept: application/vnd.github.v3+json",
            "-H", "User-Agent: atlas-cli",
            &api_url,
        ])
        .output()?;

    if !output.status.success() {
        anyhow::bail!("GitHub API request failed (status {})", output.status);
    }

    let body_str = String::from_utf8(output.stdout)?;
    let json: serde_json::Value = serde_json::from_str(&body_str)
        .map_err(|e| anyhow::anyhow!("GitHub API response parse error: {}", e))?;

    if let Some(msg) = json["message"].as_str() {
        anyhow::bail!("GitHub API error: {}", msg);
    }

    let title = json["title"].as_str()
        .ok_or_else(|| anyhow::anyhow!("GitHub response missing 'title'"))?
        .to_string();
    let body = json["body"].as_str().unwrap_or("").to_string();

    Ok((title, body))
}

fn parse_github_remote(url: &str) -> Option<(String, String)> {
    let stripped = url.trim_end_matches(".git").trim_end_matches('/');

    if let Some(path) = stripped.strip_prefix("git@github.com:") {
        let mut parts = path.splitn(2, '/');
        return Some((parts.next()?.to_string(), parts.next()?.to_string()));
    }
    if let Some(path) = stripped.strip_prefix("https://github.com/") {
        let mut parts = path.splitn(2, '/');
        return Some((parts.next()?.to_string(), parts.next()?.to_string()));
    }

    None
}
