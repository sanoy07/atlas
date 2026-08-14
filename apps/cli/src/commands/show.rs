use anyhow::Result;
use atlas_core::{show, ShowOptions, ShowSubjectKind};
use atlas_ir::{ShowRecord, ShowSectionKind, ShowSubject};
use atlas_storage::Store;
use chrono::{DateTime, Utc};

pub fn run(
    subject: &str,
    kind:    Option<&str>,
    full:    bool,
    limit:   usize,
    json:    bool,
) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store   = Store::open(&db_path)?;
    let repo    = super::discover_repo_root()?;

    let mut opts = ShowOptions::default();
    opts.full           = full;
    opts.section_limit  = limit.max(1);
    opts.kind           = match kind {
        None                | Some("auto")     => ShowSubjectKind::Auto,
        Some("commit")      => ShowSubjectKind::Commit,
        Some("pr")          => ShowSubjectKind::Pr,
        Some("issue")       => ShowSubjectKind::Issue,
        Some("file")        => ShowSubjectKind::File,
        Some("identity")    => ShowSubjectKind::Identity,
        Some("document")    => ShowSubjectKind::Document,
        Some("config")      => ShowSubjectKind::Config,
        Some("run")         => ShowSubjectKind::Run,
        Some(other)         => anyhow::bail!(
            "unknown --kind `{}`; supported: auto commit pr issue file identity document config run", other),
    };

    let record = show(subject, &repo, &store, opts)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&record)?);
    } else {
        render(&record);
    }
    Ok(())
}

fn render(r: &ShowRecord) {
    // Optional redirect note.
    if let Some(rd) = &r.redirect_note {
        eprintln!(
            "note: `{}` is a historical path — showing the current canonical path `{}`\n\
             (identity id: {}, use `atlas show id:{}` for full lineage)",
            rd.original_subject, rd.current_path, rd.identity_id, rd.identity_id
        );
    }

    // Header — one line per subject kind.
    print_header(&r.subject);
    println!();

    for section in &r.sections {
        let tag = match section.kind {
            ShowSectionKind::Deterministic => "DETERMINISTIC",
            ShowSectionKind::Derived       => "DERIVED",
        };
        println!("{}  [{} — {}]  ({} row{})",
            section.title, tag, section.provenance_table,
            section.rows.len(), if section.rows.len() == 1 { "" } else { "s" });
        if section.rows.is_empty() {
            println!("  (none)");
        } else {
            for row in &section.rows {
                if let Some(link) = &row.link {
                    println!("  {}", row.display);
                    println!("    [go: atlas show {}]", link.token);
                } else {
                    println!("  {}", row.display);
                }
            }
        }
        if let Some(n) = section.truncated_count {
            println!("  … and {} more (use --full or --limit)", n);
        }
        println!();
    }

    println!("PROVENANCE");
    println!("  repo: {}", r.provenance.repo_path);
    if let Some(ts) = r.provenance.ingested_at {
        println!("  subject ingested at: {}", fmt_ts(ts));
    }
    if let Some(id) = r.provenance.latest_run_id {
        println!("  latest ingest run: #{}  [go: atlas show run:{}]", id, id);
    }
}

fn print_header(subject: &ShowSubject) {
    match subject {
        ShowSubject::Commit(c) => {
            println!("COMMIT   {}", c.hash);
            println!("  short:     {}", c.short_hash);
            println!("  author:    {} <{}>", c.author_name, c.author_email);
            println!("  date:      {}", fmt_ts(c.timestamp));
            println!("  message:   {}", c.message);
        }
        ShowSubject::Pr(p) => {
            println!("PULL REQUEST  #{}   [{}]", p.number, p.state.to_uppercase());
            println!("  title:  {}", p.title);
            println!("  author: {}", p.author);
            if let Some(m) = p.merged_at   { println!("  merged: {}", fmt_ts(m)); }
            if let Some(c) = p.created_at  { println!("  created: {}", fmt_ts(c)); }
            if let Some(sha) = &p.merge_commit_sha {
                println!("  merge_commit_sha: {}", sha);
            }
            if !p.body_excerpt.is_empty() {
                println!("  body:");
                for line in p.body_excerpt.lines().take(20) {
                    println!("    {}", line);
                }
            }
        }
        ShowSubject::Issue(i) => {
            println!("ISSUE  #{}   [{}]", i.number, i.state.to_uppercase());
            println!("  title:  {}", i.title);
            if !i.body_excerpt.is_empty() {
                println!("  body:");
                for line in i.body_excerpt.lines().take(20) {
                    println!("    {}", line);
                }
            }
        }
        ShowSubject::File(f) => {
            println!("FILE   {}", f.relative_path);
            if let Some(s) = &f.analysis_status { println!("  analysis_status: {}", s); }
            if let Some(id) = f.identity_id     { println!("  identity_id:     {}  [go: atlas show id:{}]", id, id); }
            if let Some(r) = &f.role            { println!("  role:            {:?}", r); }
        }
        ShowSubject::Identity(i) => {
            println!("FILE IDENTITY   id:{}", i.identity_id);
            if let Some(p) = &i.current_path { println!("  current_path:       {}  [go: atlas show {}]", p, p); }
            println!("  path_history_count: {}", i.path_history_count);
            println!("  commit_count:       {}", i.commit_count);
        }
        ShowSubject::Document(d) => {
            println!("DOCUMENT   {}", d.file_path);
            println!("  doc_type:  {}", d.doc_type);
            println!("  title:     {}", d.title);
            println!("  body ({} bytes):", d.body_bytes);
            for line in d.body_excerpt.lines().take(40) {
                println!("    {}", line);
            }
        }
        ShowSubject::ConfigArtifact(c) => {
            println!("CONFIG ARTIFACT   {}", c.file_path);
            println!("  kind:        {}", c.artifact_kind);
            println!("  sha256:      {}", c.sha256);
            println!("  raw ({} bytes):", c.raw_bytes);
            for line in c.body_excerpt.lines().take(40) {
                println!("    {}", line);
            }
        }
        ShowSubject::IngestRun(r) => {
            println!("INGEST RUN   #{}", r.id);
            println!("  status:          {}", r.exit_status);
            println!("  atlas_version:   {}", r.atlas_version);
            println!("  requested_scope: {}", r.requested_scope);
            if let Some(h) = &r.git_head    { println!("  git_head:        {}  [go: atlas show {}]", h, h); }
            if let Some(b) = &r.git_branch  { println!("  git_branch:      {}", b); }
            println!("  started_at:      {}", fmt_ts(r.started_at));
            if let Some(e) = r.ended_at { println!("  ended_at:        {}", fmt_ts(e)); }
        }
    }
}

fn fmt_ts(ts: i64) -> String {
    DateTime::<Utc>::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| ts.to_string())
}
