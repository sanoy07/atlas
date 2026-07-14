use anyhow::{Context, Result};
use atlas_connectors::Connector;
use atlas_git::{GitHubIssueConnector, GitHubPrConnector, GitRepo};
use atlas_ir::{
    AnchorMatch, ArtifactRole, CandidateArtifact, CandidateReason, CochangeEntry, CommitSummary,
    ConceptExpansion, ContextDocument, CouplingEntry, CoverageMap, CoverageStatus,
    DocumentaryEvidence, EvidenceSummary, EvidenceType, FileIdentity, FileSignificance,
    HistoricalEntry, InvestigationCoverage, InvestigationDocument, IssueSummary, MatchSource,
    PrFileContext, PrSummary, RelatedHistory, ReviewContextDocument, ReviewCoverage,
    SearchCoverage, SearchDocument, StructuralEdgeSummary, StructuralObservation,
    UnresolvedConnection, VerifiedExpansion,
};
use atlas_parser::{gh_json, git_log, git_renames, ts_structural};
use atlas_storage::{CommitRow, HotFileRow, Store};
use std::collections::HashSet;
use tracing::info;

pub fn ingest_git(repo_path: &str, store: &Store) -> Result<usize> {
    let connector = GitRepo::open(repo_path)?;
    let payload   = connector.fetch_raw()?;
    let commits   = git_log::parse(&payload.data)?;
    let count     = commits.len();

    info!(
        "connector={} capability={} parsed={} commits",
        connector.name(),
        connector.capability().name,
        count,
    );

    for commit in &commits {
        store.insert_commit(commit, repo_path)?;
    }

    Ok(count)
}

/// Rebuild materialized file identities from stored rename evidence.
///
/// Reads `rename_evidence`, constructs identity chains from oldest to newest,
/// and writes to `file_identities` + `file_path_observations`.  The operation
/// is destructive (clears existing identity state first) and deterministic:
/// running it twice produces the same result.
///
/// v0.4 scope: handles linear rename chains and single-cycle path reuse.
/// Multiple reuse cycles (path renamed away, reused, renamed away again) are
/// detected but the second reuse chain is not further extended.
pub fn rebuild_file_identities(repo_path: &str, store: &Store) -> Result<usize> {
    use std::collections::{HashMap, HashSet};

    store.clear_file_identities(repo_path)?;

    let edges = store.rename_evidence_with_timestamps(repo_path)?;
    if edges.is_empty() {
        return Ok(0);
    }

    // current: which identity currently occupies each path
    let mut current:   HashMap<String, i64> = HashMap::new();
    let mut id_count   = 0;

    for edge in &edges {
        let identity_id = if let Some(&id) = current.get(&edge.old_path) {
            id
        } else {
            // old_path starts a new chain — find its introducing commit
            let introduced_by = store
                .first_seen(&edge.old_path, repo_path)?
                .map(|c| c.hash)
                .unwrap_or_else(|| edge.commit_hash.clone());
            let id = store.insert_file_identity(repo_path)?;
            store.insert_path_observation(id, &edge.old_path, &introduced_by, None, repo_path)?;
            current.insert(edge.old_path.clone(), id);
            id_count += 1;
            id
        };

        store.supersede_path_observation(identity_id, &edge.old_path, &edge.commit_hash, repo_path)?;
        store.insert_path_observation(identity_id, &edge.new_path, &edge.commit_hash, None, repo_path)?;
        current.remove(&edge.old_path);
        current.insert(edge.new_path.clone(), identity_id);
    }

    // Detect path-reuse: a path that was renamed away but reappears in commit_files.
    // For each edge's old_path, check if new commits touched it AFTER the rename.
    let mut processed: HashSet<String> = HashSet::new();

    for edge in &edges {
        if processed.contains(&edge.old_path) { continue; }

        let reuse_commits = store.commits_for_file_after_ts(&edge.old_path, edge.timestamp, repo_path)?;
        if !reuse_commits.is_empty() {
            let first = &reuse_commits[0]; // oldest (ASC order)
            let new_id = store.insert_file_identity(repo_path)?;
            store.insert_path_observation(new_id, &edge.old_path, &first.hash, None, repo_path)?;
            id_count += 1;
            processed.insert(edge.old_path.clone());
        }
    }

    // Second pass: materialize commit membership.
    // For each path observation, commits within the observation's temporal window
    // [introduced_ts, superseded_ts) are assigned to the identity.  The temporal
    // bound is what prevents S1's service.rs from absorbing commits that reused
    // that path in S2's era.
    store.populate_identity_commits(repo_path)?;

    info!("rebuild_file_identities repo={} identities={}", repo_path, id_count);
    Ok(id_count)
}

/// Ingest rename evidence from Git's `--name-status` output.
///
/// This is a separate call from `ingest_git` — rename evidence builds the
/// foundation for v0.4 Historical Identity without affecting any path-scoped
/// queries.  `atlas context` remains path-scoped until Phase 5/6.
pub fn ingest_rename_evidence(repo_path: &str, store: &Store) -> Result<usize> {
    let connector = GitRepo::open(repo_path)?;
    let raw       = connector.log_renames_raw()?;
    let evidence  = git_renames::parse(&raw)?;
    let count     = evidence.len();

    info!(
        "connector=git capability=rename-evidence parsed={} records",
        count
    );

    for ev in &evidence {
        store.insert_rename_evidence(ev, repo_path)?;
    }

    Ok(count)
}

/// Ingest GitHub data from raw JSON strings (testable without the `gh` binary).
pub fn ingest_github_from_json(
    prs_json:   &str,
    issues_json: &str,
    repo_path:  &str,
    store:      &Store,
) -> Result<usize> {
    let prs    = gh_json::parse_prs(prs_json)?;
    let links  = gh_json::parse_pr_issue_links(prs_json)?;
    let issues = gh_json::parse_issues(issues_json)?;

    let pr_count = prs.len();

    info!("parsed={} PRs {} issues {} links", pr_count, issues.len(), links.len());

    for pr in &prs {
        store.insert_pull_request(pr, repo_path)?;
    }
    for (pr_number, issue_number) in &links {
        store.link_pr_to_issue(*pr_number, *issue_number, repo_path)?;
    }
    for issue in &issues {
        store.insert_issue(issue, repo_path)?;
    }

    Ok(pr_count)
}

pub fn ingest_github(repo_path: &str, store: &Store) -> Result<usize> {
    let pr_conn    = GitHubPrConnector::new(repo_path);
    let issue_conn = GitHubIssueConnector::new(repo_path);

    let prs_json    = pr_conn.fetch_raw()?.data;
    let issues_json = issue_conn.fetch_raw()?.data;

    ingest_github_from_json(&prs_json, &issues_json, repo_path, store)
}

