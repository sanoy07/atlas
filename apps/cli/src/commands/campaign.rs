use anyhow::Result;
use atlas_core::campaign_next;
use atlas_ir::{CampaignOutcome, GapEntry};

use super::discover_repo_root;

pub fn run_next() -> Result<()> {
    let repo_path = discover_repo_root()?;
    let brief = campaign_next(&repo_path)?;

    match &brief.outcome {
        CampaignOutcome::Ready { gap } => {
            print_ready(gap);
        }
        CampaignOutcome::NoneEarned { candidates } => {
            print_none_earned(candidates);
        }
    }
    print_all_gaps(&brief.all_gaps);

    Ok(())
}

fn print_ready(gap: &GapEntry) {
    let bar = "═".repeat(60);
    println!("\nCAMPAIGN  C-{}", gap.id);
    println!("{bar}\n");

    println!("Problem");
    println!("  {}", gap.name);
    println!("  {}\n", gap.description);

    println!("Classification");
    println!("  {}\n", gap.classification);

    let repo_list = if gap.repositories.is_empty() {
        "—".to_string()
    } else {
        gap.repositories.join(", ")
    };
    let bench_list = if gap.benchmarks.is_empty() {
        "—".to_string()
    } else {
        gap.benchmarks.join(", ")
    };

    println!("Evidence");
    println!("  N={}/{}  ·  {} repositories  ·  {} benchmarks",
        gap.n, gap.threshold, gap.repositories.len(), gap.benchmarks.len());
    println!("  Repositories:  {}", repo_list);
    println!("  Benchmarks:    {}\n", bench_list);

    println!("Earned primitive");
    println!("  Status: {} (threshold N={} met: N={})\n", gap.status, gap.threshold, gap.n);

    println!("Suggested implementation");
    println!("  {}\n", gap.suggested_implementation);

    println!("Success criterion");
    println!("  {}\n", gap.success_criterion);

    println!("Stop condition");
    println!("  All tests pass.");
    println!("  Re-run the blocked investigation — it must reach Optimal without source reads.");
    println!("  Write a decision record and benchmark before committing.\n");
}

fn print_none_earned(candidates: &[GapEntry]) {
    println!("\nNo implementation-ready campaigns exist.\n");

    let (earned_candidates, watch): (Vec<_>, Vec<_>) = candidates.iter()
        .partition(|g| g.n >= 2);

    if !earned_candidates.is_empty() {
        println!("Candidates (needs one more occurrence to earn implementation):\n");
        for g in &earned_candidates {
            println!("  {:<36}  N={}/{}  (needs {} more)",
                g.id, g.n, g.threshold, g.remaining());
            println!("  {}", g.name);
            println!();
        }
    }

    if !watch.is_empty() {
        println!("Watch (single occurrence — not yet a pattern):\n");
        for g in watch {
            println!("  {:<36}  N={}/{}  (needs {} more)",
                g.id, g.n, g.threshold, g.remaining());
            println!("  {}", g.name);
            println!();
        }
    }

    println!("Next action");
    println!("  Run an investigation on RWATP, VestaScan, or a new repository.");
    println!("  When a Watch gap failure occurs, add gap_id frontmatter to the benchmark.");
    println!("  When N reaches threshold, re-run `atlas campaign next`.\n");
}

pub fn print_all_gaps(all_gaps: &[GapEntry]) {
    let implemented: Vec<_> = all_gaps.iter().filter(|g| g.is_implemented()).collect();
    if !implemented.is_empty() {
        println!("Completed:\n");
        for g in implemented {
            println!("  {:<36}  N={}/{}  implemented",
                g.id, g.n, g.threshold);
            println!("  {}", g.name);
            println!();
        }
    }
}
