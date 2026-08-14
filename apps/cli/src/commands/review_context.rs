use anyhow::Result;
use atlas_core;
use atlas_ir::ReviewContextDocument;
use atlas_storage::Store;

pub fn run(pr_number: u64, json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store   = Store::open(&db_path)?;
    let repo    = super::discover_repo_root()?;

    let doc = atlas_core::build_review_context(pr_number, &repo, &store)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&doc)?);
        return Ok(());
    }

    print_review_context(&doc);
    Ok(())
}

fn print_review_context(doc: &ReviewContextDocument) {
    println!("REVIEW CONTEXT  PR #{}", doc.pr_number);
    println!("{}", doc.pr_title);
    if !doc.linked_issue_numbers.is_empty() {
        let issues: Vec<String> = doc.linked_issue_numbers.iter().map(|n| format!("#{n}")).collect();
        println!("Closes: {}", issues.join(", "));
    }
    println!();

    println!("CHANGED FILES  ({})", doc.pr_files.len());
    for f in &doc.pr_files {
        println!("  {}", f.file);
    }
    println!();

    for f in &doc.pr_files {
        println!("────────────────────────────────────────");
        println!("{}", f.file);
        println!("  Role:         {:?}", f.role);
        println!("  Touch count:  {}", f.touch_count);
        if let Some(msg) = &f.last_commit_message {
            let short: String = msg.lines().next().unwrap_or("").chars().take(72).collect();
            println!("  Last commit:  {short}");
        }

        if !f.structural_out.is_empty() {
            println!();
            println!("  OUTGOING EDGES");
            for e in &f.structural_out {
                let sym = e.symbol.as_deref().unwrap_or("");
                println!("    [{kind}] → {file}{sym_part}",
                    kind = e.kind.to_uppercase(),
                    file = e.file,
                    sym_part = if sym.is_empty() { String::new() } else { format!("  ({sym})") },
                );
            }
        }

        if !f.structural_in.is_empty() {
            println!();
            println!("  INCOMING EDGES");
            for e in &f.structural_in {
                println!("    [{kind}] ← {file}", kind = e.kind.to_uppercase(), file = e.file);
            }
        }

        if !f.cochanges.is_empty() {
            println!();
            println!("  CO-CHANGES");
            for c in &f.cochanges {
                let marker = if c.in_pr { " [in PR]" } else { "" };
                println!("    {}×  {}{}", c.count, c.file, marker);
            }
        }

        println!();
    }

    if !doc.documentary.is_empty() {
        println!("DOCUMENTARY CONTEXT");
        for d in &doc.documentary {
            let anchors = d.matched_anchors.join(", ");
            println!("  {} #{}: {} (matched: {})", d.kind.to_uppercase(), d.number, d.title, anchors);
        }
        println!();
    }

    println!("COVERAGE");
    println!("  git history      {}", coverage_flag(doc.coverage.git_history));
    println!("  github prs       {}", coverage_flag(doc.coverage.github_prs));
    println!("  github issues    {}", coverage_flag(doc.coverage.github_issues));
    println!("  structural edges {}", coverage_flag(doc.coverage.structural_edges));
}

fn coverage_flag(v: bool) -> &'static str {
    if v { "✓" } else { "✗ not ingested" }
}