/// Assemble a `ContextDocument` for `file` in `repo_path` from all available sources.
///
/// Resolution order: identity-scoped (when a FileIdentity chain exists for this path)
/// then path-scoped fallback.  The queried path is always the entry point; `current_path`
/// in FileIdentity tells the consumer where the artifact lives now when the queried path
/// is historical.
/// Extract TypeScript structural edges from all `.ts`/`.tsx` files under `repo_path`
/// and persist them to storage. Returns the number of edges inserted.
/// Classify an artifact's role from its path alone.
/// The heuristic is conservative — paths that don't clearly belong to a supporting
/// role are called ProductionSource.  Never promotes or demotes based on content.
fn classify_artifact_role(path: &str) -> ArtifactRole {
    let p = path.to_lowercase();

    // Test — any path segment named tests/test/spec/__tests__, or *.spec/.test suffix
    if p.starts_with("tests/") || p.starts_with("test/") || p.starts_with("spec/")
        || p.starts_with("__tests__/")
        || p.contains("/tests/") || p.contains("/test/") || p.contains("/spec/")
        || p.contains("/__tests__/")
        || p.ends_with(".spec.ts") || p.ends_with(".spec.js")
        || p.ends_with(".test.ts") || p.ends_with(".test.js")
    {
        return ArtifactRole::Test;
    }

    // Migration — scripts/migrations/ or any /migrations/ segment
    if p.contains("/migrations/") || p.starts_with("migrations/") {
        return ArtifactRole::Migration;
    }

    // Seeder — scripts/seeders/ or any /seeders/ or /seeds/ segment
    if p.contains("/seeders/") || p.contains("/seeds/")
        || p.starts_with("seeders/") || p.starts_with("seeds/")
    {
        return ArtifactRole::Seeder;
    }

    // Script — scripts/ prefix (migrations/ and seeders/ already caught above)
    if p.starts_with("scripts/") || p.contains("/scripts/") {
        return ArtifactRole::Script;
    }

    // Example — examples/ or example/ prefix
    if p.starts_with("examples/") || p.starts_with("example/") {
        return ArtifactRole::Example;
    }

    // Schema — GraphQL typeDefs directories or .graphql/.gql files
    if p.contains("/typedefs/") || p.contains("/type-defs/")
        || p.ends_with(".graphql") || p.ends_with(".gql")
        || p.ends_with(".schema.ts") || p.ends_with(".schema.js")
    {
        return ArtifactRole::Schema;
    }

    // Validation — /validation/ or /validators/ segment, or *.validation.ts suffix
    if p.contains("/validation/") || p.contains("/validators/")
        || p.ends_with(".validation.ts") || p.ends_with(".validator.ts")
    {
        return ArtifactRole::Validation;
    }

    // Permission — /permissions/ or /guards/ segment, or *.guard.ts suffix
    if p.contains("/permissions/") || p.contains("/guards/")
        || p.ends_with(".permission.ts") || p.ends_with(".guard.ts")
    {
        return ArtifactRole::Permission;
    }

    // Documentation — docs/ prefix or common doc extensions
    if p.starts_with("docs/") || p.ends_with(".md") || p.ends_with(".rst") || p.ends_with(".adoc") {
        return ArtifactRole::Documentation;
    }

    ArtifactRole::ProductionSource
}

/// English stopwords and framework-structural vocabulary excluded from concept expansion.
/// Framework vocabulary matches file paths but carries no domain meaning.
const EXPANSION_STOPWORDS: &[&str] = &[
    // English
    "a", "an", "the", "and", "or", "but", "for", "to", "in", "of", "at",
    "by", "with", "this", "that", "from", "into", "as", "is", "it", "its",
    "be", "been", "has", "have", "had", "not", "no", "on", "are", "was",
    "were", "will", "can", "all", "we", "our", "their", "which", "when",
    "where", "how", "who", "what", "any", "each", "new", "add", "use",
    "also", "more", "other", "than", "then", "so", "if", "do", "does",
    "did", "get", "set", "via", "per", "after", "before", "about", "over",
    "up", "out", "now", "just", "some", "should", "would", "could",
    "need", "may", "must", "make", "made", "both", "only", "same",
    "full", "four", "first", "last", "next", "total", "items", "item",
    // Framework / filesystem vocabulary — appear in file paths but are not domain concepts
    // Singular forms
    "model", "service", "handler", "controller", "module", "index",
    "type", "data", "base", "create", "update", "delete",
    "query", "error", "config", "util", "helper", "test", "spec",
    "mock", "stub", "fixture", "factory", "dto", "enum",
    "middleware", "guard", "interceptor", "filter", "decorator", "provider",
    "resolver", "schema", "registry", "constant", "export", "import",
    "class", "async", "await", "return", "function", "method", "field",
    "true", "false", "null", "undefined", "object", "array", "string",
    "number", "boolean", "interface", "abstract", "extends", "implements",
    "init", "setup", "seed", "script", "throw", "catch", "throw",
    "common", "global", "server", "chain", "second", "entry", "format",
    "validate", "document", "reference", "rules", "summary", "price",
    "account", "input", "output", "result", "response", "request",
    // Infrastructure / generic technical terms
    "cache", "memory", "limit", "logging", "logger", "log", "logs",
    "details", "overview", "dates", "date", "branch", "minor",
    "adapter", "implementation", "interface", "version", "value", "values",
    "token", "tokens", "header", "headers", "status", "state", "states",
    "event", "events", "command", "commands", "entity", "entities",
    "action", "actions", "steps", "step", "notes", "note", "changes",
    "feature", "features", "update", "updates", "fixes", "fixed",
    "issue", "issues", "param", "params", "argument", "arguments",
    "body", "path", "paths", "route", "routes", "endpoint", "endpoints",
    "layer", "layers", "level", "levels", "scope", "scopes",
    // Plural forms (separately because split produces them)
    "models", "services", "handlers", "controllers", "modules",
    "types", "queries", "errors", "configs", "utils", "helpers",
    "tests", "specs", "mocks", "stubs", "fixtures", "factories",
    "middlewares", "guards", "interceptors", "filters", "decorators",
    "providers", "resolvers", "schemas", "registries", "constants",
    "functions", "methods", "fields", "classes", "interfaces",
    "seeds", "scripts", "enums", "documents", "references",
    "rules", "mutations", "typedefs", "validations", "validators",
    "permissions", "helpers", "contracts", "resolvers", "services",
    "modules", "schemas", "graphql", "shield", "adapters",
    "implementations", "versions", "caches", "limits", "loggers",
];

/// Extract candidate expansion terms from a documentary text window.
/// Produces single tokens plus hyphenated bigrams and trigrams.
/// Hyphens within tokens (e.g. "conversion-history") are preserved.
fn extract_candidate_terms(
    text: &str,
    anchor: &str,
    existing: &std::collections::HashSet<String>,
) -> Vec<String> {
    let text_lower  = text.to_lowercase();
    let anchor_lower = anchor.to_lowercase();

    // Split on anything that is not alphanumeric or hyphen
    // This preserves "conversion-history" as a single token
    let raw: Vec<&str> = text_lower
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|t| !t.is_empty())
        .collect();

    let keep = |t: &str| -> bool {
        t.len() >= 5
            && !EXPANSION_STOPWORDS.contains(&t)
            && t != anchor_lower
            && !existing.contains(t)
    };

    let mut candidates: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Single tokens (hyphens preserved — "conversion-history" is already a good compound)
    for &tok in &raw {
        if keep(tok) { candidates.insert(tok.to_string()); }
    }

    // Build bigrams and trigrams from simple (no-hyphen) tokens only, to avoid
    // generating overly-long compounds from already-compound tokens
    let simple: Vec<&str> = raw.iter()
        .filter(|t| !t.contains('-'))
        .copied()
        .collect();

    for w in simple.windows(2) {
        let (a, b) = (w[0], w[1]);
        if EXPANSION_STOPWORDS.contains(&a) || EXPANSION_STOPWORDS.contains(&b) { continue; }
        if a.len() < 4 || b.len() < 4 { continue; }
        let c = format!("{}-{}", a, b);
        if !existing.contains(&c) && a != anchor_lower && b != anchor_lower {
            candidates.insert(c);
        }
    }

    for w in simple.windows(3) {
        let (a, b, c_tok) = (w[0], w[1], w[2]);
        if EXPANSION_STOPWORDS.contains(&a)
            || EXPANSION_STOPWORDS.contains(&b)
            || EXPANSION_STOPWORDS.contains(&c_tok)
        { continue; }
        if a.len() < 4 || b.len() < 4 || c_tok.len() < 4 { continue; }
        let c = format!("{}-{}-{}", a, b, c_tok);
        if !existing.contains(&c) {
            candidates.insert(c);
        }
    }

    candidates.into_iter().collect()
}

