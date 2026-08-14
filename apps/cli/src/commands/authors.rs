use anyhow::{anyhow, Result};
use atlas_core::{compute_authors, AuthorsSubjectKind};
use atlas_ir::{AuthorScope, AuthorsReport};
use atlas_storage::Store;
use chrono::{DateTime, Utc};

pub fn run(path: &str, kind: Option<&str>, json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store   = Store::open(&db_path)?;
    let repo    = super::discover_repo_root()?;

    let subject_kind = match kind {
        None | Some("auto") => AuthorsSubjectKind::Auto,
        Some("dir")  | Some("directory") => AuthorsSubjectKind::Directory,
        Some("file") => AuthorsSubjectKind::File,
        Some(other)  => return Err(anyhow!(
            "unknown --kind '{}': use auto | dir | file",
            other
        )),
    };

    let report = compute_authors(path, subject_kind, &repo, &store)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render(&report);
    }
    Ok(())
}

fn render(r: &AuthorsReport) {
    if let Some(rn) = &r.redirect_note {
        eprintln!(
            "note: `{}` is a historical path — showing authors for the current\n\
             canonical path `{}` (identity id: {})",
            rn.original_subject, rn.current_path, rn.identity_id
        );
    }

    println!("AUTHORS   (subject: {})", r.subject);
    let scope_label = match r.scope {
        AuthorScope::Prefix    => "prefix",
        AuthorScope::ExactFile => "exact file",
        AuthorScope::Identity  => "identity chain (rename-safe)",
    };
    println!("  scope:  {} — {}", scope_label, r.scope_detail);
    println!("  peers:  authors are (name, email) tuples — no alias merging");
    println!();

    if r.authors.is_empty() {
        println!("(no commits observed for this subject in this repository)");
        println!();
        print_provenance(r);
        return;
    }

    println!("Per author (sorted by commit count desc):");
    println!("  {:>8}  {:<12}  {:<12}  {}", "commits", "first", "last", "author");
    for a in &r.authors {
        println!(
            "  {:>8}  {:<12}  {:<12}  {} <{}>",
            a.commit_count,
            format_date(a.first_touch),
            format_date(a.last_touch),
            a.author_name,
            a.author_email,
        );
    }
    println!();

    println!(
        "TOTALS: {} commit{} · {} unique (name, email) tuple{}",
        r.total_commits, plural(r.total_commits),
        r.total_authors, plural(r.total_authors),
    );
    println!();
    print_provenance(r);
}

fn print_provenance(r: &AuthorsReport) {
    println!("PROVENANCE");
    match r.scope {
        AuthorScope::Prefix | AuthorScope::ExactFile => {
            println!("  data source:    commits + commit_files (repo-scoped via commits.repo_path)");
        }
        AuthorScope::Identity => {
            println!("  data source:    commits + file_identity_commits (repo-scoped via commits.repo_path and fic.repo_path)");
        }
    }
    println!("  denominator:    COUNT(DISTINCT commits.hash) — a commit that touched N");
    println!("                  files in the subtree counts as 1, not N");
    println!("  language note:  counts are OBSERVED COMMITS — not a claim about ownership,");
    println!("                  expertise, or contribution weight");
}

fn format_date(unix_seconds: i64) -> String {
    match DateTime::<Utc>::from_timestamp(unix_seconds, 0) {
        Some(dt) => dt.format("%Y-%m-%d").to_string(),
        None     => "-".to_string(),
    }
}

fn plural(n: usize) -> &'static str { if n == 1 { "" } else { "s" } }
