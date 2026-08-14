//! `atlas init` — one command from "cloned repo" to "queryable evidence graph".
//!
//! Everything here was previously a sequence the user had to know: find the
//! repo root, decide where the DB goes, remember `--typescript`, and discover
//! for themselves that `atlas.db` would otherwise show up as an untracked file
//! in every `git status`.  None of that is a decision worth making twice.

use anyhow::Result;
use std::io::Write;
use std::path::Path;

const BOLD: &str = "\x1b[1m";
const DIM:  &str = "\x1b[2m";
const GRN:  &str = "\x1b[32m";
const RST:  &str = "\x1b[0m";

pub fn run(github: bool, no_gitignore: bool) -> Result<()> {
    let repo = super::discover_repo_root()?;
    let db_path = Path::new(&repo).join(super::DB_FILENAME);

    println!("{BOLD}Initializing Atlas{RST}");
    println!("  {DIM}repository{RST}  {repo}");
    println!("  {DIM}database{RST}    {}", db_path.display());

    if !no_gitignore {
        match ensure_db_ignored(&repo) {
            Ok(true) => println!("  {GRN}✓{RST} added `{}` to .gitignore", super::DB_FILENAME),
            Ok(false) => {}
            // Never fail init over .gitignore — it is a convenience, not the job.
            Err(e) => eprintln!("  {DIM}note: could not update .gitignore: {e}{RST}"),
        }
    }

    println!();
    // Structural extractors are auto-detected inside ingest; `typescript: false`
    // means "do not force", not "skip".
    super::ingest::run(&repo, github, false, false)?;

    println!();
    println!("{BOLD}Next{RST}");
    println!("  atlas status                       {DIM}# health + evidence freshness{RST}");
    println!("  atlas map                          {DIM}# orient in the repository{RST}");
    println!("  atlas code-search <symbol>         {DIM}# ranked structural search{RST}");
    println!("  atlas callers <symbol>             {DIM}# who calls this{RST}");
    println!("  atlas investigate \"<question>\"     {DIM}# evidence packet + reasoning{RST}");
    Ok(())
}

/// Append `atlas.db` to the repository's `.gitignore` unless it is already
/// mentioned.  Returns whether the file was modified.
///
/// Deliberately a substring check rather than full gitignore-pattern matching:
/// the only question is "will the user be surprised by an untracked DB", and a
/// pattern like `*.db` already answers it.
fn ensure_db_ignored(repo: &str) -> Result<bool> {
    let gitignore = Path::new(repo).join(".gitignore");
    let existing = std::fs::read_to_string(&gitignore).unwrap_or_default();

    if existing.lines().any(|l| {
        let l = l.trim();
        l == super::DB_FILENAME || l == "*.db" || l == "/atlas.db"
    }) {
        return Ok(false);
    }

    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&gitignore)?;

    let leading_newline = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    write!(
        f,
        "{leading_newline}\n# Atlas evidence database (rebuild with `atlas ingest .`)\n{}\n",
        super::DB_FILENAME
    )?;
    Ok(true)
}