/// Extract a character window of `half_chars` on each side of the first
/// occurrence of `anchor` in `text`. Falls back to the first `half_chars*2`
/// characters if the anchor is not found.
fn extract_window(text: &str, anchor: &str, half_chars: usize) -> String {
    let lower = text.to_lowercase();
    let anchor_lower = anchor.to_lowercase();
    match lower.find(&anchor_lower) {
        None => text.chars().take(half_chars * 2).collect(),
        Some(byte_pos) => {
            let char_pos = text[..byte_pos].chars().count();
            let total = text.chars().count();
            let start = char_pos.saturating_sub(half_chars);
            let end = (char_pos + anchor.len() + half_chars).min(total);
            text.chars().skip(start).take(end - start).collect()
        }
    }
}

/// Phase 0: Concept Resolution.
///
/// For each anchor that matches fewer than MIN_FILE_PATH_MATCHES file paths
/// (i.e. the developer term is not directly a repository vocabulary term),
/// search documentary evidence, extract nearby tokens, verify against file
/// paths, and return a set of verified expansion terms.
///
/// Only documentary corpus (PR/issue bodies) is used as the bridge source —
/// preserving the Atlas epistemic model: expansion claims must be grounded
/// in what the team actually wrote, not in generic synonym databases.
fn resolve_concepts(
    anchors: &[&str],
    repo_path: &str,
    store: &Store,
) -> Result<(Vec<ConceptExpansion>, Vec<String>)> {
    const MIN_FILE_PATH_MATCHES: usize = 5;

    let mut existing: std::collections::HashSet<String> =
        anchors.iter().map(|s| s.to_lowercase()).collect();
    let mut expansions: Vec<ConceptExpansion> = Vec::new();

    for &anchor in anchors {
        let all_rows = store.search_anchor(anchor, repo_path)?;

        // Anchors already well-represented in file paths don't need vocabulary bridging
        let file_path_hits = all_rows.iter().filter(|r| r.source_type == "file_path").count();
        if file_path_hits >= MIN_FILE_PATH_MATCHES { continue; }

        // Find the richest documentary context for this anchor
        let doc_rows: Vec<_> = all_rows.into_iter()
            .filter(|r| r.source_type == "pr_body" || r.source_type == "issue_body")
            .collect();
        if doc_rows.is_empty() { continue; }

        // Use the first match as the bridge source
        let bridge     = &doc_rows[0];
        let bridge_num: i64 = bridge.source_id.parse().unwrap_or(0);
        let bridge_kind = if bridge.source_type == "pr_body" { "PR" } else { "Issue" };

        let window = extract_window(&bridge.text, anchor, 250);
        let candidates = extract_candidate_terms(&window, anchor, &existing);

        // Verify each candidate: it must appear in at least one file path
        let mut verified: Vec<VerifiedExpansion> = Vec::new();
        for candidate in &candidates {
            let hits = store.search_anchor(candidate, repo_path)?;
            if let Some(fh) = hits.iter().find(|r| r.source_type == "file_path") {
                verified.push(VerifiedExpansion {
                    term:        candidate.clone(),
                    verified_in: fh.source_id.clone(),
                });
                existing.insert(candidate.clone());
            }
        }

        // Prefer shorter (more specific) terms; cap at 8 expansions per anchor
        verified.sort_by_key(|v| v.term.len());
        verified.truncate(8);

        if !verified.is_empty() {
            expansions.push(ConceptExpansion {
                original_term:       anchor.to_string(),
                bridge_source:       format!("{} #{}", bridge_kind, bridge_num),
                bridge_snippet:      extract_snippet(&bridge.text, anchor, 60),
                verified_expansions: verified,
            });
        }
    }

    let effective: Vec<String> = existing.into_iter().collect();
    Ok((expansions, effective))
}

/// Compose anchor retrieval + structural observation + historical/documentary evidence
/// into a single provenance-preserving InvestigationDocument.
///
/// Algorithm (bounded, deterministic):
///   Phase 1 — Seed: anchor search → file-path matches become seed candidates.
///             PR/issue matches become documentary evidence.
///   Phase 2 — Expand: 1 structural hop from seed candidates only.
///             Structural neighbors added with StructuralNeighbor provenance.
///   Phase 3 — Structure: collect structural edges scoped to the candidate set.
///   Phase 4 — History: touch count + co-changes within candidates (threshold ≥ 2).
///   Phase 5 — Unresolved: seed candidates with no structural neighbors in set
///             and documentary evidence present in the investigation.
pub fn investigate(anchors: &[&str], repo_path: &str, store: &Store) -> Result<InvestigationDocument> {
    let pr_count    = store.pr_count(repo_path)?;
    let issue_count = store.issue_count(repo_path)?;
    let edge_count  = store.structural_edge_count(repo_path)?;

    let coverage = InvestigationCoverage {
        git_history:   store.commit_count(repo_path)? > 0,
        github_prs:    pr_count > 0,
        github_issues: issue_count > 0,
        file_paths:    true,
        es_imports:    edge_count > 0,
        static_calls:  edge_count > 0,
        model_refs:    edge_count > 0,
    };

    // ── Phase 0: Concept Resolution ───────────────────────────────────────────
    //
    // For anchors that don't map directly to file paths, search documentary
    // evidence (PR/issue bodies) for vocabulary bridges and verify candidate
    // expansion terms against the repository before adding them.  This expands
    // the anchor set only using vocabulary the team actually used.
    let (concept_expansions, effective_anchor_strs) =
        resolve_concepts(anchors, repo_path, store)?;
    let effective_anchor_refs: Vec<&str> =
        effective_anchor_strs.iter().map(String::as_str).collect();

    // ── Phase 1: Anchor search ────────────────────────────────────────────────
    let search_doc = search(&effective_anchor_refs, repo_path, store)?;

    // Indexed: (kind, number) → DocumentaryEvidence
    let mut doc_map: std::collections::HashMap<(String, i64), DocumentaryEvidence> =
        std::collections::HashMap::new();

    // candidates: file → Vec<CandidateReason>
    let mut candidates: indexmap::IndexMap<String, Vec<CandidateReason>> =
        indexmap::IndexMap::new();

    for m in &search_doc.matches {
        match &m.source {
            MatchSource::FilePath => {
                candidates
                    .entry(m.source_id.clone())
                    .or_default()
                    .push(CandidateReason::AnchorMatch {
                        anchor: m.anchor.clone(),
                        via:    "file_path".to_string(),
                    });
            }
            MatchSource::CommitMessage => {
                // Commit message matches are historical evidence — don't add file as candidate
                // (too indirect and would require a commit→file join for v0.5c).
            }
            MatchSource::PrTitle | MatchSource::PrBody => {
                let number: i64 = m.source_id.parse().unwrap_or(0);
                let entry = doc_map
                    .entry(("pr".to_string(), number))
                    .or_insert_with(|| DocumentaryEvidence {
                        kind:            "pr".to_string(),
                        number,
                        title:           String::new(),
                        matched_anchors: Vec::new(),
                        snippets:        Vec::new(),
                    });
                if !entry.matched_anchors.contains(&m.anchor) {
                    entry.matched_anchors.push(m.anchor.clone());
                }
                if m.source == MatchSource::PrTitle && entry.title.is_empty() {
                    entry.title = m.snippet.clone();
                }
                if m.source == MatchSource::PrBody {
                    entry.snippets.push(m.snippet.clone());
                }
            }
            MatchSource::IssueTitle | MatchSource::IssueBody => {
                let number: i64 = m.source_id.parse().unwrap_or(0);
                let entry = doc_map
                    .entry(("issue".to_string(), number))
                    .or_insert_with(|| DocumentaryEvidence {
                        kind:            "issue".to_string(),
                        number,
                        title:           String::new(),
                        matched_anchors: Vec::new(),
                        snippets:        Vec::new(),
                    });
                if !entry.matched_anchors.contains(&m.anchor) {
                    entry.matched_anchors.push(m.anchor.clone());
                }
                if m.source == MatchSource::IssueTitle && entry.title.is_empty() {
                    entry.title = m.snippet.clone();
                }
                if m.source == MatchSource::IssueBody {
                    entry.snippets.push(m.snippet.clone());
                }
            }
        }
    }

    // ── Seed scoring and trimming ─────────────────────────────────────────────
    //
    // When a broad anchor like "model" matches dozens of files, prefer candidates
    // that matched more distinct anchors.  Trim to MAX_SEEDS before expansion so
    // the investigation stays focused on the most-relevant neighborhood.
    const MAX_SEEDS: usize = 20;
    if candidates.len() > MAX_SEEDS {
        let mut scored: Vec<(String, Vec<CandidateReason>, usize)> = candidates
            .into_iter()
            .map(|(file, reasons)| {
                let anchor_count = reasons.iter()
                    .filter_map(|r| match r {
                        CandidateReason::AnchorMatch { anchor, .. } => Some(anchor.as_str()),
                        _ => None,
                    })
                    .collect::<std::collections::HashSet<_>>()
                    .len();
                (file, reasons, anchor_count)
            })
            .collect();
        scored.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
        scored.truncate(MAX_SEEDS);
        candidates = scored.into_iter()
            .map(|(file, reasons, _)| (file, reasons))
            .collect();
    }

    // ── Phase 2: 1-hop semantic expansion from seed candidates ───────────────
    //
    // Expansion triggers (both directions):
    //   outgoing CALLS_STATIC + REFERENCES_MODEL — what this seed calls/queries
    //   incoming CALLS_STATIC + REFERENCES_MODEL — who calls/queries this seed
    //
    // IMPORTS is excluded in both directions: every file imports utilities and
    // types — including it would inflate the investigation with infrastructure
    // files (e.g. mongoose, logger, config).
    //
    // Incoming REFERENCES_MODEL specifically matters for model files: models have
    // no outgoing edges of their own, but the services that query them (via
    // Model.findOne() etc.) are precisely the structural neighbors we want.
    let seed_files: Vec<String> = candidates.keys().cloned().collect();

    for seed in &seed_files {
        let outgoing = store.structural_edges_for_file(seed, repo_path)?;
        for edge in &outgoing {
            if edge.target_file.starts_with("UNRESOLVED:") { continue; }
            if edge.kind != "calls_static" && edge.kind != "calls_instance" && edge.kind != "references_model" { continue; }
            candidates
                .entry(edge.target_file.clone())
                .or_default()
                .push(CandidateReason::StructuralNeighbor {
                    from_file:  seed.clone(),
                    kind:       edge.kind.clone(),
                    direction:  "outgoing".to_string(),
                });
        }

        let incoming = store.structural_edges_targeting(seed, repo_path)?;
        for edge in &incoming {
            if edge.source_file.starts_with("UNRESOLVED:") { continue; }
            if edge.kind != "calls_static" && edge.kind != "calls_instance" && edge.kind != "references_model" { continue; }
            candidates
                .entry(edge.source_file.clone())
                .or_default()
                .push(CandidateReason::StructuralNeighbor {
                    from_file:  seed.clone(),
                    kind:       edge.kind.clone(),
                    direction:  "incoming".to_string(),
                });
        }
    }

    let candidate_set: std::collections::HashSet<String> =
        candidates.keys().cloned().collect();

    // ── Phase 3: Structural observations scoped to candidate set ─────────────
    let mut observed_structure: Vec<StructuralObservation> = Vec::new();

    for file in candidates.keys() {
        let out_raw = store.structural_edges_for_file(file, repo_path)?;
        let inc_raw = store.structural_edges_targeting(file, repo_path)?;

        let outgoing: Vec<StructuralEdgeSummary> = out_raw.iter()
            .filter(|e| candidate_set.contains(&e.target_file))
            .map(|e| StructuralEdgeSummary {
                file:   e.target_file.clone(),
                kind:   e.kind.clone(),
                symbol: e.target_symbol.clone(),
            })
            .collect();
        let incoming: Vec<StructuralEdgeSummary> = inc_raw.iter()
            .filter(|e| candidate_set.contains(&e.source_file))
            .map(|e| StructuralEdgeSummary {
                file:   e.source_file.clone(),
                kind:   e.kind.clone(),
                symbol: e.target_symbol.clone(),
            })
            .collect();

        observed_structure.push(StructuralObservation {
            file:     file.clone(),
            outgoing,
            incoming,
        });
    }

    // ── Phase 4: Historical evidence ──────────────────────────────────────────
    let mut historical: Vec<HistoricalEntry> = Vec::new();

    for file in candidates.keys() {
        let touch_count = store.touch_count(file, repo_path)?;
        let co_raw = store.co_changes_for_file(file, repo_path, 2)?;
        let co_changed_candidates: Vec<String> = co_raw.iter()
            .filter(|c| candidate_set.contains(&c.file_path))
            .map(|c| c.file_path.clone())
            .collect();

        if touch_count > 0 || !co_changed_candidates.is_empty() {
            historical.push(HistoricalEntry {
                file:                  file.clone(),
                touch_count,
                co_changed_candidates,
            });
        }
    }

    // ── Phase 5: Unresolved connections ───────────────────────────────────────
    //
    // Only flag seed candidates that satisfy all three conditions:
    //   1. Matched 2+ distinct anchors (single-anchor broad terms like "model" are
    //      excluded — they don't indicate a specific unresolved relationship)
    //   2. No structural neighbors in the candidate set
    //   3. At least one documentary item matched one of those same anchors
    //      (the documentary evidence is specifically about this seed, not incidental)
    let mut unresolved: Vec<UnresolvedConnection> = Vec::new();

    for (file, reasons) in &candidates {
        let is_seed = reasons.iter().any(|r| matches!(r, CandidateReason::AnchorMatch { .. }));
        if !is_seed { continue; }

        // Unresolved connections are only meaningful for production source files.
        // Tests/migrations/schemas being structurally isolated is expected, not notable.
        if classify_artifact_role(file) != ArtifactRole::ProductionSource { continue; }

        let matched_anchors: std::collections::HashSet<&str> = reasons.iter()
            .filter_map(|r| match r {
                CandidateReason::AnchorMatch { anchor, .. } => Some(anchor.as_str()),
                _ => None,
            })
            .collect();

        if matched_anchors.len() < 2 { continue; }

        let has_structural_neighbor = observed_structure.iter()
            .find(|obs| obs.file == *file)
            .map(|obs| !obs.outgoing.is_empty() || !obs.incoming.is_empty())
            .unwrap_or(false);

        if has_structural_neighbor { continue; }

        let specific_doc = doc_map.values().find(|doc| {
            matched_anchors.iter().any(|a| doc.matched_anchors.contains(&a.to_string()))
        });

        if let Some(doc) = specific_doc {
            unresolved.push(UnresolvedConnection {
                subject: file.clone(),
                documentary_indication: Some(format!(
                    "{} #{}: {}",
                    doc.kind.to_uppercase(),
                    doc.number,
                    if doc.title.is_empty() { "(no title)" } else { &doc.title }
                )),
                observation: "No structural edges to other candidates observed \
                              (ES imports, static calls, model references).".to_string(),
            });
        }
    }

    // ── Assemble ──────────────────────────────────────────────────────────────
    let mut core_candidates: Vec<CandidateArtifact> = Vec::new();
    let mut supporting_artifacts: Vec<CandidateArtifact> = Vec::new();

    for (file, reasons) in candidates {
        let role = classify_artifact_role(&file);
        let artifact = CandidateArtifact { file, role: role.clone(), reasons };
        if role == ArtifactRole::ProductionSource {
            core_candidates.push(artifact);
        } else {
            supporting_artifacts.push(artifact);
        }
    }
    core_candidates.sort_by(|a, b| a.file.cmp(&b.file));
    supporting_artifacts.sort_by(|a, b| a.file.cmp(&b.file));

    let mut documentary: Vec<DocumentaryEvidence> = doc_map.into_values().collect();
    documentary.sort_by_key(|d| (d.kind.clone(), d.number));

    observed_structure.sort_by(|a, b| a.file.cmp(&b.file));
    historical.sort_by(|a, b| b.touch_count.cmp(&a.touch_count).then(a.file.cmp(&b.file)));
    unresolved.sort_by(|a, b| a.subject.cmp(&b.subject));

    let mut effective_anchors: Vec<String> = effective_anchor_strs;
    effective_anchors.sort();

    Ok(InvestigationDocument {
        schema_version:    3,
        anchors:           anchors.iter().map(|s| s.to_string()).collect(),
        effective_anchors,
        concept_expansions,
        core_candidates,
        supporting_artifacts,
        observed_structure,
        documentary,
        historical,
        unresolved,
        coverage,
    })
}

pub fn ingest_typescript(repo_path: &str, store: &Store) -> Result<usize> {
    let (edges, _file_count) = ts_structural::extract_all(repo_path);
    let count = edges.len();
    for edge in &edges {
        store.insert_structural_edge(edge, repo_path)?;
    }
    info!("typescript structural edges inserted={}", count);
    Ok(count)
}

pub fn build_context(file: &str, repo_path: &str, store: &Store) -> Result<ContextDocument> {
    // Attempt identity-scoped resolution.
    // Returns None when: no identity chain exists for this path, or path-reuse ambiguity
    // (identities_for_path > 1).  Ambiguous paths fall back to path-scoped to avoid
    // silently conflating two distinct artifacts.
    let identity_id = store.resolve_path_to_identity(file, repo_path)?;

    let (first_commit, last_commit, touch_count, all_commits, current_path, is_historical_path) =
        if let Some(id) = identity_id {
            // Identity-scoped: commits span the full artifact history across all renames.
            let commits = store.commits_for_identity(id, repo_path)?;
            // commits_for_identity is sorted newest-first (DESC).
            let first = commits.last().cloned();  // oldest commit in the identity's history
            let last  = commits.first().cloned(); // newest commit
            let count = commits.len() as i64;

            // Navigation hint: is the queried path the current canonical path for this identity?
            let is_current_occupant = store.resolve_current_path(file, repo_path)?.is_some();
            let (curr_path, is_hist) = if is_current_occupant {
                (None, false)
            } else {
                // Historical path — locate the current canonical path from path observations.
                let history = store.path_history_for_identity(id, repo_path)?;
                let current_obs = history.iter().find(|o| o.superseded_by_commit.is_none());
                (current_obs.map(|o| o.path.clone()), true)
            };

            (first, last, count, commits, curr_path, is_hist)
        } else {
            // Path-scoped fallback: correct for exact path, may be missing pre-rename history.
            let first   = store.first_seen(file, repo_path)?;
            let last    = store.last_seen(file, repo_path)?;
            let count   = store.touch_count(file, repo_path)?;
            let commits = store.commits_for_file(file, repo_path)?;
            (first, last, count, commits, None, false)
        };

    // Newest-first for recent activity display (both code paths produce DESC-sorted commits).

    // PRs whose merge commit touched this file, plus issue linkage.
    let pr_rows    = store.prs_for_file(file, repo_path)?;
    let issue_rows = store.issues_for_file(file, repo_path)?;

    let mut prs = Vec::with_capacity(pr_rows.len());
    for pr in &pr_rows {
        let linked = store.issue_numbers_for_pr(pr.number, repo_path)?;
        prs.push(PrSummary {
            number:           pr.number,
            title:            pr.title.clone(),
            state:            pr.state.clone(),
            merge_commit_sha: pr.merge_commit_sha.clone(),
            linked_issues:    linked,
        });
    }
    let issues: Vec<IssueSummary> = issue_rows.iter().map(|i| IssueSummary {
        number: i.number,
        title:  i.title.clone(),
        state:  i.state.clone(),
    }).collect();

    // Co-changes — identity-aware when an identity chain exists, path-scoped otherwise.
    // Identity-aware sees coupling across all path phases (pre- and post-rename).
    let co_changes = match identity_id {
        Some(id) => store.co_changes_for_identity(id, repo_path, 1)?,
        None     => store.co_changes_for_file(file, repo_path, 1)?,
    };
    let (doc_changes, src_changes): (Vec<_>, Vec<_>) = co_changes
        .into_iter()
        .partition(|e| is_documentary(&e.file_path));

    let coupling: Vec<CouplingEntry> = src_changes.iter().map(|c| CouplingEntry {
        file_path:    c.file_path.clone(),
        change_count: c.change_count,
    }).collect();
    let documentary: Vec<CouplingEntry> = doc_changes.iter().map(|c| CouplingEntry {
        file_path:    c.file_path.clone(),
        change_count: c.change_count,
    }).collect();

    // Significance: rank using identity-aware hot list when identities are present.
    // The ranking path is the canonical (current) path for this artifact — historical
    // paths do not appear independently in the ranking since they are dead paths.
    let canonical_path = if is_historical_path {
        current_path.as_deref().unwrap_or(file)
    } else {
        file
    };
    let hot_all = if identity_id.is_some() {
        store.hot_files_identity_aware(repo_path, 9999)?
    } else {
        store.hot_files(repo_path, 9999)?
    };
    let significance = compute_significance(canonical_path, touch_count, &hot_all);

    // Coverage map — what sources are present?
    let repo_commits  = store.commit_count(repo_path)?;
    let repo_prs      = store.pr_count(repo_path)?;
    let repo_issues   = store.issue_count(repo_path)?;
    let has_identities = store.has_materialized_identities(repo_path)?;
    let doc_status    = if documentary.is_empty() {
        CoverageStatus::NotIngested
    } else {
        CoverageStatus::CoChangeOnly
    };

    let co_count = coupling.len() + documentary.len();
    let evidence = EvidenceSummary {
        commits:     all_commits.len(),
        prs:         prs.len(),
        issues:      issues.len(),
        co_changes:  co_count,
        total_facts: all_commits.len() + prs.len() + issues.len() + co_count,
    };

    Ok(ContextDocument {
        schema_version: 2,
        subject: file.to_string(),
        identity: FileIdentity {
            first_commit:       first_commit.map(row_to_summary),
            last_commit:        last_commit.map(row_to_summary),
            touch_count,
            current_path,
            is_historical_path,
        },
        recent_activity: all_commits.into_iter().map(row_to_summary).collect(),
        related_history: RelatedHistory { pull_requests: prs, issues },
        coupling,
        documentary,
        significance,
        evidence,
        coverage: CoverageMap {
            // PathScoped: git history is available but scoped to the exact path.
            // Pre-rename history at other paths is not tracked (no --follow / rename detection).
            git_history:     if repo_commits > 0 { CoverageStatus::PathScoped } else { CoverageStatus::NotIngested },
            rename_tracking: if has_identities { CoverageStatus::Available } else { CoverageStatus::NotIngested },
            github_prs:      if repo_prs    > 0 { CoverageStatus::Available } else { CoverageStatus::NotIngested },
            github_issues:   if repo_issues > 0 { CoverageStatus::Available } else { CoverageStatus::NotIngested },
            documentation:   doc_status,
            working_tree:    CoverageStatus::NotIngested,
        },
    })
}

/// Search the ingested corpus for one or more anchor terms.
///
/// For each anchor, Atlas searches: file paths (Observed), commit messages
/// (Historical), PR and issue titles and bodies (Documentary).  Results are
/// deduplicated by (anchor, source_type, source_id) so large PR bodies that
/// mention an anchor many times produce a single match.
///
/// Evidence type is assigned by source:
/// - FilePath → Observed (code artifact exists)
/// - CommitMessage → Historical (something happened)
/// - PrTitle / PrBody / IssueTitle / IssueBody → Documentary (someone wrote it)
pub fn search(anchors: &[&str], repo_path: &str, store: &Store) -> Result<SearchDocument> {
    let pr_count    = store.pr_count(repo_path)?;
    let issue_count = store.issue_count(repo_path)?;
    let repo_commits = store.commit_count(repo_path)?;

    let coverage = SearchCoverage {
        file_paths:     true,
        commit_history: repo_commits > 0,
        pull_requests:  pr_count > 0,
        issues:         issue_count > 0,
        source_code:    false,
        working_tree:   false,
    };

    // Collect matches across all anchors, deduplicating by (anchor, source_type, source_id).
    let mut seen:    HashSet<(String, String, String)> = HashSet::new();
    let mut matches: Vec<AnchorMatch> = Vec::new();

    for &anchor in anchors {
        let rows = store.search_anchor(anchor, repo_path)?;
        for row in rows {
            let key = (row.anchor.clone(), row.source_type.clone(), row.source_id.clone());
            if !seen.insert(key) {
                continue;
            }
            let (source, evidence_type) = classify_source(&row.source_type);
            let snippet = extract_snippet(&row.text, &row.anchor, 80);
            matches.push(AnchorMatch {
                anchor:        row.anchor,
                source,
                source_id:     row.source_id,
                snippet,
                evidence_type,
            });
        }
    }

    // Sort: Observed first, then Documentary, then Historical.
    // Within each group: shorter source_ids (file paths) before longer ones.
    matches.sort_by(|a, b| {
        a.evidence_type.cmp(&b.evidence_type)
            .then(a.source_id.len().cmp(&b.source_id.len()))
            .then(a.source_id.cmp(&b.source_id))
            .then(a.anchor.cmp(&b.anchor))
    });

    Ok(SearchDocument {
        schema_version: 1,
        anchors: anchors.iter().map(|s| s.to_string()).collect(),
        matches,
        coverage,
    })
}

fn classify_source(source_type: &str) -> (MatchSource, EvidenceType) {
    match source_type {
        "file_path"       => (MatchSource::FilePath,      EvidenceType::Observed),
        "commit_message"  => (MatchSource::CommitMessage, EvidenceType::Historical),
        "pr_title"        => (MatchSource::PrTitle,       EvidenceType::Documentary),
        "pr_body"         => (MatchSource::PrBody,        EvidenceType::Documentary),
        "issue_title"     => (MatchSource::IssueTitle,    EvidenceType::Documentary),
        "issue_body"      => (MatchSource::IssueBody,     EvidenceType::Documentary),
        _                 => (MatchSource::FilePath,      EvidenceType::Observed),
    }
}

/// Extract a text window around the first occurrence of `anchor` in `text`.
/// Returns the full text unchanged when it fits within 2×window characters.
/// Uses character boundaries, not byte offsets, for correct Unicode handling.
fn extract_snippet(text: &str, anchor: &str, window: usize) -> String {
    // Short texts — return as-is.
    if text.chars().count() <= window * 2 {
        return text.trim().to_string();
    }
    let lower      = text.to_lowercase();
    let lower_anch = anchor.to_lowercase();
    let byte_pos   = match lower.find(&lower_anch) {
        Some(p) => p,
        None    => return text.chars().take(window * 2).collect(),
    };
    // Map byte position → char position.
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let char_pos  = chars.partition_point(|(b, _)| *b < byte_pos);
    let start_c   = char_pos.saturating_sub(window);
    let end_c     = (char_pos + anchor.chars().count() + window).min(chars.len());
    let start_b   = chars[start_c].0;
    let end_b     = if end_c < chars.len() { chars[end_c].0 } else { text.len() };
    let prefix    = if start_c > 0         { "…" } else { "" };
    let suffix    = if end_c < chars.len() { "…" } else { "" };
    // Strip surrounding whitespace/newlines from the window.
    let inner = text[start_b..end_b].trim().replace('\n', " ").replace('\r', "");
    format!("{}{}{}", prefix, inner, suffix)
}

fn row_to_summary(c: CommitRow) -> CommitSummary {
    CommitSummary {
        short_hash: c.short_hash,
        message:    c.message,
        author:     c.author_name,
        timestamp:  c.timestamp,
    }
}

// Document discovery uses a path-based heuristic.
//
// This is discovery evidence, not document ontology.
// A path matching these patterns is treated as a document candidate
// for context assembly under CoverageStatus::CoChangeOnly.
//
// Future sources may include: nested READMEs, RFC directories, exported
// Notion documents, PDFs, office documents, browser captures, and chat
// attachments.  Do not treat this heuristic as authoritative classification.
fn is_documentary(path: &str) -> bool {
    path.starts_with("docs/")
        || path.ends_with(".md")
        || path.ends_with(".rst")
        || path.ends_with(".adoc")
}

fn compute_significance(file: &str, touch_count: i64, hot: &[HotFileRow]) -> Option<FileSignificance> {
    if touch_count == 0 {
        return None;
    }
    let total = hot.len();
    let rank  = hot.iter().position(|r| r.file_path == file)
                   .map(|i| i + 1)
                   .unwrap_or(total + 1);
    Some(FileSignificance { rank, total_files: total, touch_count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_storage::Store;
    use std::process::Command;
    use tempfile::TempDir;

    // ── Fixture git repo builder ────────────────────────────────────────────

    struct FixtureRepo {
        _dir:    TempDir,
        pub path:   String,
        pub hash_a: String,
        pub hash_b: String,
        #[allow(dead_code)]
        pub hash_c: String,
    }

    fn create_fixture_repo() -> FixtureRepo {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().to_str().unwrap().to_string();

        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(&p)
                .output()
                .expect("git");
            assert!(
                status.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&status.stderr)
            );
        };

        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "test@test.com"]);
        git(&["config", "user.name", "Test"]);

        let commit_dated = |path: &str, msg: &str, date: &str| {
            let out = Command::new("git")
                .args(["commit", "-m", msg])
                .current_dir(path)
                .env("GIT_AUTHOR_DATE",    date)
                .env("GIT_COMMITTER_DATE", date)
                .output()
                .expect("git commit");
            assert!(out.status.success(), "commit failed: {}", String::from_utf8_lossy(&out.stderr));
        };

        // Commit A — creates auth.ts (2024-01-01)
        std::fs::write(format!("{p}/auth.ts"), "export {}").unwrap();
        git(&["add", "auth.ts"]);
        commit_dated(&p, "Add authentication module", "2024-01-01T10:00:00+0000");
        let hash_a = head_hash(&p);

        // Commit B — modifies auth.ts, creates user.ts (2024-01-02)
        std::fs::write(format!("{p}/auth.ts"), "export function auth() {}").unwrap();
        std::fs::write(format!("{p}/user.ts"), "export {}").unwrap();
        git(&["add", "auth.ts", "user.ts"]);
        commit_dated(&p, "Add user model, extend auth", "2024-01-02T10:00:00+0000");
        let hash_b = head_hash(&p);

        // Commit C — modifies user.ts (2024-01-03)
        std::fs::write(format!("{p}/user.ts"), "export function getUser() {}").unwrap();
        git(&["add", "user.ts"]);
        commit_dated(&p, "Add getUser function", "2024-01-03T10:00:00+0000");
        let hash_c = head_hash(&p);

        FixtureRepo { _dir: dir, path: p, hash_a, hash_b, hash_c }
    }

    fn head_hash(repo: &str) -> String {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .expect("rev-parse");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn fixture_prs_json(merge_commit_sha: &str) -> String {
        format!(r#"[{{
            "number": 12,
            "title": "Add user authentication",
            "state": "MERGED",
            "body": "Closes #10",
            "author": {{"login": "alice"}},
            "mergeCommit": {{"oid": "{merge_commit_sha}"}},
            "closingIssuesReferences": [{{"number": 10}}]
        }}]"#)
    }

    const FIXTURE_ISSUES_JSON: &str = r#"[{
        "number": 10,
        "title": "Add user authentication",
        "state": "CLOSED",
        "body": null,
        "author": {"login": "alice"}
    }]"#;

    // ── Tests ───────────────────────────────────────────────────────────────

    #[test]
    fn ingest_git_extracts_three_commits() {
        let fixture = create_fixture_repo();
        let store = Store::open(":memory:").unwrap();

        let count = ingest_git(&fixture.path, &store).unwrap();
        assert_eq!(count, 3, "expected 3 commits");
    }

    #[test]
    fn ingest_git_correct_files_per_commit() {
        let fixture = create_fixture_repo();
        let store = Store::open(":memory:").unwrap();
        ingest_git(&fixture.path, &store).unwrap();

        // auth.ts is touched by commits A and B
        let auth = store.commits_for_file("auth.ts", &fixture.path).unwrap();
        assert_eq!(auth.len(), 2, "auth.ts should have exactly 2 commits");

        // user.ts is touched by commits B and C
        let user = store.commits_for_file("user.ts", &fixture.path).unwrap();
        assert_eq!(user.len(), 2, "user.ts should have exactly 2 commits");
    }

    #[test]
    fn ingest_git_twice_no_duplication() {
        let fixture = create_fixture_repo();
        let store = Store::open(":memory:").unwrap();
        ingest_git(&fixture.path, &store).unwrap();
        ingest_git(&fixture.path, &store).unwrap();
        assert_eq!(store.commit_count(&fixture.path).unwrap(), 3);
    }

    #[test]
    fn ingest_github_from_json_full_scenario() {
        let fixture = create_fixture_repo();
        let store = Store::open(":memory:").unwrap();

        // Ingest git so the PR→commit→file chain can be resolved.
        ingest_git(&fixture.path, &store).unwrap();

        let prs_json = fixture_prs_json(&fixture.hash_b);
        ingest_github_from_json(&prs_json, FIXTURE_ISSUES_JSON, &fixture.path, &store).unwrap();

        // PR #12 should appear for auth.ts (touched by commit B = merge commit)
        let auth_prs = store.prs_for_file("auth.ts", &fixture.path).unwrap();
        assert_eq!(auth_prs.len(), 1);
        assert_eq!(auth_prs[0].number, 12);

        // Issue #10 should be reachable from auth.ts via PR #12
        let auth_issues = store.issues_for_file("auth.ts", &fixture.path).unwrap();
        assert_eq!(auth_issues.len(), 1);
        assert_eq!(auth_issues[0].number, 10);
    }

    #[test]
    fn ingest_github_twice_no_duplication() {
        let fixture = create_fixture_repo();
        let store = Store::open(":memory:").unwrap();
        ingest_git(&fixture.path, &store).unwrap();

        let prs_json = fixture_prs_json(&fixture.hash_b);
        ingest_github_from_json(&prs_json, FIXTURE_ISSUES_JSON, &fixture.path, &store).unwrap();
        ingest_github_from_json(&prs_json, FIXTURE_ISSUES_JSON, &fixture.path, &store).unwrap();

        let auth_prs = store.prs_for_file("auth.ts", &fixture.path).unwrap();
        assert_eq!(auth_prs.len(), 1);
        let auth_issues = store.issues_for_file("auth.ts", &fixture.path).unwrap();
        assert_eq!(auth_issues.len(), 1);
    }

    #[test]
    fn malformed_github_json_returns_error_not_panic() {
        let store = Store::open(":memory:").unwrap();
        let result = ingest_github_from_json("{bad json}", "[]", ".", &store);
        assert!(result.is_err());
    }

    #[test]
    fn commit_hashes_survive_pipeline() {
        let fixture = create_fixture_repo();
        let store = Store::open(":memory:").unwrap();
        ingest_git(&fixture.path, &store).unwrap();

        // Both hash_a and hash_b should appear for auth.ts
        let auth_commits = store.commits_for_file("auth.ts", &fixture.path).unwrap();
        let short_hashes: Vec<String> = auth_commits.iter().map(|c| c.short_hash.clone()).collect();

        let expected_short_a = &fixture.hash_a[..7];
        let expected_short_b = &fixture.hash_b[..7];

        assert!(
            short_hashes.iter().any(|h| h == expected_short_a),
            "commit A short hash {} not found in {:?}", expected_short_a, short_hashes
        );
        assert!(
            short_hashes.iter().any(|h| h == expected_short_b),
            "commit B short hash {} not found in {:?}", expected_short_b, short_hashes
        );
    }

    #[test]
    fn build_context_identity_and_coverage() {
        let fixture = create_fixture_repo();
        let store   = Store::open(":memory:").unwrap();
        ingest_git(&fixture.path, &store).unwrap();

        let doc = build_context("auth.ts", &fixture.path, &store).unwrap();

        assert_eq!(doc.identity.touch_count, 2);
        assert_eq!(doc.recent_activity.len(), 2);
        assert_eq!(
            doc.identity.first_commit.as_ref().unwrap().short_hash,
            &fixture.hash_a[..7]
        );
        assert_eq!(doc.coverage.git_history,     atlas_ir::CoverageStatus::PathScoped);
        assert_eq!(doc.coverage.rename_tracking, atlas_ir::CoverageStatus::NotIngested);
        assert_eq!(doc.coverage.github_prs,      atlas_ir::CoverageStatus::NotIngested);
        assert_eq!(doc.evidence.commits, 2);
        assert_eq!(doc.evidence.prs,     0);
        assert_eq!(doc.evidence.issues,  0);
    }

    #[test]
    fn build_context_coupling_and_github() {
        let fixture = create_fixture_repo();
        let store   = Store::open(":memory:").unwrap();
        ingest_git(&fixture.path, &store).unwrap();

        let prs_json = fixture_prs_json(&fixture.hash_b);
        ingest_github_from_json(&prs_json, FIXTURE_ISSUES_JSON, &fixture.path, &store).unwrap();

        let doc = build_context("auth.ts", &fixture.path, &store).unwrap();

        // user.ts changed in the same commit as auth.ts (commit B)
        assert!(
            doc.coupling.iter().any(|e| e.file_path == "user.ts"),
            "user.ts missing from coupling: {:?}", doc.coupling
        );

        assert_eq!(doc.coverage.github_prs,    atlas_ir::CoverageStatus::Available);
        assert_eq!(doc.coverage.github_issues, atlas_ir::CoverageStatus::Available);
        assert_eq!(doc.related_history.pull_requests.len(), 1);
        assert_eq!(doc.related_history.pull_requests[0].number, 12);
        assert_eq!(doc.related_history.pull_requests[0].linked_issues, vec![10]);
        assert_eq!(doc.related_history.issues.len(), 1);
        assert_eq!(doc.related_history.issues[0].number, 10);
        assert!(doc.evidence.total_facts > 0);
    }

    #[test]
    fn build_context_unknown_file_returns_empty_not_error() {
        let fixture = create_fixture_repo();
        let store   = Store::open(":memory:").unwrap();
        ingest_git(&fixture.path, &store).unwrap();

        let doc = build_context("nonexistent.ts", &fixture.path, &store).unwrap();
        assert_eq!(doc.identity.touch_count, 0);
        assert!(doc.recent_activity.is_empty());
        assert!(doc.significance.is_none());
    }
}

// ─── Review Context (v0.7b) ───────────────────────────────────────────────────

/// Assemble a `ReviewContextDocument` for `pr_number` in `repo_path`.
///
/// Algorithm:
///   1. Fetch PR metadata and changed files via `gh pr view <pr_number>`.
///   2. Use the changed files as mandatory seeds — no scoring, no trimming.
///   3. Per file: structural edges (unscoped), co-changes, historical summary.
///   4. Search the documentary corpus for terms in the PR title.
///   5. Return the assembled document.
pub fn build_review_context(
    pr_number: u64,
    repo_path: &str,
    store: &Store,
) -> Result<ReviewContextDocument> {
    use std::process::Command;

    // ── Phase 1: Fetch PR metadata from GitHub ────────────────────────────────
    let out = Command::new("gh")
        .args([
            "pr", "view",
            &pr_number.to_string(),
            "--json", "title,body,closingIssuesReferences,files",
        ])
        .current_dir(repo_path)
        .output()
        .context("gh not found — install with: nix profile install nixpkgs#gh")?;

    anyhow::ensure!(
        out.status.success(),
        "gh pr view {} failed: {}",
        pr_number,
        String::from_utf8_lossy(&out.stderr)
    );

    let json = String::from_utf8(out.stdout)?;
    let detail = gh_json::parse_pr_detail(&json)?;

    let pr_file_set: std::collections::HashSet<String> =
        detail.changed_files.iter().cloned().collect();

    // ── Phase 2: Coverage probes ──────────────────────────────────────────────
    let pr_count       = store.pr_count(repo_path)?;
    let issue_count    = store.issue_count(repo_path)?;
    let edge_count     = store.structural_edge_count(repo_path)?;

    // ── Phase 3: Per-file context ─────────────────────────────────────────────
    let mut pr_files: Vec<PrFileContext> = Vec::new();
    let mut any_history = false;

    for file in &detail.changed_files {
        let touch_count = store.touch_count(file, repo_path)?;
        if touch_count > 0 { any_history = true; }

        let last_commit_message = store
            .commits_for_file(file, repo_path)?
            .into_iter()
            .next()
            .map(|c| c.message);

        let out_raw = store.structural_edges_for_file(file, repo_path)?;
        let in_raw  = store.structural_edges_targeting(file, repo_path)?;

        let structural_out: Vec<StructuralEdgeSummary> = out_raw.iter()
            .filter(|e| !e.target_file.starts_with("UNRESOLVED:"))
            .map(|e| StructuralEdgeSummary {
                file:   e.target_file.clone(),
                kind:   e.kind.clone(),
                symbol: e.target_symbol.clone(),
            })
            .collect();

        let structural_in: Vec<StructuralEdgeSummary> = in_raw.iter()
            .filter(|e| !e.source_file.starts_with("UNRESOLVED:"))
            .map(|e| StructuralEdgeSummary {
                file:   e.source_file.clone(),
                kind:   e.kind.clone(),
                symbol: e.target_symbol.clone(),
            })
            .collect();

        let co_raw = store.co_changes_for_file(file, repo_path, 2)?;
        let cochanges: Vec<CochangeEntry> = co_raw.into_iter()
            .map(|c| {
                let in_pr = pr_file_set.contains(&c.file_path);
                CochangeEntry { file: c.file_path, count: c.change_count, in_pr }
            })
            .collect();

        pr_files.push(PrFileContext {
            file:                file.clone(),
            role:                classify_artifact_role(file),
            touch_count,
            last_commit_message,
            structural_out,
            structural_in,
            cochanges,
        });
    }

    // ── Phase 4: Documentary search using PR title terms ──────────────────────
    //
    // Extract meaningful tokens from the PR title (skip stopwords, short words).
    // Search each token; collect matching PRs and issues, excluding the PR being reviewed.
    let stopwords: std::collections::HashSet<&str> = [
        "a", "an", "the", "and", "or", "for", "to", "of", "in", "on",
        "at", "is", "are", "was", "be", "it", "its", "by", "as", "with",
        "from", "into", "add", "use", "fix", "feat", "docs", "chore",
    ].iter().cloned().collect();

    let title_tokens: Vec<String> = detail.title
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| {
            let lower = t.to_lowercase();
            t.len() >= 4 && !stopwords.contains(&lower[..])
        })
        .map(|t| t.to_lowercase())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let mut doc_map: std::collections::HashMap<(String, i64), DocumentaryEvidence> =
        std::collections::HashMap::new();

    for token in &title_tokens {
        let matches = store.search_anchor(token, repo_path)?;
        for m in matches {
            let (kind, number) = match m.source_type.as_str() {
                "pr_title" | "pr_body" => {
                    let n: i64 = m.source_id.parse().unwrap_or(0);
                    if n == pr_number as i64 { continue; } // skip the PR being reviewed
                    ("pr".to_string(), n)
                }
                "issue_title" | "issue_body" => {
                    let n: i64 = m.source_id.parse().unwrap_or(0);
                    ("issue".to_string(), n)
                }
                _ => continue,
            };

            let entry = doc_map
                .entry((kind.clone(), number))
                .or_insert_with(|| DocumentaryEvidence {
                    kind:            kind.clone(),
                    number,
                    title:           String::new(),
                    matched_anchors: Vec::new(),
                    snippets:        Vec::new(),
                });

            if !entry.matched_anchors.contains(token) {
                entry.matched_anchors.push(token.clone());
            }
            if (m.source_type == "pr_title" || m.source_type == "issue_title") && entry.title.is_empty() {
                entry.title = m.text.clone();
            }
            if m.source_type == "pr_body" || m.source_type == "issue_body" {
                let snippet: String = m.text.chars().take(200).collect();
                entry.snippets.push(snippet);
            }
        }
    }

    let mut documentary: Vec<DocumentaryEvidence> = doc_map.into_values().collect();
    // Sort: most matched anchors first, then by kind+number for determinism.
    documentary.sort_by(|a, b| {
        b.matched_anchors.len().cmp(&a.matched_anchors.len())
            .then(a.kind.cmp(&b.kind))
            .then(a.number.cmp(&b.number))
    });

    // ── Assemble ──────────────────────────────────────────────────────────────
    Ok(ReviewContextDocument {
        schema_version:       1,
        pr_number,
        pr_title:             detail.title,
        pr_body:              detail.body,
        linked_issue_numbers: detail.linked_issue_numbers,
        pr_files,
        documentary,
        coverage: ReviewCoverage {
            git_history:      any_history,
            github_prs:       pr_count > 0,
            github_issues:    issue_count > 0,
            structural_edges: edge_count > 0,
        },
    })
}
