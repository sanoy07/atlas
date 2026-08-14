use anyhow::{Context, Result};
use atlas_connectors::Connector;
use atlas_git::{GitHubIssueConnector, GitHubPrConnector, GitRepo};
use atlas_ir::{
    AnchorMatch, ArtifactRole, AuthorAggregate, AuthorScope, AuthorsReport, CampaignBrief,
    CampaignOutcome, CandidateArtifact, CandidateReason, CochangeEntry, CommitSubject,
    CommitSummary, ConceptExpansion, ConfigArtifactSubject, ContextDocument, CouplingEntry,
    CoverageMap, CoverageStatus, DocumentaryEvidence, DocumentSubject, EvidenceSummary,
    EvidenceType, ExternalDependencyRow, FileIdentity, FileSignificance, FileSubject, GapEntry,
    HistoricalEntry, HistoricalRedirect, IdentitySubject, IngestRunSubject, InspectionChild,
    InspectionCoverage, InspectionDocument, InspectionDocumentRef, InspectionEdge,
    InspectionSubjectKind, InvestigationCoverage, InvestigationDocument, IssueSubject,
    IssueSummary, LexiconExpansion, LexiconRelKind, LowComplexityNote, MatchSource,
    ModuleCouplingCell, ModuleCouplingKindBreakdown, ModuleCouplingReport, ModuleFanIndicator,
    PeerStructureDeviation, PeerStructurePattern, PeerStructureReport, PlatformUsageRow,
    PrFileContext, PrSubject, PrSummary, ProfileClaim, ProfileClaimKind, RelatedDecision,
    RelatedHistory, RepositoryTree, ReviewContextDocument, ReviewCoverage, ScoreBreakdown,
    SearchCoverage, SearchDocument, ShowLink, ShowProvenance, ShowRecord, ShowRow, ShowSection,
    ShowSectionKind, ShowSubject, StructuralEdgeSummary, StructuralObservation, TreeNode,
    TreeNodeKind, UnresolvedConnection, VerifiedExpansion,
};
use atlas_parser::{c_structural, gh_json, git_log, git_renames, python_structural, rust_structural, ts_structural};
use atlas_storage::{AuthorAggregateRow, CommitRow, HotFileRow, Store};
use std::collections::HashSet;
use std::path::Path;
use tracing::info;

mod lexicon;
pub use lexicon::build_lexicon;

mod repo_inspector;
pub use repo_inspector::inspect_repository;

mod project;
pub use project::{
    build_project_census, create_project, get_project, ingest_project, list_projects,
    list_repositories, register_repository_at_path, IngestOptions, RepositoryIngestSummary,
};

/// B5–B10 aggregation reports (modules, tests, deps, cohorts, anomalies, config).
mod b_layer;
pub use b_layer::{
    compute_anomalies, compute_config_inventory, compute_config_provenance,
    compute_dependency_linkage, compute_directory_cohorts, compute_modules,
    compute_test_module_links, external_package_name, path_looks_like_test,
};

/// Local AI provider abstraction (Ollama + fake).
pub mod ollama_config;
pub use ollama_config::{probe_ollama, OllamaConfig, OllamaProbe};

pub mod ai_provider;
pub use ai_provider::{
    parse_reasoning_response, FakeReasoningProvider, OllamaProvider, ReasoningProvider,
};

/// Evidence packet + multi-round reasoning investigation loop.
mod reasoning;
pub use reasoning::{
    anchors_from_question, build_evidence_packet, evidence_resolves_pub, options_from_file,
    options_from_issue, options_from_question, run_reasoning_investigation, verify_claims,
    verify_hypotheses, PacketOptions, ReasoningOptions,
};

/// C4-ER — evidence ranking, temporal supersession, hard claim entailment.
mod evidence_reasoning;
pub use evidence_reasoning::{
    enrich_packet, enrich_packet_with_store, hard_verify_claim, hard_verify_claims,
    hard_verify_hypotheses, parse_github_numbers, rank_evidence, statement_is_causal,
    verification_policy,
};

/// C5.1 — question-personalized structural PageRank (Aider-inspired, Atlas edges).
mod personalized_rank;
pub use personalized_rank::{
    collect_links_for_files, personalized_file_ranks, FileRank, PersonalizedRankInput,
    StructuralLink,
};

/// Deterministic code-intelligence: callers, implementations, capabilities, ranked search.
mod code_intel;
pub use code_intel::{
    compute_capabilities, definition_ranked_search, find_callees, find_callers,
    find_implementations, CallSite, CallersReport, CapabilitiesReport, CapabilitySurface,
    CodeSearchHit, CodeSearchReport, ImplementationHit, ImplementationsReport,
};

/// C5.1-R — deterministic retrieval recall expansion (issue / domain / flow seeds).
mod retrieval_expand;
pub use retrieval_expand::{detect_issue_numbers, expand_retrieval, RetrievalExpansion};

/// C5.1-L — identifier-weighted lexical relevance + structure-aware dedup.
mod lexical_relevance;
pub use lexical_relevance::{rerank_candidates, score_path_for_question, structure_aware_dedup};

/// C5.1-E — role-aware / entrypoint primacy from structure + bag IDF.
mod role_aware;
pub use role_aware::{apply_role_aware_rerank, concept_search_fragments, infer_role, InferredRole};

/// Path/file class soft ranking (production vs demo/asset/CI/notebook).
mod path_class;
pub use path_class::{classify_path, PathClass};

/// C5.1-S — free-text → structural subject resolution.
mod subject_resolve;
pub use subject_resolve::{discover_code_roots, resolve_subjects, SubjectResolution};

/// Section C — Map / Focus / Impact (orientation claims over existing evidence).
mod section_c;
pub use section_c::{build_focus, build_impact, build_map, resolve_modules_subject};

/// Is the evidence graph still describing the current tree?
mod freshness;
pub use freshness::{compute_freshness, Freshness, FreshnessReport};

/// Repository Awareness: understands which paths represent generated artifacts
/// vs. source code worth investigating. Applied during ingest to prevent build
/// output from occupying candidate slots or polluting historical evidence.
///
/// Earned primitive (N=3): RWATP, Atlas self-ingest, and VestaScan all produced
/// build artifact noise during investigation. VestaScan specifically commits
/// `dist/` to version control, so `.gitignore` alone is insufficient —
/// hardcoded common patterns are the primary exclusion mechanism.
///
/// Intentionally supports a small subset of `.gitignore` semantics:
/// only the root `.gitignore`; only simple entries (no `*`, `?`, `[`,
/// `!`, or anchored `/foo`); all patterns are matched **root-anchored**
/// against a repo-relative path.  See
/// docs/decisions/2026-08-08-repo-awareness-bare-name-fix.md.
struct RepoAwareness {
    /// Patterns from `.gitignore` that ended in `/` and the hardcoded defaults.
    /// Match: `path.starts_with(prefix)` (prefix always ends in `/`).
    dir_prefixes: Vec<String>,
    /// Bare-name patterns from `.gitignore` (no trailing `/`).
    /// Match: `path == name` OR `path` is `name` followed by `/…`.
    /// Kept root-anchored to preserve the existing `starts_with` semantics
    /// used by every other prefix in this struct.
    names:        Vec<String>,
}

impl RepoAwareness {
    fn load(repo_path: &str) -> Self {
        let hardcoded = [
            "dist/", "node_modules/", "target/", "build/", ".next/",
            "coverage/", "__pycache__/", ".cache/", "out/", ".nuxt/",
        ];
        let mut dir_prefixes: Vec<String> = hardcoded.iter().map(|&s| s.to_string()).collect();
        let mut names: Vec<String> = Vec::new();

        let gitignore = Path::new(repo_path).join(".gitignore");
        if let Ok(content) = std::fs::read_to_string(gitignore) {
            for raw in content.lines() {
                let line = raw.trim();
                if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                    continue;
                }
                // Only handle simple name patterns — skip globs (*/?/[).
                if line.contains('*') || line.contains('?') || line.contains('[') {
                    continue;
                }
                // Skip anchored patterns (`/foo`) — supporting them requires
                // deciding whether the leading `/` should also make nested
                // matches invalid, and that is out of scope for this fix.
                if line.starts_with('/') {
                    continue;
                }
                if line.ends_with('/') {
                    if !dir_prefixes.contains(&line.to_string()) {
                        dir_prefixes.push(line.to_string());
                    }
                } else if !names.contains(&line.to_string()) {
                    names.push(line.to_string());
                }
            }
        }

        RepoAwareness { dir_prefixes, names }
    }

    fn is_excluded(&self, path: &str) -> bool {
        let p = path.trim_start_matches('/');
        if self.dir_prefixes.iter().any(|prefix| p.starts_with(prefix.as_str())) {
            return true;
        }
        // Bare names match the root file (`path == name`) or anything beneath a
        // root directory of that name (`name/…`).  Both cases are root-anchored.
        self.names.iter().any(|name| {
            p == name.as_str()
                || (p.len() > name.len()
                    && p.starts_with(name.as_str())
                    && p.as_bytes()[name.len()] == b'/')
        })
    }
}

#[cfg(test)]
mod repo_awareness_tests {
    use super::*;
    use tempfile::TempDir;

    /// Load `RepoAwareness` from a tempdir populated with an optional `.gitignore`.
    fn load_with_gitignore(contents: Option<&str>) -> (TempDir, RepoAwareness) {
        let dir = TempDir::new().unwrap();
        if let Some(text) = contents {
            std::fs::write(dir.path().join(".gitignore"), text).unwrap();
        }
        let awareness = RepoAwareness::load(dir.path().to_str().unwrap());
        (dir, awareness)
    }

    #[test]
    fn hardcoded_defaults_still_apply_without_gitignore() {
        let (_dir, aw) = load_with_gitignore(None);
        for prefix in ["dist/", "node_modules/", "target/", "build/", ".next/"] {
            let sample = format!("{}sample.txt", prefix);
            assert!(aw.is_excluded(&sample),
                "hardcoded default {} must exclude {}", prefix, sample);
        }
    }

    #[test]
    fn bare_gitignore_name_excludes_root_file() {
        let (_dir, aw) = load_with_gitignore(Some("atlas.db\n"));
        assert!(aw.is_excluded("atlas.db"),
            "bare `atlas.db` in .gitignore must exclude root file `atlas.db`");
    }

    #[test]
    fn bare_gitignore_name_still_excludes_directory_and_contents() {
        let (_dir, aw) = load_with_gitignore(Some("cache\n"));
        assert!(aw.is_excluded("cache/anything.log"),
            "bare name must still exclude paths beneath a directory of that name");
        assert!(aw.is_excluded("cache"),
            "bare name must also exclude the exact root path (file or dir called `cache`)");
    }

    #[test]
    fn bare_gitignore_name_does_not_match_partial_prefix() {
        let (_dir, aw) = load_with_gitignore(Some("foo\n"));
        assert!(!aw.is_excluded("foobar"),
            "bare name `foo` must NOT match `foobar` (word-boundary at path separator)");
        assert!(!aw.is_excluded("foo.txt"),
            "bare name `foo` must NOT match `foo.txt` (word-boundary at path separator)");
    }

    #[test]
    fn bare_gitignore_name_is_root_anchored() {
        let (_dir, aw) = load_with_gitignore(Some("atlas.db\n"));
        assert!(!aw.is_excluded("packages/foo/atlas.db"),
            "bare name is root-anchored per existing RepoAwareness semantics — \
             expanding to any depth would silently change gitignore behaviour");
        assert!(!aw.is_excluded("nested/atlas.db"),
            "same — no unrooted matching");
    }

    #[test]
    fn trailing_slash_gitignore_pattern_unchanged() {
        let (_dir, aw) = load_with_gitignore(Some("target/\n"));
        assert!(aw.is_excluded("target/debug/foo"),
            "explicit trailing-slash pattern must exclude contents (unchanged behaviour)");
        assert!(!aw.is_excluded("target"),
            "explicit trailing-slash pattern must NOT match a bare string with no slash \
             (unchanged behaviour — callers pass `target/` when probing a directory)");
    }

    #[test]
    fn glob_and_negation_lines_are_still_ignored() {
        let (_dir, aw) = load_with_gitignore(Some("*.log\n!keep.log\nfoo/**/bar\n"));
        assert!(!aw.is_excluded("app.log"),
            "glob patterns are silently unsupported (documented limitation)");
        assert!(!aw.is_excluded("keep.log"), "negation is silently unsupported");
    }

    #[test]
    fn anchored_pattern_line_is_skipped() {
        // Leading-slash patterns are skipped rather than silently reinterpreted.
        let (_dir, aw) = load_with_gitignore(Some("/only-root\n"));
        assert!(!aw.is_excluded("only-root"),
            "anchored `/name` patterns are intentionally skipped — supporting them \
             requires a separate decision about nested-match semantics");
    }

    #[test]
    fn empty_and_comment_lines_are_skipped() {
        let (_dir, aw) = load_with_gitignore(Some("\n# comment\n   \nnode_modules\n"));
        assert!(aw.is_excluded("node_modules/pkg/index.js"),
            "bare `node_modules` from .gitignore must exclude (already covered by hardcoded, \
             but confirms comment/empty-line filtering did not eat the entry)");
    }
}

/// Hard cap on commits ingested per stage.  If the repository has more
/// commits than this, the excess is silently dropped by git itself — Atlas
/// records the applied cap and the actual repo commit count in
/// `ingest_runs.stages_json` so downstream queries can detect truncation.
pub const INGEST_COMMIT_CAP: usize = 10_000;

/// Backward-compatible entry point.  Defaults to HEAD-only scope (the
/// historical semantics before P1-1).
pub fn ingest_git(repo_path: &str, store: &Store) -> Result<usize> {
    ingest_git_scoped(repo_path, store, atlas_git::GitScope::HeadOnly)
}

/// Ingest git history at an explicit scope.  Every commit inserted also
/// records its parent hashes into `commit_parents` (P1-2).
pub fn ingest_git_scoped(
    repo_path: &str,
    store:     &Store,
    scope:     atlas_git::GitScope,
) -> Result<usize> {
    let awareness = RepoAwareness::load(repo_path);
    let connector = GitRepo::open(repo_path)?;
    let payload   = connector.log_raw_scoped(scope, INGEST_COMMIT_CAP)?;
    let commits   = git_log::parse(&payload)?;
    let count     = commits.len();

    info!(
        "connector={} capability={} scope={} parsed={} commits",
        connector.name(),
        connector.capability().name,
        scope.as_str(),
        count,
    );

    let mut excluded = 0usize;
    for commit in &commits {
        let mut filtered = commit.clone();
        let before = filtered.files_changed.len();
        filtered.files_changed.retain(|p| !awareness.is_excluded(p));
        excluded += before - filtered.files_changed.len();
        store.insert_commit(&filtered, repo_path)?;
    }
    if excluded > 0 {
        info!("repository awareness excluded {} file-change records", excluded);
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

/// Stopwords for issue anchor extraction — English function words only.
/// Does NOT exclude infrastructure or domain terms (memory, cache, logging, etc.)
/// because those terms often appear in file names and are exactly the vocabulary
/// needed to find the relevant files from an issue description.
const ISSUE_ANCHOR_STOPWORDS: &[&str] = &[
    // English function words
    "a", "an", "the", "and", "or", "but", "for", "to", "in", "of", "at",
    "by", "with", "this", "that", "from", "into", "as", "is", "it", "its",
    "be", "been", "has", "have", "had", "not", "no", "on", "are", "was",
    "were", "will", "can", "all", "we", "our", "their", "which", "when",
    "where", "how", "who", "what", "any", "each", "via", "per", "after",
    "before", "about", "over", "up", "out", "now", "just", "some", "should",
    "would", "could", "need", "may", "must", "make", "made", "both", "only",
    "same", "also", "more", "other", "than", "then", "so", "if", "do",
    "does", "did", "get", "set", "new", "add", "use", "true", "false",
    "null", "undefined",
    // Uninformative prose words that appear in issue descriptions but not file paths
    "issue", "issues", "feature", "features", "summary", "overview",
    "steps", "step", "notes", "note", "changes", "fixes", "fixed",
    "details", "branch", "full", "first", "last", "next", "total",
    "item", "items", "minor", "major", "small", "large", "simple",
];

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
            let window: String = text.chars().skip(start).take(end - start).collect();
            trim_partial_edge_words(&window, start > 0, end < total)
        }
    }
}

/// Drop the leading/trailing token of a window when that edge was produced by
/// slicing mid-word. Without this, cutting "…support…" yields the fragment
/// "pport", which then verifies as a real expansion term because it is a
/// substring of a genuine file path.
fn trim_partial_edge_words(window: &str, cut_at_start: bool, cut_at_end: bool) -> String {
    let mut s = window;
    if cut_at_start {
        if let Some(i) = s.find(char::is_whitespace) {
            s = &s[i + 1..];
        } else {
            return String::new();
        }
    }
    if cut_at_end {
        if let Some(i) = s.rfind(char::is_whitespace) {
            s = &s[..i];
        } else {
            return String::new();
        }
    }
    s.trim().to_string()
}

/// Phase 0a: Lexicon expansion.
///
/// Only expands anchors that have few direct file-path matches (same threshold as
/// Phase 0 concept resolution).  An anchor with ≥ MIN_FILE_PATH_MATCHES direct
/// matches already points to the right vocabulary — expanding it adds noise.
///
/// Returns the list of expansions surfaced to the user and the augmented anchor list.
fn expand_via_lexicon(
    anchors: &[&str],
    repo_path: &str,
    store: &Store,
) -> Result<(Vec<LexiconExpansion>, Vec<String>)> {
    const MIN_CONFIDENCE: f32 = 0.65;
    const MIN_FILE_PATH_MATCHES: usize = 3;

    let mut seen: std::collections::HashSet<String> =
        anchors.iter().map(|s| s.to_lowercase()).collect();
    let mut expansions: Vec<LexiconExpansion> = Vec::new();
    let mut augmented: Vec<String> = anchors.iter().map(|s| s.to_lowercase()).collect();

    for &anchor in anchors {
        // Skip if the anchor already has sufficient direct file-path coverage.
        let all_rows = store.search_anchor(anchor, repo_path)?;
        let file_path_hits = all_rows.iter().filter(|r| r.source_type == "file_path").count();
        if file_path_hits >= MIN_FILE_PATH_MATCHES { continue; }

        let rels = store.lexicon_expand(anchor, repo_path)?;
        for row in rels {
            if row.confidence < MIN_CONFIDENCE as f64 { continue; }
            let resolved = row.to_term.to_lowercase();
            if seen.contains(&resolved) { continue; }

            let kind = LexiconRelKind::from_str(&row.kind)
                .unwrap_or(LexiconRelKind::CommitBridge);

            // Ground the expansion in a file path that contains the resolved term.
            let grounded_in = {
                let hits = store.search_anchor(&resolved, repo_path)?;
                hits.into_iter()
                    .find(|r| r.source_type == "file_path")
                    .map(|r| r.source_id)
                    .unwrap_or_else(|| format!("lexicon ({}x co-occur)", row.co_occurrence_count))
            };

            expansions.push(LexiconExpansion {
                original_term: anchor.to_lowercase(),
                kind,
                resolved_term: resolved.clone(),
                confidence:    row.confidence as f32,
                grounded_in,
            });
            seen.insert(resolved.clone());
            augmented.push(resolved);
        }
    }

    Ok((expansions, augmented))
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
    /// An anchor written across many PR/issue bodies is shared project prose
    /// ("computed", "behaviour"), not a distinguishing domain concept. Bridging
    /// it pulls whichever unrelated PR happens to be matched first into
    /// retrieval. Terms that are genuinely specific are rare in the corpus.
    const MAX_DOC_BREADTH: usize = 8;

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

        // Generic-vocabulary guard: bridge only anchors that are rare in the
        // documentary corpus. Breadth is counted over distinct PR/issue
        // sources, not rows, so repeated use inside one body still counts once.
        let breadth: std::collections::HashSet<&str> =
            doc_rows.iter().map(|r| r.source_id.as_str()).collect();
        if breadth.len() > MAX_DOC_BREADTH { continue; }

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

        // Prefer shorter (more specific) terms; cap expansions per anchor.
        // Each expansion is an extra retrieval anchor, so a single bridged term
        // injecting 8 of them dominates the bag over the original question.
        verified.sort_by_key(|v| v.term.len());
        verified.truncate(4);

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

    // ── Phase 0z: Per-anchor identity redirect (Item 1) ──────────────────────
    //
    // If any anchor is a file-path address that FileIdentity recognises as
    // historical, add the current canonical path as an additional anchor.
    // The original user anchor is preserved in `anchors`; the redirect is
    // recorded in `anchor_redirects` so consumers see both addresses.
    //
    // A file-path anchor is detected heuristically (contains a `/` and a `.`,
    // OR resolves cleanly to a file via FileIdentity).  Non-file anchors
    // ("order", "identity") are unchanged — they were never paths.
    let mut anchor_redirects: Vec<atlas_ir::AnchorRedirect> = Vec::new();
    let mut identity_augmented: Vec<String> = anchors.iter().map(|s| s.to_string()).collect();
    for a in anchors {
        let looks_like_path = a.contains('/') || a.contains('.');
        if !looks_like_path { continue; }
        if let Some(current) = store.current_path_if_historical(a, repo_path)? {
            let identity_id = store
                .resolve_path_to_identity(a, repo_path)?
                .unwrap_or(0);
            if !identity_augmented.iter().any(|s| s == &current) {
                identity_augmented.push(current.clone());
            }
            anchor_redirects.push(atlas_ir::AnchorRedirect {
                original_anchor: a.to_string(),
                current_path:    current,
                identity_id,
            });
        }
    }
    let identity_augmented_refs: Vec<&str> =
        identity_augmented.iter().map(String::as_str).collect();

    // ── Phase 0a: Lexicon expansion ───────────────────────────────────────────
    //
    // For each anchor, query the repository lexicon for vocabulary relationships
    // built during ingest (abbreviations, commit bridges, compound components).
    // Adds high-confidence resolved terms to the anchor set before concept
    // resolution, so concept resolution can confirm and extend what the lexicon
    // already knows.
    let (lexicon_expansions, lexicon_augmented_anchors) =
        expand_via_lexicon(&identity_augmented_refs, repo_path, store)?;
    let lexicon_anchor_refs: Vec<&str> =
        lexicon_augmented_anchors.iter().map(String::as_str).collect();

    // ── Phase 0: Concept Resolution ───────────────────────────────────────────
    //
    // For anchors that don't map directly to file paths, search documentary
    // evidence (PR/issue bodies) for vocabulary bridges and verify candidate
    // expansion terms against the repository before adding them.  This expands
    // the anchor set only using vocabulary the team actually used.
    let (concept_expansions, effective_anchor_strs) =
        resolve_concepts(&lexicon_anchor_refs, repo_path, store)?;
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

    // decision records matched: path → first snippet (preserve insertion order)
    let mut decision_snippets: indexmap::IndexMap<String, String> = indexmap::IndexMap::new();

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
            MatchSource::DecisionBody => {
                // Decision records explain *why* — not candidates, but rationale.
                // Collect first snippet per document; render as ENGINEERING DECISIONS.
                decision_snippets
                    .entry(m.source_id.clone())
                    .or_insert_with(|| m.snippet.clone());
            }
        }
    }

    // -- Phase 1 pre-filter -------------------------------------------------------
    //
    // Cap the seed pool before structural expansion so Phase 2 queries remain
    // bounded even on very large repositories (>1000 file-path matches).
    // Use a lightweight lexical pre-sort: filename hit first, then anchor count.
    // Full multi-signal ranking happens after Phase 4 when historical data is
    // available, so this pre-filter only prevents combinatorial blowup.
    const PREFILTER: usize = 60;
    {
        let mut pre: Vec<(String, Vec<CandidateReason>, bool, usize)> = candidates
            .into_iter()
            .map(|(file, reasons)| {
                let matched_anchors: std::collections::HashSet<&str> = reasons.iter()
                    .filter_map(|r| match r {
                        CandidateReason::AnchorMatch { anchor, .. } => Some(anchor.as_str()),
                        _ => None,
                    })
                    .collect();
                let anchor_count = matched_anchors.len();
                let basename = file.split('/').last().unwrap_or(file.as_str());
                let stem = if let Some(dot) = basename.find('.') { &basename[..dot] } else { basename };
                let stem_lower = stem.to_lowercase();
                let filename_hit = matched_anchors.iter()
                    .any(|a| stem_lower.contains(&a.to_lowercase()));
                (file, reasons, filename_hit, anchor_count)
            })
            .collect();
        pre.sort_by(|a, b| b.2.cmp(&a.2).then(b.3.cmp(&a.3)).then(a.0.cmp(&b.0)));
        pre.truncate(PREFILTER);
        candidates = pre.into_iter()
            .map(|(file, reasons, _, _)| (file, reasons))
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

    // ── Multi-signal ranking ──────────────────────────────────────────────────
    //
    // Signals and weights:
    //   35% Lexical    — anchor match quality (count + filename vs directory)
    //   25% Structural — edges within candidate set (normalised)
    //   20% Historical — log-normalised touch count
    //   15% Centrality — in-degree within candidate set (normalised)
    //
    // All signals are normalised to [0, 1] before weighting.
    // After scoring, candidates are sorted by total score (desc) and
    // truncated to MAX_OUTPUT.
    const MAX_OUTPUT: usize = 20;
    {
        let total_anchors = effective_anchor_refs.len().max(1);

        // Build touch count lookup from the historical vec
        let touch_map: std::collections::HashMap<&str, i64> = historical.iter()
            .map(|h| (h.file.as_str(), h.touch_count))
            .collect();

        // Build edge-count map and PageRank scores for the candidate subgraph.
        // PageRank replaces raw in-degree as the centrality signal — it accounts for
        // the "quality" of in-links, not just their count.
        let edge_count_map: std::collections::HashMap<&str, usize> = observed_structure.iter()
            .map(|o| (o.file.as_str(), o.outgoing.len() + o.incoming.len()))
            .collect();

        // Build node index for PageRank.
        // Use owned Strings so `candidates` is free to be moved later.
        let nodes: Vec<String> = candidates.keys().cloned().collect();
        let node_idx: std::collections::HashMap<&str, usize> = nodes.iter()
            .enumerate().map(|(i, f)| (f.as_str(), i)).collect();

        // Build directed edge list from observed outgoing edges.
        let mut pr_edges: Vec<(usize, usize)> = Vec::new();
        for obs in &observed_structure {
            if let Some(&from) = node_idx.get(obs.file.as_str()) {
                for e in &obs.outgoing {
                    if let Some(&to) = node_idx.get(e.file.as_str()) {
                        pr_edges.push((from, to));
                    }
                }
            }
        }

        let node_strs: Vec<&str> = nodes.iter().map(String::as_str).collect();
        let pr_scores = pagerank(&node_strs, &pr_edges, 20, 0.85);
        let max_pr = pr_scores.iter().cloned().fold(0.0_f32, f32::max);

        let pagerank_map: std::collections::HashMap<&str, f32> = nodes.iter()
            .enumerate()
            .map(|(i, f)| (f.as_str(), pr_scores[i]))
            .collect();

        let max_touch  = touch_map.values().copied().max().unwrap_or(0);
        let max_edges  = edge_count_map.values().copied().max().unwrap_or(0);

        let score_candidate = |file: &str, reasons: &[CandidateReason]| -> ScoreBreakdown {
            // lexical
            let matched: std::collections::HashSet<&str> = reasons.iter()
                .filter_map(|r| match r {
                    CandidateReason::AnchorMatch { anchor, .. } => Some(anchor.as_str()),
                    _ => None,
                })
                .collect();
            let anchor_frac = matched.len() as f32 / total_anchors as f32;
            let basename = file.split('/').last().unwrap_or(file);
            let stem = if let Some(d) = basename.find('.') { &basename[..d] } else { basename };
            let stem_lower = stem.to_lowercase();
            let filename_hit = matched.iter().any(|a| stem_lower.contains(&a.to_lowercase()));
            let lexical = (anchor_frac * if filename_hit { 1.3 } else { 1.0 }).min(1.0);

            // structural (total edge degree within candidate set)
            let edges = edge_count_map.get(file).copied().unwrap_or(0);
            let structural = if max_edges > 0 { edges as f32 / max_edges as f32 } else { 0.0 };

            // historical (log-normalised touch count)
            let tc = touch_map.get(file).copied().unwrap_or(0);
            let historical_s = if max_touch > 0 {
                (tc as f32 + 1.0).ln() / (max_touch as f32 + 1.0).ln()
            } else { 0.0 };

            // centrality (PageRank within candidate subgraph)
            let pr = pagerank_map.get(file).copied().unwrap_or(0.0);
            let centrality = if max_pr > 0.0 { pr / max_pr } else { 0.0 };

            let total = 0.35 * lexical + 0.25 * structural + 0.20 * historical_s + 0.15 * centrality;

            ScoreBreakdown { lexical, structural, historical: historical_s, centrality, total }
        };

        let mut scored: Vec<(String, Vec<CandidateReason>, ScoreBreakdown)> = candidates
            .into_iter()
            .map(|(file, reasons)| {
                let s = score_candidate(&file, &reasons);
                (file, reasons, s)
            })
            .collect();

        scored.sort_by(|a, b| b.2.total.partial_cmp(&a.2.total).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(MAX_OUTPUT);

        candidates = scored.into_iter()
            .map(|(file, reasons, _)| (file, reasons))
            .collect();

        // Re-scope observed_structure and historical to the post-ranking candidate set
        let kept: std::collections::HashSet<&str> = candidates.keys().map(String::as_str).collect();
        observed_structure.retain(|o| kept.contains(o.file.as_str()));
        historical.retain(|h| kept.contains(h.file.as_str()));
        historical.sort_by(|a, b| b.touch_count.cmp(&a.touch_count).then(a.file.cmp(&b.file)));

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
    // Filter candidates to files that exist on disk right now.
    // Git history records every file that ever existed; deleted files remain in
    // the DB after the commit that removed them.  Presenting deleted files as
    // candidates is a correctness error — the investigator would act on
    // evidence that no longer reflects reality.
    let repo_root = repo_path.trim_end_matches('/');

    let mut core_candidates: Vec<CandidateArtifact> = Vec::new();
    let mut supporting_artifacts: Vec<CandidateArtifact> = Vec::new();
    let mut deleted_candidates: Vec<String> = Vec::new();

    // Build final scores over the post-ranking, disk-extant candidate set.
    // Wrapped in a block so the closure's borrows are released before the sort calls below.
    {
        let final_touch_max = historical.iter().map(|h| h.touch_count).max().unwrap_or(0);
        let final_edge_max  = observed_structure.iter()
            .map(|o| o.outgoing.len() + o.incoming.len()).max().unwrap_or(0);
        let final_indeg_max = observed_structure.iter()
            .map(|o| o.incoming.len()).max().unwrap_or(0);
        let total_anchors_final = effective_anchor_refs.len().max(1);

        let compute_score = |file: &str, reasons: &[CandidateReason]| -> ScoreBreakdown {
            let matched: std::collections::HashSet<&str> = reasons.iter()
                .filter_map(|r| match r {
                    CandidateReason::AnchorMatch { anchor, .. } => Some(anchor.as_str()),
                    _ => None,
                })
                .collect();
            let anchor_frac = matched.len() as f32 / total_anchors_final as f32;
            let basename = file.split('/').last().unwrap_or(file);
            let stem = if let Some(d) = basename.find('.') { &basename[..d] } else { basename };
            let stem_lower = stem.to_lowercase();
            let filename_hit = matched.iter().any(|a| stem_lower.contains(&a.to_lowercase()));
            let lexical = (anchor_frac * if filename_hit { 1.3 } else { 1.0 }).min(1.0);

            let obs = observed_structure.iter().find(|o| o.file == file);
            let edges  = obs.map(|o| o.outgoing.len() + o.incoming.len()).unwrap_or(0);
            let in_deg = obs.map(|o| o.incoming.len()).unwrap_or(0);
            let structural = if final_edge_max > 0 { edges as f32 / final_edge_max as f32 } else { 0.0 };
            let centrality = if final_indeg_max > 0 { in_deg as f32 / final_indeg_max as f32 } else { 0.0 };

            let tc = historical.iter().find(|h| h.file == file).map(|h| h.touch_count).unwrap_or(0);
            let historical_s = if final_touch_max > 0 {
                (tc as f32 + 1.0).ln() / (final_touch_max as f32 + 1.0).ln()
            } else { 0.0 };

            let total = 0.35 * lexical + 0.25 * structural + 0.20 * historical_s + 0.15 * centrality;
            ScoreBreakdown { lexical, structural, historical: historical_s, centrality, total }
        };

        for (file, reasons) in candidates {
            let abs_path = format!("{}/{}", repo_root, file);
            if !std::path::Path::new(&abs_path).exists() {
                deleted_candidates.push(file);
                continue;
            }
            let score = compute_score(&file, &reasons);
            let role = classify_artifact_role(&file);
            let artifact = CandidateArtifact { file, role: role.clone(), reasons, score };
            if role == ArtifactRole::ProductionSource {
                core_candidates.push(artifact);
            } else {
                supporting_artifacts.push(artifact);
            }
        }
    } // compute_score and its borrows dropped here

    // Sort by score descending so highest-confidence files appear first.
    core_candidates.sort_by(|a, b|
        b.score.total.partial_cmp(&a.score.total).unwrap_or(std::cmp::Ordering::Equal)
        .then(a.file.cmp(&b.file)));
    supporting_artifacts.sort_by(|a, b|
        b.score.total.partial_cmp(&a.score.total).unwrap_or(std::cmp::Ordering::Equal)
        .then(a.file.cmp(&b.file)));
    deleted_candidates.sort();

    let mut documentary: Vec<DocumentaryEvidence> = doc_map.into_values().collect();
    documentary.sort_by_key(|d| (d.kind.clone(), d.number));

    observed_structure.sort_by(|a, b| a.file.cmp(&b.file));
    historical.sort_by(|a, b| b.touch_count.cmp(&a.touch_count).then(a.file.cmp(&b.file)));
    unresolved.sort_by(|a, b| a.subject.cmp(&b.subject));

    let mut effective_anchors: Vec<String> = effective_anchor_strs;
    effective_anchors.sort();

    let mut related_decisions: Vec<RelatedDecision> = Vec::new();
    for (path, snippet) in decision_snippets {
        let title = store
            .document_by_path(&path, repo_path)?
            .unwrap_or_else(|| path.clone());
        related_decisions.push(RelatedDecision { title, path, snippet });
    }

    Ok(InvestigationDocument {
        schema_version:    6,
        anchors:           anchors.iter().map(|s| s.to_string()).collect(),
        effective_anchors,
        lexicon_expansions,
        concept_expansions,
        core_candidates,
        supporting_artifacts,
        observed_structure,
        documentary,
        historical,
        unresolved,
        related_decisions,
        coverage,
        deleted_candidates,
        anchor_redirects,
    })
}

/// Run an investigation and store the result in the DB, returning the document.
/// If the same (anchors, git HEAD) combination is already cached, returns the
/// cached result without re-running all 5 phases.
pub fn investigate_cached(
    anchors:   &[&str],
    repo_path: &str,
    store:     &Store,
) -> Result<InvestigationDocument> {
    let anchors_key = {
        let mut sorted = anchors.to_vec();
        sorted.sort_unstable();
        sorted.join(",")
    };
    let git_head = current_git_head(repo_path).unwrap_or_else(|_| "unknown".to_string());

    // Cache hit?
    if let Some(json) = store.get_investigation(repo_path, &anchors_key, &git_head)? {
        if let Ok(doc) = serde_json::from_str::<InvestigationDocument>(&json) {
            return Ok(doc);
        }
    }

    let doc = investigate(anchors, repo_path, store)?;
    let json = serde_json::to_string(&doc)
        .context("failed to serialize investigation document")?;
    let ran_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    store.store_investigation(repo_path, &anchors_key, &git_head, ran_at, &json)?;
    Ok(doc)
}

/// Return the current HEAD commit hash for the repo, or an error.
fn current_git_head(repo_path: &str) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["-C", repo_path, "rev-parse", "--short", "HEAD"])
        .output()
        .context("git rev-parse")?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// List stored investigation records for a repo (newest first).
pub fn list_stored_investigations(
    repo_path: &str,
    store:     &Store,
    limit:     i64,
) -> Result<Vec<atlas_storage::InvestigationRecord>> {
    store.list_investigations(repo_path, limit)
}

/// Load a stored investigation by its row ID.
pub fn load_investigation_by_id(
    id:    i64,
    store: &Store,
) -> Result<Option<InvestigationDocument>> {
    let Some(json) = store.get_investigation_by_id(id)? else { return Ok(None) };
    let doc = serde_json::from_str::<InvestigationDocument>(&json)
        .context("failed to deserialize stored investigation")?;
    Ok(Some(doc))
}

/// Parser version strings — bump when the extractor logic changes in a way
/// that invalidates prior rows (P2-1).  Format: `<lang>-v<major>.<minor>`.
pub const TS_EXTRACTOR_VERSION:     &str = "typescript-v1.0";
pub const C_EXTRACTOR_VERSION:      &str = "c-v1.0";
pub const RUST_EXTRACTOR_VERSION:   &str = "rust-v1.0";
pub const PYTHON_EXTRACTOR_VERSION: &str = "python-v1.0";

pub fn ingest_typescript(repo_path: &str, store: &Store) -> Result<usize> {
    let awareness = RepoAwareness::load(repo_path);
    let (edges, outcomes) = ts_structural::extract_all_with_outcomes(repo_path);
    store.clear_structural_edges(repo_path)?;
    let mut kept = 0usize;
    let mut excluded = 0usize;
    for edge in &edges {
        if awareness.is_excluded(&edge.source_file) || awareness.is_excluded(&edge.target_file) {
            excluded += 1;
            continue;
        }
        store.insert_structural_edge_versioned(edge, repo_path, TS_EXTRACTOR_VERSION)?;
        kept += 1;
    }
    // Authoritative per-file analysis status.  Analyzed/parser_failure here
    // wins over the extension-based fallback in `stamp_analysis_status`.
    for outcome in &outcomes {
        if awareness.is_excluded(&outcome.file) { continue; }
        write_analysis_status(store, repo_path, outcome)?;
    }
    if excluded > 0 {
        info!("repository awareness excluded {} structural edges", excluded);
    }
    info!("typescript structural edges inserted={}", kept);
    Ok(kept)
}

/// Returns true if `repo_path` contains any C/C++/CUDA/ObjC source files.
pub fn repo_has_c_files(repo_path: &str) -> bool {
    c_structural::repo_has_c_files(repo_path)
}

/// Extract `#include` edges from all C/C++/CUDA/ObjC files and persist them.
/// Called automatically during ingest when C-family files are detected.
pub fn ingest_c(repo_path: &str, store: &Store) -> Result<usize> {
    let awareness = RepoAwareness::load(repo_path);
    let (edges, outcomes) = c_structural::extract_all_with_outcomes(repo_path);
    let mut kept = 0usize;
    for edge in &edges {
        if awareness.is_excluded(&edge.source_file) || awareness.is_excluded(&edge.target_file) {
            continue;
        }
        store.insert_structural_edge_versioned(edge, repo_path, C_EXTRACTOR_VERSION)?;
        kept += 1;
    }
    for outcome in &outcomes {
        if awareness.is_excluded(&outcome.file) { continue; }
        write_analysis_status(store, repo_path, outcome)?;
    }
    info!("c structural edges inserted={}", kept);
    Ok(kept)
}

pub fn repo_has_rust_files(repo_path: &str) -> bool {
    rust_structural::repo_has_rust_files(repo_path)
}

pub fn repo_has_typescript_files(repo_path: &str) -> bool {
    ts_structural::repo_has_ts_files(repo_path)
}

pub fn repo_has_python_files(repo_path: &str) -> bool {
    python_structural::repo_has_python_files(repo_path)
}

/// Extract `import`/`from ... import` edges from all Python files and persist them.
pub fn ingest_python(repo_path: &str, store: &Store) -> Result<usize> {
    let awareness = RepoAwareness::load(repo_path);
    let (edges, outcomes) = python_structural::extract_all_with_outcomes(repo_path);
    let mut kept = 0usize;
    for edge in &edges {
        if awareness.is_excluded(&edge.source_file) || awareness.is_excluded(&edge.target_file) {
            continue;
        }
        store.insert_structural_edge_versioned(edge, repo_path, PYTHON_EXTRACTOR_VERSION)?;
        kept += 1;
    }
    for outcome in &outcomes {
        if awareness.is_excluded(&outcome.file) { continue; }
        write_analysis_status(store, repo_path, outcome)?;
    }
    info!("python structural edges inserted={}", kept);
    Ok(kept)
}

/// Extract `use crate::` / `use super::` edges from all Rust files and persist them.
/// Called automatically during ingest when Rust files are detected.
pub fn ingest_rust(repo_path: &str, store: &Store) -> Result<usize> {
    let awareness = RepoAwareness::load(repo_path);
    let (edges, outcomes) = rust_structural::extract_all_with_outcomes(repo_path);
    let mut kept = 0usize;
    for edge in &edges {
        if awareness.is_excluded(&edge.source_file) || awareness.is_excluded(&edge.target_file) {
            continue;
        }
        store.insert_structural_edge_versioned(edge, repo_path, RUST_EXTRACTOR_VERSION)?;
        kept += 1;
    }
    for outcome in &outcomes {
        if awareness.is_excluded(&outcome.file) { continue; }
        write_analysis_status(store, repo_path, outcome)?;
    }
    info!("rust structural edges inserted={}", kept);
    Ok(kept)
}

/// Persist a per-file analysis outcome from a language extractor.
/// Authoritative — overrides any prior status for the same file.
fn write_analysis_status(
    store:     &Store,
    repo_path: &str,
    outcome:   &atlas_parser::FileAnalysis,
) -> Result<()> {
    use atlas_parser::FileAnalysisStatus;
    let (status, detail) = match &outcome.status {
        FileAnalysisStatus::Analyzed              => ("analyzed", None),
        FileAnalysisStatus::ParserFailure { reason } => ("parser_failure", Some(reason.as_str())),
    };
    if let Some(reason) = detail {
        info!("parser_failure: {}  reason={}", outcome.file, reason);
    }
    store.set_analysis_status(&outcome.file, repo_path, status)?;
    Ok(())
}

/// Recognised configuration file kinds and their identifying paths (P1-8).
///
/// Order matters — the first pattern that matches wins.  Extending this list
/// is intentional: adding a kind must be a decision, not a silent side effect.
const CONFIG_ARTIFACTS: &[(&str, &str)] = &[
    ("package_json",         "package.json"),
    ("package_lock",         "package-lock.json"),
    ("yarn_lock",            "yarn.lock"),
    ("pnpm_lock",            "pnpm-lock.yaml"),
    ("bun_lock",             "bun.lockb"),
    ("tsconfig",             "tsconfig.json"),
    ("tsconfig_base",        "tsconfig.base.json"),
    ("tsconfig_build",       "tsconfig.build.json"),
    ("cargo_toml",           "Cargo.toml"),
    ("cargo_lock",           "Cargo.lock"),
    ("dockerfile",           "Dockerfile"),
    ("docker_compose",       "docker-compose.yml"),
    ("docker_compose_yaml",  "docker-compose.yaml"),
    ("docker_compose_local", "docker-compose.local.yml"),
    ("pnpm_workspace",       "pnpm-workspace.yaml"),
    ("gitignore",            ".gitignore"),
    ("npmrc",                ".npmrc"),
    ("nvmrc",                ".nvmrc"),
];

/// Persist raw configuration artifact evidence for a repository.
///
/// Each recognised config file at the repository root is stored verbatim in
/// `configuration_artifacts` with its SHA-256 for change-detection.  Nested
/// configs (e.g. `packages/*/package.json`) are out of scope for this pass —
/// they earn ingestion when a real investigation demonstrates the need.
pub fn ingest_configuration_artifacts(repo_path: &str, store: &Store) -> Result<usize> {
    use sha2::{Digest, Sha256};

    let root = Path::new(repo_path);
    let mut count = 0usize;
    for (kind, filename) in CONFIG_ARTIFACTS {
        let path = root.join(filename);
        // .lockb is binary — try text read but skip if non-UTF-8.
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let sha = hex_encode(&hasher.finalize());
        store.insert_configuration_artifact(repo_path, filename, kind, &content, &sha)?;
        count += 1;
    }
    info!("configuration artifacts ingested={}", count);
    Ok(count)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// Run `repo_inspector` during single-repo ingest and persist the resulting
/// `ProfileClaim`s (P1-9).  When no `projects`/`repositories` row exists,
/// a synthetic project + repository record is created keyed on `repo_path`.
///
/// Idempotent: repeated calls upsert claims via `replace_profile_claims`.
pub fn ingest_profile_claims(repo_path: &str, store: &Store) -> Result<usize> {
    // Ensure a projects row exists for the synthetic default project.
    // Name is fixed so the synthetic project is discoverable.
    const SYNTHETIC_PROJECT: &str = "_atlas_ingest";
    if store.get_project_by_name(SYNTHETIC_PROJECT)?.is_none() {
        let _ = store.create_project(SYNTHETIC_PROJECT, Some("Auto-created by single-repo ingest"))?;
    }
    let project = store.get_project_by_name(SYNTHETIC_PROJECT)?
        .expect("synthetic project must exist after create");

    // Ensure a repositories row exists for this repo_path.
    let repo_row = match store.get_repository_by_path(repo_path)? {
        Some(r) => r,
        None => {
            let name = std::path::Path::new(repo_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(repo_path)
                .to_string();
            let id = store.register_repository(
                project.id,
                &name,
                None,                       // role_label
                Some(repo_path),            // local_path
                None,                       // remote_url
                &atlas_ir::ExistenceSource::LocalObserved,
                &atlas_ir::AccessState::Accessible,
                &atlas_ir::IngestionState::Ingested,
            )?;
            atlas_ir::RepositoryRecord {
                id,
                project_id:       project.id,
                name,
                role_label:       None,
                local_path:       Some(repo_path.to_string()),
                remote_url:       None,
                existence_source: atlas_ir::ExistenceSource::LocalObserved,
                access_state:     atlas_ir::AccessState::Accessible,
                ingestion_state:  atlas_ir::IngestionState::Ingested,
            }
        }
    };

    let claims = repo_inspector::inspect_repository(repo_path)?;
    let count  = claims.len();
    let now    = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    store.replace_profile_claims(repo_row.id, &claims, now)?;
    info!("profile claims ingested={}", count);
    Ok(count)
}

/// Stamp `files.analysis_status` for every tracked file in `repo_path`.
///
/// Runs AFTER all language extractors so that files an extractor visited are
/// marked `analyzed`.  Files whose extension identifies a source language
/// Atlas has no extractor for are marked `not_analyzed_language`.  Everything
/// else is `not_source_file`.
///
/// Distinct from "file appears in structural_edges": a file with zero edges
/// after an `analyzed` status genuinely has zero imports (e.g. a leaf module).
/// `no rows` and `not_analyzed` were previously indistinguishable.
pub fn stamp_analysis_status(repo_path: &str, store: &Store) -> Result<usize> {
    let paths = store.all_file_paths(repo_path)?;
    let mut stamped = 0usize;
    for path in &paths {
        let status = classify_file_analysis(path);
        // if-unset: never clobber an authoritative `analyzed` or
        // `parser_failure` written by a language extractor upstream.
        store.set_analysis_status_if_unset(path, repo_path, status)?;
        stamped += 1;
    }
    Ok(stamped)
}

/// Deterministic per-extension classification.  Reflects the current
/// extractor coverage — extending this must be a decision, not a silent
/// side effect of adding a new parser.
fn classify_file_analysis(path: &str) -> &'static str {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let ext_lower = ext.to_lowercase();

    match ext_lower.as_str() {
        // Languages Atlas has extractors for.
        "ts" | "tsx"                                        => "analyzed",
        "js" | "jsx" | "mjs" | "cjs"                        => "analyzed",
        "rs"                                                => "analyzed",
        "py"                                                => "analyzed",
        "c" | "h" | "cpp" | "cc" | "hpp" | "cu" | "m"       => "analyzed",
        // Recognisable source languages Atlas has no extractor for.
        "go" | "java" | "kt" | "kts" | "swift" | "rb"
        | "php" | "cs" | "scala" | "clj" | "hs" | "ml"
        | "lua" | "sh" | "bash" | "zsh" | "fish" | "dart"
        | "elm" | "erl" | "ex" | "exs" | "fs" | "fsi"
        | "vue" | "svelte" | "sol"                          => "not_analyzed_language",
        // Documentation / config / assets.
        _                                                   => "not_source_file",
    }
}

/// Ingest markdown documentation.
///
/// Sources, in deterministic precedence order:
///   1. `docs/decisions/*.md`  (top-level only) → doc_type = "decision"
///   2. `docs/adr/*.md`        (top-level only) → doc_type = "adr"
///   3. Root `README.md`                        → doc_type = "readme"
///   4. Any other `*.md` under `docs/` (recursive) → doc_type = "doc"
///
/// The recursive pass skips anything under `docs/decisions/` or `docs/adr/` so
/// files claimed by (1) or (2) are never re-classified as generic docs.  This
/// keeps precedence explicit rather than relying on `INSERT OR REPLACE` order.
///
/// Each file's full content (frontmatter + body) is stored verbatim so body
/// text is searchable via `atlas search`.
pub fn ingest_documents(repo_path: &str, store: &Store) -> Result<usize> {
    let mut count = 0;
    let root = Path::new(repo_path);

    // (1) and (2): flat scans — unchanged behaviour.
    let flat_scans = [("docs/decisions", "decision"), ("docs/adr", "adr")];
    for (subdir, doc_type) in &flat_scans {
        let dir = root.join(subdir);
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().map(|e| e == "md").unwrap_or(false) {
                ingest_one_doc(&path, doc_type, repo_path, store)?;
                count += 1;
            }
        }
    }

    // (3): root README.md.
    let readme = root.join("README.md");
    if readme.is_file() {
        ingest_one_doc(&readme, "readme", repo_path, store)?;
        count += 1;
    }

    // (3b): nested README.md files (e.g. crates/*/README.md, packages/*/README.md).
    // Respects RepoAwareness so `node_modules/**/README.md` and `target/**/README.md`
    // do not leak in.  Skips the root README (already handled above).
    let awareness = RepoAwareness::load(repo_path);
    let mut nested_readmes: Vec<std::path::PathBuf> = Vec::new();
    collect_readmes_recursive(root, root, &awareness, &mut nested_readmes);
    for path in nested_readmes {
        // Skip the root README — already ingested above.
        if path == readme { continue; }
        ingest_one_doc(&path, "readme", repo_path, store)?;
        count += 1;
    }

    // (4): every other .md under docs/, recursively.
    let docs_dir = root.join("docs");
    if docs_dir.is_dir() {
        let decisions_dir = docs_dir.join("decisions");
        let adr_dir       = docs_dir.join("adr");
        let mut found: Vec<std::path::PathBuf> = Vec::new();
        collect_markdown_recursive(&docs_dir, &mut found);
        for path in found {
            if path.starts_with(&decisions_dir) || path.starts_with(&adr_dir) {
                continue;
            }
            ingest_one_doc(&path, "doc", repo_path, store)?;
            count += 1;
        }
    }

    info!("documents ingested={}", count);
    Ok(count)
}

/// Recursively walk `dir` collecting every `README.md` (case-sensitive).
/// Applies `RepoAwareness` to prune excluded subtrees (`node_modules/`,
/// `target/`, `.git/`, etc.) so third-party READMEs cannot leak into
/// Atlas's evidence surface.  Also skips `.git` unconditionally.
fn collect_readmes_recursive(
    root:      &Path,
    dir:       &Path,
    awareness: &RepoAwareness,
    out:       &mut Vec<std::path::PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut sorted: Vec<_> = entries.flatten().collect();
    sorted.sort_by_key(|e| e.file_name());
    for entry in sorted {
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel_path_owned: String = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string_lossy().into_owned());

        if ft.is_dir() {
            if name == ".git" { continue; }
            let probe = format!("{}/", rel_path_owned);
            if awareness.is_excluded(&probe) { continue; }
            collect_readmes_recursive(root, &path, awareness, out);
        } else if ft.is_file() && name == "README.md" {
            out.push(path);
        }
    }
}

fn ingest_one_doc(
    path:      &Path,
    doc_type:  &str,
    repo_path: &str,
    store:     &Store,
) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let title = extract_frontmatter_title(&content).unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });
    let rel_path = path
        .strip_prefix(repo_path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned());
    store.insert_document(&rel_path, doc_type, &title, &content, repo_path)?;
    Ok(())
}

fn collect_markdown_recursive(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    // Sort for deterministic iteration order.
    let mut sorted: Vec<_> = entries.flatten().collect();
    sorted.sort_by_key(|e| e.file_name());
    for entry in sorted {
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        if ft.is_dir() {
            collect_markdown_recursive(&path, out);
        } else if ft.is_file()
            && path.extension().map(|e| e == "md").unwrap_or(false)
        {
            out.push(path);
        }
    }
}

fn extract_frontmatter_title(content: &str) -> Option<String> {
    parse_frontmatter(content).remove("title")
}

// ─── Repository Tree (v0.7c) ─────────────────────────────────────────────────
//
// Transient spatial view over the working tree.  Not persisted, not joined
// against ingestion evidence.  The RepoAwareness exclusion rules used
// elsewhere in this crate are reused as-is; nothing here weakens or extends
// them.  Purpose: give downstream commands a stable navigation coordinate
// system.  See docs/decisions/2026-08-08-repository-tree-view.md.

const REPOSITORY_TREE_SCHEMA_VERSION: u32 = 1;

/// Walk the working tree at `repo_path` from disk and produce a
/// `RepositoryTree`.  Every directory is visited in alphabetical order of its
/// entry basenames for deterministic output.  Excluded prefixes (build
/// artifacts, `.gitignore` simple names, plus `.git` itself) are pruned
/// during the walk and reported in `RepositoryTree.excluded`.
///
/// `depth_limit`:
///   * `None`      — walk to every leaf (unbounded).
///   * `Some(0)`   — root node only, no children.
///   * `Some(N>0)` — root plus N levels of descendants.  Directories reached
///                   at exactly depth N appear with `children: []`.
pub fn build_repository_tree(
    repo_path:   &str,
    depth_limit: Option<u32>,
) -> Result<RepositoryTree> {
    let awareness = RepoAwareness::load(repo_path);
    let root_path = Path::new(repo_path);
    let root_name = root_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo_path.to_string());

    let mut excluded: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let root = walk_tree(
        root_path,
        String::new(),
        root_name,
        &awareness,
        depth_limit,
        0,
        &mut excluded,
    );

    Ok(RepositoryTree {
        schema_version: REPOSITORY_TREE_SCHEMA_VERSION,
        repo_path:      repo_path.to_string(),
        root,
        depth_limit,
        excluded:       excluded.into_iter().collect(),
    })
}

fn walk_tree(
    dir:            &Path,
    relative_path:  String,
    name:           String,
    awareness:      &RepoAwareness,
    depth_limit:    Option<u32>,
    current_depth:  u32,
    excluded:       &mut std::collections::BTreeSet<String>,
) -> TreeNode {
    let is_dir = dir.is_dir();
    let kind = if is_dir { TreeNodeKind::Directory } else { TreeNodeKind::File };

    let mut children: Vec<TreeNode> = Vec::new();
    if is_dir {
        let can_recurse = depth_limit.map(|d| current_depth < d).unwrap_or(true);
        if can_recurse {
            if let Ok(entries) = std::fs::read_dir(dir) {
                let mut sorted: Vec<_> = entries.flatten().collect();
                sorted.sort_by_key(|e| e.file_name());
                for entry in sorted {
                    let Ok(ft) = entry.file_type() else { continue };
                    let child_name = entry.file_name().to_string_lossy().into_owned();
                    let child_rel = if relative_path.is_empty() {
                        child_name.clone()
                    } else {
                        format!("{}/{}", relative_path, child_name)
                    };

                    // RepoAwareness stores prefixes with a trailing '/'; probe with the
                    // matching shape so the same prefix matches a directory and any
                    // file beneath it.
                    let probe = if ft.is_dir() {
                        format!("{}/", child_rel)
                    } else {
                        child_rel.clone()
                    };
                    if awareness.is_excluded(&probe) {
                        excluded.insert(child_rel);
                        continue;
                    }
                    // Additionally skip the `.git` directory unconditionally.  Git
                    // internals are never useful in a spatial view; adding this to
                    // RepoAwareness would change ingestion behaviour elsewhere, so
                    // the filter is scoped to this walker.
                    if ft.is_dir() && child_name == ".git" {
                        excluded.insert(child_rel);
                        continue;
                    }

                    children.push(walk_tree(
                        &entry.path(),
                        child_rel,
                        child_name,
                        awareness,
                        depth_limit,
                        current_depth + 1,
                        excluded,
                    ));
                }
            }
        }
    }

    TreeNode { name, relative_path, kind, children }
}

// ─── Peer Structure (v0.8a — B1) ─────────────────────────────────────────────
//
// Aggregation over the `files` table.  No new storage, no new extractor.
//
// Algorithm reuses the *shape* of the peer-observation logic already in
// `apps/cli/src/commands/structural.rs` (peer enumeration → element
// counting → prevalence threshold → deviation reporting), but the data
// source is file-existence rather than structural edges, and the peer
// unit is a directory rather than a compound-suffix file family.

const PEER_STRUCTURE_SCHEMA_VERSION: u32 = 1;

/// Default deviation threshold: strict majority of peers.
/// Consumers can override via a caller-supplied fraction.
const DEFAULT_DEVIATION_THRESHOLD: (usize, usize) = (1, 2); // strict majority = num*den > den*num_supplied when num=1,den=2

/// Default "low-complexity peer" file-count cutoff.  Reported SEPARATELY
/// from the prevalence denominator; peers below this are still counted
/// as peers.
const DEFAULT_LOW_COMPLEXITY_FILE_THRESHOLD: usize = 5;

/// Detect repeated structural patterns across the immediate directory
/// peers of `subject`.
///
/// Peer resolution:
///   * If `subject` itself contains ≥ 1 immediate child directory in the
///     `files` table, `subject` IS the peer parent; peers are its immediate
///     child directories.
///   * Otherwise, `subject`'s parent is used as the peer parent and
///     `subject` becomes one of the peers.
///
/// Everything derived from `store.all_file_paths(repo_path)` — no schema
/// change, no new extractor.
pub fn detect_peer_structure(
    subject:   &str,
    repo_path: &str,
    store:     &Store,
) -> Result<PeerStructureReport> {
    let normalized = subject
        .trim()
        .trim_start_matches('/')
        .trim_end_matches('/')
        .to_string();

    let all_paths = store.all_file_paths(repo_path)?;

    // Enumerate immediate child directories of a given prefix by looking
    // for path segments after that prefix.  If prefix is empty, the
    // "immediate children" are top-level names.
    let immediate_child_dirs = |prefix: &str| -> Vec<String> {
        let prefix_with_slash = if prefix.is_empty() {
            String::new()
        } else {
            format!("{}/", prefix)
        };
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for p in &all_paths {
            if !p.starts_with(&prefix_with_slash) { continue; }
            let rest = &p[prefix_with_slash.len()..];
            // Only immediate CHILDREN — must have a slash after the first segment
            // (i.e. the segment is a directory, not a file at this level).
            if let Some(slash) = rest.find('/') {
                let seg = &rest[..slash];
                if !seg.is_empty() { set.insert(seg.to_string()); }
            }
        }
        set.into_iter().collect()
    };

    // Determine peer_parent + peers.
    let children_of_subject = immediate_child_dirs(&normalized);
    let (peer_parent, peers) = if !children_of_subject.is_empty() {
        (normalized.clone(), children_of_subject)
    } else if let Some(idx) = normalized.rfind('/') {
        let parent = normalized[..idx].to_string();
        let siblings = immediate_child_dirs(&parent);
        (parent, siblings)
    } else {
        // Subject is at repo root with no children AND no parent.
        // Return an empty report (still schema-valid).
        return Ok(PeerStructureReport {
            schema_version:          PEER_STRUCTURE_SCHEMA_VERSION,
            subject:                 subject.to_string(),
            peer_parent:             normalized,
            peers:                   Vec::new(),
            patterns:                Vec::new(),
            singletons:              Vec::new(),
            deviations:              Vec::new(),
            deviation_threshold_num: DEFAULT_DEVIATION_THRESHOLD.0,
            deviation_threshold_den: DEFAULT_DEVIATION_THRESHOLD.1,
            low_complexity_note:     None,
        });
    };

    // Prefer peers that exist on disk when the working tree is available
    // (drops historical/ghost module directories that only live in `files`).
    let peers: Vec<String> = {
        let root = std::path::Path::new(repo_path);
        if root.is_dir() {
            peers
                .into_iter()
                .filter(|p| root.join(&peer_parent).join(p).is_dir())
                .collect()
        } else {
            peers
        }
    };

    let peer_count = peers.len();

    // For each peer, enumerate its structural elements from the files table.
    // Element vocabulary is deliberately small and deterministic:
    //   1. Immediate subdirectory names → "<name>/"
    //   2. Specific well-known file identities within subdirs:
    //         graphql/permissions.ts       (exact file)
    //         graphql/*.typeDefs.ts        (suffix under graphql/)
    //         graphql/*.resolvers.ts       (same)
    //         services/*.service.ts        (suffix under services/)
    //         models/*.model.ts            (suffix under models/)
    //         validation/*.validation.ts   (either 'validation' or 'validations')
    //         validations/*.validation.ts
    //         providers/*.provider.ts
    //         tests/                       (at parent level — mirrored test dir)
    //
    // This list is intentionally small.  Anything else the caller wants to
    // check across peers can be added by extending the pattern set.
    let elements_of = |peer_name: &str| -> std::collections::BTreeSet<String> {
        let peer_prefix = format!("{}/{}/", peer_parent, peer_name);
        let mut elems: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

        for p in &all_paths {
            if !p.starts_with(&peer_prefix) { continue; }
            let rest = &p[peer_prefix.len()..];
            if rest.is_empty() { continue; }

            // (1) Immediate subdirectory.
            if let Some(slash) = rest.find('/') {
                let dir = &rest[..slash];
                if !dir.is_empty() { elems.insert(format!("{}/", dir)); }
            }

            // (2) Specific well-known file identities.
            //
            // Two shapes:
            //   * `label = None`     → EXACT filename match (`permissions.ts`).
            //   * `label = Some(_)`  → SUFFIX pattern (`*.service.ts`) at depth 1.
            //
            // Splitting the two prevents `blockchain.permissions.ts` from being
            // silently counted as `permissions.ts` — that's a genuine naming
            // deviation the report should surface, not merge.
            for (subdir, pat, label) in [
                ("graphql",     "permissions.ts", None::<&str>),
                ("graphql",     ".typeDefs.ts",   Some("graphql/*.typeDefs.ts")),
                ("graphql",     ".resolvers.ts",  Some("graphql/*.resolvers.ts")),
                ("services",    ".service.ts",    Some("services/*.service.ts")),
                ("models",      ".model.ts",      Some("models/*.model.ts")),
                ("validation",  ".validation.ts", Some("validation/*.validation.ts")),
                ("validations", ".validation.ts", Some("validations/*.validation.ts")),
                ("providers",   ".provider.ts",   Some("providers/*.provider.ts")),
            ] {
                let sub_prefix = format!("{}/", subdir);
                let Some(rest_after_sub) = rest.strip_prefix(&sub_prefix) else { continue };
                if rest_after_sub.contains('/') { continue; }  // only depth 1 under subdir
                let matched = match label {
                    None    => rest_after_sub == pat,                    // exact filename
                    Some(_) => rest_after_sub.ends_with(pat),             // suffix pattern
                };
                if matched {
                    let elem = match label {
                        Some(l) => l.to_string(),
                        None    => format!("{}/{}", subdir, pat),
                    };
                    elems.insert(elem);
                }
            }
        }
        elems
    };

    // Enumerate elements per peer once, then aggregate.
    let mut per_peer_elements: Vec<(String, std::collections::BTreeSet<String>)> =
        peers.iter().map(|p| (p.clone(), elements_of(p))).collect();

    // Union all elements observed anywhere → count occurrences.
    let mut element_presence: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (peer, elems) in &per_peer_elements {
        for e in elems {
            element_presence.entry(e.clone()).or_default().push(peer.clone());
        }
    }
    // sort each present_in list alphabetically (BTreeSet iteration is
    // already sorted but we collected from a vec — sort defensively).
    for v in element_presence.values_mut() { v.sort(); }

    // Split into patterns (prevalence ≥ 2) and singletons (prevalence == 1).
    let mut patterns:   Vec<PeerStructurePattern> = Vec::new();
    let mut singletons: Vec<PeerStructurePattern> = Vec::new();
    for (element, present_in) in element_presence {
        let n = present_in.len();
        let record = PeerStructurePattern {
            element,
            present_in,
            prevalence_num: n,
            prevalence_den: peer_count,
        };
        if n >= 2 { patterns.push(record); }
        else       { singletons.push(record); }
    }
    // Patterns sorted by prevalence descending, then element name.
    patterns.sort_by(|a, b| b.prevalence_num.cmp(&a.prevalence_num).then(a.element.cmp(&b.element)));
    singletons.sort_by(|a, b| a.element.cmp(&b.element));

    // Deviations: for each peer, list elements the peer LACKS that
    // >= threshold of the (full) peer set have.
    // Threshold interpretation: `num * peer_count >= den * present_count`
    //   default (num=1, den=2) → strict majority: `present * 2 > peer_count`.
    // We use `present * threshold_den >= threshold_num * peer_count` for the
    // half-way case (>= threshold rather than > threshold).
    let (thr_num, thr_den) = DEFAULT_DEVIATION_THRESHOLD;
    let meets_threshold = |present: usize| -> bool {
        // Strict majority: present * 2 > peer_count for default (1,2).
        // Generic form: present * thr_den > thr_num * peer_count.
        present * thr_den > thr_num * peer_count
    };
    let mut deviations: Vec<PeerStructureDeviation> = Vec::new();
    for (peer, elems) in &per_peer_elements {
        for pat in &patterns {
            if !elems.contains(&pat.element) && meets_threshold(pat.prevalence_num) {
                deviations.push(PeerStructureDeviation {
                    peer:                 peer.clone(),
                    element:              pat.element.clone(),
                    peer_prevalence_num:  pat.prevalence_num,
                    peer_prevalence_den:  peer_count,
                });
            }
        }
    }
    // Sort deviations for stable output.
    deviations.sort_by(|a, b| a.peer.cmp(&b.peer).then(a.element.cmp(&b.element)));

    // Low-complexity note (derived observation, reported separately).
    let low_complexity_note = {
        let threshold = DEFAULT_LOW_COMPLEXITY_FILE_THRESHOLD;
        let mut low: Vec<(String, usize)> = Vec::new();
        for peer in &peers {
            let peer_prefix = format!("{}/{}/", peer_parent, peer);
            let n = all_paths.iter().filter(|p| p.starts_with(&peer_prefix)).count();
            if n < threshold {
                low.push((peer.clone(), n));
            }
        }
        if low.is_empty() {
            None
        } else {
            low.sort();
            Some(LowComplexityNote { file_count_threshold: threshold, low_complexity_peers: low })
        }
    };

    // Discard the per_peer_elements moves — nothing further needed.
    let _ = &mut per_peer_elements;

    Ok(PeerStructureReport {
        schema_version:          PEER_STRUCTURE_SCHEMA_VERSION,
        subject:                 subject.to_string(),
        peer_parent,
        peers,
        patterns,
        singletons,
        deviations,
        deviation_threshold_num: thr_num,
        deviation_threshold_den: thr_den,
        low_complexity_note,
    })
}

// ─── Show — drill-down (v0.8c — B3) ──────────────────────────────────────────
//
// Resolves a subject to one concrete Atlas record and returns its
// immediate provenance + linked records.  Every linked row carries a
// `token` the caller passes back to `atlas show`.

const SHOW_SCHEMA_VERSION: u32 = 1;
const DEFAULT_SECTION_LIMIT: usize = 10;
const DEFAULT_BODY_EXCERPT_BYTES: usize = 1200;

/// The set of subject kinds `atlas show` knows how to resolve.
/// A caller-supplied `--kind` flag forces this; otherwise `show` infers it
/// from the subject string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowSubjectKind {
    Auto, Commit, Pr, Issue, File, Identity, Document, Config, Run,
}

/// Options controlling drill-down output shape.  Defaults match the CLI
/// defaults; `Default::default()` is the invocation used by tests.
#[derive(Debug, Clone, Copy)]
pub struct ShowOptions {
    pub kind:              ShowSubjectKind,
    pub full:              bool,
    pub section_limit:     usize,
    pub body_excerpt_bytes: usize,
}

impl Default for ShowOptions {
    fn default() -> Self {
        Self {
            kind:              ShowSubjectKind::Auto,
            full:              false,
            section_limit:     DEFAULT_SECTION_LIMIT,
            body_excerpt_bytes: DEFAULT_BODY_EXCERPT_BYTES,
        }
    }
}

/// Resolve `subject` into a concrete Atlas record and gather its linked
/// evidence.  Every returned link is a token a caller can pass back to
/// `atlas show`.
pub fn show(
    subject_input: &str,
    repo_path:     &str,
    store:         &Store,
    opts:          ShowOptions,
) -> Result<ShowRecord> {
    let latest_run_id = store.latest_ingest_run(repo_path)?.map(|r| r.id);
    let provenance = ShowProvenance {
        repo_path:     repo_path.to_string(),
        ingested_at:   None, // filled per-subject where applicable
        latest_run_id,
    };

    // Resolve subject.
    let (subject, redirect_note, sections_and_links, ingested_at) =
        resolve_subject(subject_input, repo_path, store, opts)?;

    let (sections, mut links) = sections_and_links;
    // Dedup + sort links.
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    links.retain(|l| seen.insert((l.kind.clone(), l.token.clone())));
    links.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.label.cmp(&b.label)));

    let mut provenance = provenance;
    provenance.ingested_at = ingested_at;

    Ok(ShowRecord {
        schema_version: SHOW_SCHEMA_VERSION,
        subject_input:  subject_input.to_string(),
        subject,
        redirect_note,
        sections,
        links,
        provenance,
    })
}

/// Subject-resolution dispatcher.  Returns:
///   * the concrete `ShowSubject`,
///   * an optional historical-path redirect note,
///   * the ordered sections + all links referenced anywhere in the record,
///   * the subject's `ingested_at` (when available on the source row).
fn resolve_subject(
    input:     &str,
    repo_path: &str,
    store:     &Store,
    opts:      ShowOptions,
) -> Result<(
    ShowSubject,
    Option<HistoricalRedirect>,
    (Vec<ShowSection>, Vec<ShowLink>),
    Option<i64>,
)> {
    let s = input.trim();

    // Explicit-kind fast paths.  Order matters — try prefixes before
    // ambiguous auto-detection.
    if opts.kind == ShowSubjectKind::Run || s.starts_with("run:") {
        return build_run_view(strip_prefix(s, "run:"), repo_path, store, opts);
    }
    if opts.kind == ShowSubjectKind::Config || s.starts_with("config:") {
        return build_config_view(strip_prefix(s, "config:"), repo_path, store, opts);
    }
    if opts.kind == ShowSubjectKind::Document || s.starts_with("doc:") {
        return build_document_view(strip_prefix(s, "doc:"), repo_path, store, opts);
    }
    if opts.kind == ShowSubjectKind::Identity || s.starts_with("id:") {
        let n: i64 = strip_prefix(s, "id:").parse().with_context(|| format!("expected `id:<n>`, got `{}`", s))?;
        return build_identity_view(n, repo_path, store, opts);
    }
    if opts.kind == ShowSubjectKind::Pr || s.starts_with("pr:") || s.starts_with("pr#") || s.starts_with('#') {
        let raw = s.trim_start_matches("pr:").trim_start_matches("pr#").trim_start_matches('#');
        let n: i64 = raw.parse().with_context(|| format!("expected PR number, got `{}`", raw))?;
        return build_pr_view(n, repo_path, store, opts);
    }
    if opts.kind == ShowSubjectKind::Issue || s.starts_with("issue:") || s.starts_with("issue#") {
        let raw = s.trim_start_matches("issue:").trim_start_matches("issue#");
        let n: i64 = raw.parse().with_context(|| format!("expected issue number, got `{}`", raw))?;
        return build_issue_view(n, repo_path, store, opts);
    }

    // Auto-detection.
    if opts.kind == ShowSubjectKind::Auto || opts.kind == ShowSubjectKind::Commit {
        // Commit hash: 7–40 hex chars.
        if looks_like_commit_hash(s) {
            let matches = store.resolve_commit_prefix(s, repo_path)?;
            match matches.len() {
                0 if opts.kind == ShowSubjectKind::Commit =>
                    anyhow::bail!("no commit found with hash prefix `{}`", s),
                0 => {}, // fall through to other auto detections
                1 => return build_commit_view(&matches[0], repo_path, store, opts),
                _ => {
                    let list: Vec<String> = matches.iter().take(10).map(|h| h[..16.min(h.len())].to_string()).collect();
                    anyhow::bail!(
                        "commit hash prefix `{}` is ambiguous — matches {} commits:\n  {}",
                        s, matches.len(), list.join("\n  ")
                    );
                }
            }
        }
    }

    // Auto: document by exact path.
    if opts.kind == ShowSubjectKind::Auto {
        if store.document_by_path(s, repo_path)?.is_some() {
            return build_document_view(s, repo_path, store, opts);
        }
    }

    // Auto: config artifact by exact path.
    if opts.kind == ShowSubjectKind::Auto {
        if store.configuration_artifact(s, repo_path)?.is_some() {
            return build_config_view(s, repo_path, store, opts);
        }
    }

    // Auto: file by path.  Also handles the historical-path redirect.
    if opts.kind == ShowSubjectKind::Auto || opts.kind == ShowSubjectKind::File {
        return build_file_view(s, repo_path, store, opts);
    }

    anyhow::bail!(
        "could not resolve subject `{}` — supported forms:\n  \
         commit hash (7-40 hex)\n  \
         #<n> | pr:<n> | pr#<n>\n  \
         issue:<n> | issue#<n>\n  \
         id:<n>            (file identity)\n  \
         config:<path>     (configuration artifact)\n  \
         doc:<path>        (document)\n  \
         run:<id> | run:latest\n  \
         <path>            (file, auto-detected)",
        s
    );
}

fn strip_prefix<'a>(s: &'a str, prefix: &str) -> &'a str {
    s.strip_prefix(prefix).unwrap_or(s)
}

fn looks_like_commit_hash(s: &str) -> bool {
    s.len() >= 7
        && s.len() <= 40
        && s.chars().all(|c| c.is_ascii_hexdigit())
}

// ─── Per-subject builders ────────────────────────────────────────────────────

fn build_commit_view(
    hash:      &str,
    repo_path: &str,
    store:     &Store,
    opts:      ShowOptions,
) -> Result<(ShowSubject, Option<HistoricalRedirect>, (Vec<ShowSection>, Vec<ShowLink>), Option<i64>)> {
    let commit = store.commit_by_hash(hash, repo_path)?
        .ok_or_else(|| anyhow::anyhow!("commit {} not found", hash))?;

    let mut sections = Vec::new();
    let mut all_links = Vec::new();

    // PARENTS
    let parents = store.commit_parents(&commit.hash, repo_path)?;
    let parent_rows: Vec<ShowRow> = parents.iter().map(|p| {
        let link = ShowLink {
            label: format!("commit {}", &p[..7.min(p.len())]),
            token: p.clone(),
            kind:  "commit".to_string(),
        };
        all_links.push(link.clone());
        ShowRow { display: p.clone(), link: Some(link) }
    }).collect();
    sections.push(ShowSection {
        title:            "PARENTS".to_string(),
        kind:             ShowSectionKind::Deterministic,
        provenance_table: "commit_parents".to_string(),
        rows:             parent_rows,
        truncated_count:  None,
    });

    // CHANGED FILES  (repo-scoped: joins through `commits` so a shared
    // SHA under a different repository cannot leak files into this query).
    let files = store.commit_changed_files(&commit.hash, repo_path)?;
    let (shown_files, truncated) = truncate(&files, opts);
    let file_rows: Vec<ShowRow> = shown_files.iter().map(|p| {
        let link = ShowLink {
            label: format!("file {}", p),
            token: p.clone(),
            kind:  "file".to_string(),
        };
        all_links.push(link.clone());
        ShowRow { display: p.clone(), link: Some(link) }
    }).collect();
    sections.push(ShowSection {
        title:            "CHANGED FILES".to_string(),
        kind:             ShowSectionKind::Deterministic,
        provenance_table: "commit_files".to_string(),
        rows:             file_rows,
        truncated_count:  truncated,
    });

    // LINKED PULL REQUESTS (via merge_commit_sha + C4-B message refs like (#N))
    let mut prs = store.prs_for_merge_commit(&commit.hash, repo_path)?;
    let mut seen_pr: HashSet<i64> = prs.iter().map(|p| p.number).collect();
    let mut pr_link_sources: Vec<(i64, &'static str)> = prs
        .iter()
        .map(|p| (p.number, "merge_commit_sha"))
        .collect();
    // C4-B: parse (#123) / #123 from commit message and resolve to ingested PRs/issues.
    let msg_nums = crate::evidence_reasoning::parse_github_numbers(&commit.message);
    let mut message_issue_rows: Vec<ShowRow> = Vec::new();
    for n in msg_nums {
        if let Ok(Some(pr)) = store.pr_by_number(n, repo_path) {
            if seen_pr.insert(pr.number) {
                prs.push(pr);
                pr_link_sources.push((n, "commit_message"));
            }
        } else if let Ok(Some(issue)) = store.issue_by_number(n, repo_path) {
            let link = ShowLink {
                label: format!("issue #{}", issue.number),
                token: format!("issue#{}", issue.number),
                kind:  "issue".to_string(),
            };
            all_links.push(link.clone());
            message_issue_rows.push(ShowRow {
                display: format!(
                    "Issue #{}  {}  [{}]  (via commit message)",
                    issue.number,
                    issue.title,
                    issue.state.to_uppercase()
                ),
                link: Some(link),
            });
        }
    }
    let pr_rows: Vec<ShowRow> = prs
        .iter()
        .map(|pr| {
            let via = pr_link_sources
                .iter()
                .find(|(num, _)| *num == pr.number)
                .map(|(_, s)| *s)
                .unwrap_or("merge_commit_sha");
            let link = ShowLink {
                label: format!("PR #{}", pr.number),
                token: format!("pr#{}", pr.number),
                kind:  "pr".to_string(),
            };
            all_links.push(link.clone());
            ShowRow {
                display: format!(
                    "PR #{}  {}  [{}]  (via {})",
                    pr.number,
                    pr.title,
                    pr.state.to_uppercase(),
                    via
                ),
                link: Some(link),
            }
        })
        .collect();
    sections.push(ShowSection {
        title:            "LINKED PULL REQUESTS".to_string(),
        kind:             ShowSectionKind::Deterministic,
        provenance_table: "pull_requests.merge_commit_sha|commit.message_refs".to_string(),
        rows:             pr_rows,
        truncated_count:  None,
    });
    if !message_issue_rows.is_empty() {
        sections.push(ShowSection {
            title:            "LINKED ISSUES (from commit message)".to_string(),
            kind:             ShowSectionKind::Deterministic,
            provenance_table: "commit.message_refs|issues".to_string(),
            rows:             message_issue_rows,
            truncated_count:  None,
        });
    }

    let subject = ShowSubject::Commit(CommitSubject {
        hash:         commit.hash,
        short_hash:   commit.short_hash,
        author_name:  commit.author_name,
        author_email: commit.author_email,
        timestamp:    commit.timestamp,
        message:      commit.message,
    });
    Ok((subject, None, (sections, all_links), None))
}

fn build_pr_view(
    number:    i64,
    repo_path: &str,
    store:     &Store,
    opts:      ShowOptions,
) -> Result<(ShowSubject, Option<HistoricalRedirect>, (Vec<ShowSection>, Vec<ShowLink>), Option<i64>)> {
    let pr = store.pr_by_number(number, repo_path)?
        .ok_or_else(|| anyhow::anyhow!("PR #{} not found", number))?;
    let body = store.pr_body(number, repo_path)?.unwrap_or_default();

    let mut sections = Vec::new();
    let mut all_links = Vec::new();

    // Merge commit (if any) — followable.
    let mut merge_rows: Vec<ShowRow> = Vec::new();
    if let Some(sha) = &pr.merge_commit_sha {
        let link = ShowLink {
            label: format!("commit {}", &sha[..7.min(sha.len())]),
            token: sha.clone(),
            kind:  "commit".to_string(),
        };
        all_links.push(link.clone());
        merge_rows.push(ShowRow { display: sha.clone(), link: Some(link) });
    }
    sections.push(ShowSection {
        title:            "MERGE COMMIT".to_string(),
        kind:             ShowSectionKind::Deterministic,
        provenance_table: "pull_requests.merge_commit_sha".to_string(),
        rows:             merge_rows,
        truncated_count:  None,
    });

    // Linked issues: `pr_issues` supplies the numbers; each title is
    // separately fetched from `issues`.  Two tables → this section is
    // `Derived` per the ShowSectionKind contract.
    let issue_nums = store.issue_numbers_for_pr(number, repo_path)?;
    let issue_rows: Vec<ShowRow> = issue_nums.iter().map(|n| {
        let title = store.get_issue(*n, repo_path).ok().flatten()
            .map(|(t, _)| t)
            .unwrap_or_else(|| String::new());
        let link = ShowLink {
            label: format!("issue #{}", n),
            token: format!("issue#{}", n),
            kind:  "issue".to_string(),
        };
        all_links.push(link.clone());
        let display = if title.is_empty() {
            format!("issue #{}", n)
        } else {
            format!("issue #{}  {}", n, title)
        };
        ShowRow { display, link: Some(link) }
    }).collect();
    sections.push(ShowSection {
        title:            "LINKED ISSUES".to_string(),
        kind:             ShowSectionKind::Derived,
        provenance_table: "pr_issues + issues".to_string(),
        rows:             issue_rows,
        truncated_count:  None,
    });

    let subject = ShowSubject::Pr(PrSubject {
        number:           pr.number,
        title:            pr.title,
        state:            pr.state,
        author:           pr.author,
        merge_commit_sha: pr.merge_commit_sha,
        created_at:       pr.created_at,
        merged_at:        pr.merged_at,
        body_excerpt:     excerpt_or_full(&body, opts),
    });
    Ok((subject, None, (sections, all_links), None))
}

fn build_issue_view(
    number:    i64,
    repo_path: &str,
    store:     &Store,
    opts:      ShowOptions,
) -> Result<(ShowSubject, Option<HistoricalRedirect>, (Vec<ShowSection>, Vec<ShowLink>), Option<i64>)> {
    // Fetch the full issue row (author + created_at + state + title from
    // one SELECT).  `get_issue` returns only (title, state); this variant
    // exists specifically so `atlas show` doesn't fabricate empty fields.
    let issue = store.issue_by_number(number, repo_path)?
        .ok_or_else(|| anyhow::anyhow!("issue #{} not found", number))?;
    let body = store.issue_body(number, repo_path)?.unwrap_or_default();

    let mut sections = Vec::new();
    let mut all_links = Vec::new();

    // Closing PRs — single-table SELECT over `pr_issues`.
    let pr_nums = store.prs_closing_issue(number, repo_path)?;
    let pr_rows: Vec<ShowRow> = pr_nums.iter().map(|n| {
        let link = ShowLink {
            label: format!("PR #{}", n),
            token: format!("pr#{}", n),
            kind:  "pr".to_string(),
        };
        all_links.push(link.clone());
        ShowRow { display: format!("PR #{}", n), link: Some(link) }
    }).collect();
    sections.push(ShowSection {
        title:            "CLOSING PULL REQUESTS".to_string(),
        kind:             ShowSectionKind::Deterministic,
        provenance_table: "pr_issues".to_string(),
        rows:             pr_rows,
        truncated_count:  None,
    });

    let subject = ShowSubject::Issue(IssueSubject {
        number:       issue.number,
        title:        issue.title,
        state:        issue.state,
        author:       issue.author,
        created_at:   issue.created_at,
        body_excerpt: excerpt_or_full(&body, opts),
    });
    Ok((subject, None, (sections, all_links), None))
}

fn build_file_view(
    input:     &str,
    repo_path: &str,
    store:     &Store,
    opts:      ShowOptions,
) -> Result<(ShowSubject, Option<HistoricalRedirect>, (Vec<ShowSection>, Vec<ShowLink>), Option<i64>)> {
    // Historical-path redirect.
    let (relative_path, redirect_note) = match store.current_path_if_historical(input, repo_path)? {
        Some(current) => {
            let id = store.resolve_path_to_identity(input, repo_path)?.unwrap_or(0);
            (current.clone(), Some(HistoricalRedirect {
                original_subject: input.to_string(),
                current_path:     current,
                identity_id:      id,
            }))
        }
        None => (input.to_string(), None),
    };

    let analysis_status = store.analysis_status(&relative_path, repo_path)?;
    let identity_id     = store.resolve_path_to_identity(&relative_path, repo_path)?;
    let role            = if !relative_path.is_empty() { Some(classify_artifact_role(&relative_path)) } else { None };

    let mut sections = Vec::new();
    let mut all_links = Vec::new();

    // IDENTITY LINEAGE (if applicable)
    if let Some(id) = identity_id {
        let history = store.path_history_for_identity(id, repo_path)?;
        let rows: Vec<ShowRow> = history.iter().map(|obs| {
            let mut display = format!("{}  (intro {}", obs.path, &obs.introduced_by_commit[..7.min(obs.introduced_by_commit.len())]);
            if let Some(sup) = &obs.superseded_by_commit {
                display.push_str(&format!(" → superseded {}", &sup[..7.min(sup.len())]));
            } else {
                display.push_str(" → current");
            }
            display.push(')');
            // Link the introducing commit.
            let link = ShowLink {
                label: format!("commit {}", &obs.introduced_by_commit[..7.min(obs.introduced_by_commit.len())]),
                token: obs.introduced_by_commit.clone(),
                kind:  "commit".to_string(),
            };
            all_links.push(link.clone());
            if let Some(sup) = &obs.superseded_by_commit {
                all_links.push(ShowLink {
                    label: format!("commit {}", &sup[..7.min(sup.len())]),
                    token: sup.clone(),
                    kind:  "commit".to_string(),
                });
            }
            ShowRow { display, link: Some(link) }
        }).collect();
        sections.push(ShowSection {
            title:            "IDENTITY LINEAGE".to_string(),
            kind:             ShowSectionKind::Deterministic,
            provenance_table: "file_path_observations".to_string(),
            rows,
            truncated_count:  None,
        });

        // COMMITS TOUCHING THIS IDENTITY
        let commits = store.commits_for_identity(id, repo_path)?;
        let (shown, trunc) = truncate(&commits, opts);
        let rows: Vec<ShowRow> = shown.iter().map(|c| {
            let link = ShowLink {
                label: format!("commit {}", &c.short_hash),
                token: c.hash.clone(),
                kind:  "commit".to_string(),
            };
            all_links.push(link.clone());
            ShowRow {
                display: format!("{}  {}", c.short_hash, c.message),
                link:    Some(link),
            }
        }).collect();
        sections.push(ShowSection {
            title:            "COMMITS TOUCHING THIS IDENTITY".to_string(),
            kind:             ShowSectionKind::Deterministic,
            provenance_table: "file_identity_commits".to_string(),
            rows,
            truncated_count:  trunc,
        });
    } else {
        // Fall back to path-scoped commits.
        let commits = store.commits_for_file(&relative_path, repo_path)?;
        let (shown, trunc) = truncate(&commits, opts);
        let rows: Vec<ShowRow> = shown.iter().map(|c| {
            let link = ShowLink {
                label: format!("commit {}", &c.short_hash),
                token: c.hash.clone(),
                kind:  "commit".to_string(),
            };
            all_links.push(link.clone());
            ShowRow {
                display: format!("{}  {}", c.short_hash, c.message),
                link:    Some(link),
            }
        }).collect();
        sections.push(ShowSection {
            title:            "COMMITS TOUCHING THIS PATH".to_string(),
            kind:             ShowSectionKind::Deterministic,
            provenance_table: "commit_files".to_string(),
            rows,
            truncated_count:  trunc,
        });
    }

    // STRUCTURAL EDGES — outgoing
    let out = store.structural_edges_for_file(&relative_path, repo_path)?;
    let (shown, trunc) = truncate(&out, opts);
    let out_rows: Vec<ShowRow> = shown.iter().map(|e| {
        let is_external = e.target_file.starts_with("UNRESOLVED:external:");
        let display = format!("{}  → {}{}",
            e.kind, e.target_file,
            e.target_symbol.as_deref().map(|s| format!("::{}", s)).unwrap_or_default());
        let link = if !is_external {
            let l = ShowLink {
                label: format!("file {}", e.target_file),
                token: e.target_file.clone(),
                kind:  "file".to_string(),
            };
            all_links.push(l.clone());
            Some(l)
        } else { None };
        ShowRow { display, link }
    }).collect();
    sections.push(ShowSection {
        title:            "STRUCTURAL EDGES (outgoing)".to_string(),
        kind:             ShowSectionKind::Deterministic,
        provenance_table: "structural_edges".to_string(),
        rows:             out_rows,
        truncated_count:  trunc,
    });

    // STRUCTURAL EDGES — incoming
    let in_edges = store.structural_edges_targeting(&relative_path, repo_path)?;
    let (shown, trunc) = truncate(&in_edges, opts);
    let in_rows: Vec<ShowRow> = shown.iter().map(|e| {
        let display = format!("← {}  ({}{})",
            e.source_file, e.kind,
            e.target_symbol.as_deref().map(|s| format!(", {}", s)).unwrap_or_default());
        let link = ShowLink {
            label: format!("file {}", e.source_file),
            token: e.source_file.clone(),
            kind:  "file".to_string(),
        };
        all_links.push(link.clone());
        ShowRow { display, link: Some(link) }
    }).collect();
    sections.push(ShowSection {
        title:            "STRUCTURAL EDGES (incoming)".to_string(),
        kind:             ShowSectionKind::Deterministic,
        provenance_table: "structural_edges".to_string(),
        rows:             in_rows,
        truncated_count:  trunc,
    });

    let subject = ShowSubject::File(FileSubject {
        relative_path,
        analysis_status,
        identity_id,
        role,
    });
    Ok((subject, redirect_note, (sections, all_links), None))
}

fn build_identity_view(
    identity_id: i64,
    repo_path:   &str,
    store:       &Store,
    opts:        ShowOptions,
) -> Result<(ShowSubject, Option<HistoricalRedirect>, (Vec<ShowSection>, Vec<ShowLink>), Option<i64>)> {
    let summary = store.identity_summary(identity_id, repo_path)?
        .ok_or_else(|| anyhow::anyhow!("identity {} not found", identity_id))?;

    let mut sections = Vec::new();
    let mut all_links = Vec::new();

    // Path history.
    let history = store.path_history_for_identity(identity_id, repo_path)?;
    let hist_rows: Vec<ShowRow> = history.iter().map(|obs| {
        let mut display = format!("{}  (intro {}", obs.path, &obs.introduced_by_commit[..7.min(obs.introduced_by_commit.len())]);
        if let Some(sup) = &obs.superseded_by_commit {
            display.push_str(&format!(" → superseded {}", &sup[..7.min(sup.len())]));
        } else {
            display.push_str(" → current");
        }
        display.push(')');
        let link = ShowLink {
            label: format!("file {}", obs.path),
            token: obs.path.clone(),
            kind:  "file".to_string(),
        };
        all_links.push(link.clone());
        ShowRow { display, link: Some(link) }
    }).collect();
    sections.push(ShowSection {
        title:            "PATH HISTORY".to_string(),
        kind:             ShowSectionKind::Deterministic,
        provenance_table: "file_path_observations".to_string(),
        rows:             hist_rows,
        truncated_count:  None,
    });

    // Commits touching this identity.
    let commits = store.commits_for_identity(identity_id, repo_path)?;
    let (shown, trunc) = truncate(&commits, opts);
    let commit_rows: Vec<ShowRow> = shown.iter().map(|c| {
        let link = ShowLink {
            label: format!("commit {}", &c.short_hash),
            token: c.hash.clone(),
            kind:  "commit".to_string(),
        };
        all_links.push(link.clone());
        ShowRow {
            display: format!("{}  {}", c.short_hash, c.message),
            link:    Some(link),
        }
    }).collect();
    sections.push(ShowSection {
        title:            "COMMITS".to_string(),
        kind:             ShowSectionKind::Deterministic,
        provenance_table: "file_identity_commits".to_string(),
        rows:             commit_rows,
        truncated_count:  trunc,
    });

    let (_, current_path, hist_count, commit_count) = summary;
    let subject = ShowSubject::Identity(IdentitySubject {
        identity_id,
        current_path,
        path_history_count: hist_count as usize,
        commit_count:       commit_count as usize,
    });
    Ok((subject, None, (sections, all_links), None))
}

fn build_document_view(
    file_path: &str,
    repo_path: &str,
    store:     &Store,
    opts:      ShowOptions,
) -> Result<(ShowSubject, Option<HistoricalRedirect>, (Vec<ShowSection>, Vec<ShowLink>), Option<i64>)> {
    let (doc_type, title, body) = store.document_full_by_path(file_path, repo_path)?
        .ok_or_else(|| anyhow::anyhow!("document `{}` not found", file_path))?;

    let mut sections = Vec::new();
    let mut all_links = Vec::new();

    // Underlying file link.
    let file_link = ShowLink {
        label: format!("file {}", file_path),
        token: file_path.to_string(),
        kind:  "file".to_string(),
    };
    all_links.push(file_link.clone());
    sections.push(ShowSection {
        title:            "UNDERLYING FILE".to_string(),
        kind:             ShowSectionKind::Deterministic,
        provenance_table: "files".to_string(),
        rows:             vec![ShowRow { display: file_path.to_string(), link: Some(file_link) }],
        truncated_count:  None,
    });

    // Commits touching this document.
    let commits = store.commits_for_file(file_path, repo_path)?;
    let (shown, trunc) = truncate(&commits, opts);
    let commit_rows: Vec<ShowRow> = shown.iter().map(|c| {
        let link = ShowLink {
            label: format!("commit {}", &c.short_hash),
            token: c.hash.clone(),
            kind:  "commit".to_string(),
        };
        all_links.push(link.clone());
        ShowRow {
            display: format!("{}  {}", c.short_hash, c.message),
            link:    Some(link),
        }
    }).collect();
    sections.push(ShowSection {
        title:            "COMMITS TOUCHING THIS DOCUMENT".to_string(),
        kind:             ShowSectionKind::Deterministic,
        provenance_table: "commit_files".to_string(),
        rows:             commit_rows,
        truncated_count:  trunc,
    });

    let body_bytes = body.as_bytes().len();
    let excerpt = if opts.full { body.clone() } else { truncate_body(&body, opts.body_excerpt_bytes) };
    let subject = ShowSubject::Document(DocumentSubject {
        file_path:    file_path.to_string(),
        doc_type,
        title,
        body_excerpt: excerpt,
        body_bytes,
    });
    Ok((subject, None, (sections, all_links), None))
}

fn build_config_view(
    file_path: &str,
    repo_path: &str,
    store:     &Store,
    opts:      ShowOptions,
) -> Result<(ShowSubject, Option<HistoricalRedirect>, (Vec<ShowSection>, Vec<ShowLink>), Option<i64>)> {
    let art = store.configuration_artifact(file_path, repo_path)?
        .ok_or_else(|| anyhow::anyhow!("configuration artifact `{}` not found", file_path))?;

    let mut sections = Vec::new();
    let mut all_links = Vec::new();

    // Commits touching this artifact.
    let commits = store.commits_for_file(file_path, repo_path)?;
    let (shown, trunc) = truncate(&commits, opts);
    let commit_rows: Vec<ShowRow> = shown.iter().map(|c| {
        let link = ShowLink {
            label: format!("commit {}", &c.short_hash),
            token: c.hash.clone(),
            kind:  "commit".to_string(),
        };
        all_links.push(link.clone());
        ShowRow {
            display: format!("{}  {}", c.short_hash, c.message),
            link:    Some(link),
        }
    }).collect();
    sections.push(ShowSection {
        title:            "COMMITS TOUCHING THIS ARTIFACT".to_string(),
        kind:             ShowSectionKind::Deterministic,
        provenance_table: "commit_files".to_string(),
        rows:             commit_rows,
        truncated_count:  trunc,
    });

    let raw_bytes = art.raw_content.as_bytes().len();
    let excerpt = if opts.full { art.raw_content.clone() } else { truncate_body(&art.raw_content, opts.body_excerpt_bytes) };
    let ingested_at = art.ingested_at;
    let subject = ShowSubject::ConfigArtifact(ConfigArtifactSubject {
        file_path:     art.file_path,
        artifact_kind: art.artifact_kind,
        sha256:        art.sha256,
        raw_bytes,
        body_excerpt:  excerpt,
        ingested_at:   art.ingested_at,
    });
    Ok((subject, None, (sections, all_links), Some(ingested_at)))
}

fn build_run_view(
    id_str:    &str,
    repo_path: &str,
    store:     &Store,
    _opts:     ShowOptions,
) -> Result<(ShowSubject, Option<HistoricalRedirect>, (Vec<ShowSection>, Vec<ShowLink>), Option<i64>)> {
    let run = if id_str == "latest" {
        store.latest_ingest_run(repo_path)?
            .ok_or_else(|| anyhow::anyhow!("no ingest runs recorded for this repo"))?
    } else {
        let id: i64 = id_str.parse().with_context(|| format!("expected `run:<id>` or `run:latest`, got `run:{}`", id_str))?;
        // Repo-scoped lookup: a run id belonging to a different registered
        // repository must not be returned even when the id is a valid rowid
        // globally.  Preserves the repository-isolation invariant.
        store.ingest_run_by_id(id, repo_path)?
            .ok_or_else(|| anyhow::anyhow!("ingest run {} not found in this repository", id))?
    };

    let mut sections = Vec::new();
    let mut all_links = Vec::new();

    // Git head at ingest.
    if let Some(head) = &run.git_head {
        let link = ShowLink {
            label: format!("commit {}", &head[..7.min(head.len())]),
            token: head.clone(),
            kind:  "commit".to_string(),
        };
        all_links.push(link.clone());
        sections.push(ShowSection {
            title:            "GIT HEAD AT INGEST".to_string(),
            kind:             ShowSectionKind::Deterministic,
            provenance_table: "ingest_runs".to_string(),
            rows:             vec![ShowRow { display: head.clone(), link: Some(link) }],
            truncated_count:  None,
        });
    }

    // Stage results.
    let stages: Vec<serde_json::Value> = serde_json::from_str(&run.stages_json).unwrap_or_default();
    let stage_rows: Vec<ShowRow> = stages.iter().map(|s| {
        let name = s.get("stage").and_then(|v| v.as_str()).unwrap_or("?");
        let status = s.get("status").and_then(|v| v.as_str()).unwrap_or("?");
        let detail = s.get("detail").and_then(|v| v.as_str()).unwrap_or("");
        ShowRow { display: format!("{:<24}  {:<8}  {}", name, status, detail), link: None }
    }).collect();
    sections.push(ShowSection {
        title:            "STAGES".to_string(),
        kind:             ShowSectionKind::Deterministic,
        provenance_table: "ingest_runs.stages_json".to_string(),
        rows:             stage_rows,
        truncated_count:  None,
    });

    let subject = ShowSubject::IngestRun(IngestRunSubject {
        id:              run.id,
        started_at:      run.started_at,
        ended_at:        run.ended_at,
        atlas_version:   run.atlas_version,
        git_head:        run.git_head,
        git_branch:      run.git_branch,
        requested_scope: run.requested_scope,
        exit_status:     run.exit_status,
    });
    Ok((subject, None, (sections, all_links), None))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn truncate<'a, T>(items: &'a [T], opts: ShowOptions) -> (&'a [T], Option<usize>) {
    if opts.full || items.len() <= opts.section_limit {
        (items, None)
    } else {
        (&items[..opts.section_limit], Some(items.len() - opts.section_limit))
    }
}

fn truncate_body(body: &str, max_bytes: usize) -> String {
    if body.as_bytes().len() <= max_bytes { return body.to_string(); }
    // Truncate at a char boundary.
    let mut end = max_bytes;
    while end > 0 && !body.is_char_boundary(end) { end -= 1; }
    format!("{}\n… [truncated; use --full for the full body]", &body[..end])
}

/// Body-excerpt policy: return the full body when `opts.full` is set,
/// otherwise truncate to `opts.body_excerpt_bytes`.  Centralised so every
/// per-subject builder honours the CLI flags uniformly (the previous
/// per-builder implementations ignored `opts` — see B3 repair note).
fn excerpt_or_full(body: &str, opts: ShowOptions) -> String {
    if opts.full {
        body.to_string()
    } else {
        truncate_body(body, opts.body_excerpt_bytes)
    }
}

// ─── Module Coupling (v0.8b — B2 aggregation) ────────────────────────────────
//
// Aggregates the existing `structural_edges` table into module-to-module
// coupling records.  Modules are the immediate child directories of
// `subject` (default `src/modules`).
//
// Reuses `store.all_file_paths(repo)` for module enumeration and
// `store.structural_edges_from_prefix("", repo)` (empty-prefix → LIKE '%'
// → all edges) for the edge scan.  No new storage method.
//
// Canonical representation is the SPARSE `cells: Vec<ModuleCouplingCell>`
// list — only non-zero cells are stored.  The dense matrix is a
// render-time convenience produced by the CLI when the module count is
// small enough to display.

const MODULE_COUPLING_SCHEMA_VERSION: u32 = 1;

/// Build a module coupling report for `subject`'s immediate child
/// directories.  For each ordered pair of distinct modules (A, B) with
/// ≥1 structural edge from A to B, emit one `ModuleCouplingCell`.
///
/// Edges wholly within one module (source and target both in the same
/// module) are NOT included — they are internal cohesion, not coupling.
///
/// Edges whose target is `UNRESOLVED:external:*` are reported in
/// `external_dependencies`.  Edges whose target is a repository file
/// outside `subject/*` (e.g. `src/common/…`) are reported in
/// `platform_usage`, aggregated by the target's first path segment.
pub fn compute_module_coupling(
    subject:   &str,
    repo_path: &str,
    store:     &Store,
) -> Result<ModuleCouplingReport> {
    let normalized = subject
        .trim()
        .trim_start_matches('/')
        .trim_end_matches('/')
        .to_string();

    let all_paths = store.all_file_paths(repo_path)?;

    // Enumerate modules = immediate child directories of `normalized`.
    let modules: Vec<String> = {
        let prefix_with_slash = if normalized.is_empty() {
            String::new()
        } else {
            format!("{}/", normalized)
        };
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for p in &all_paths {
            if !p.starts_with(&prefix_with_slash) { continue; }
            let rest = &p[prefix_with_slash.len()..];
            if let Some(slash) = rest.find('/') {
                let seg = &rest[..slash];
                if !seg.is_empty() { set.insert(seg.to_string()); }
            }
        }
        set.into_iter().collect()
    };

    // Prefix → module name lookup.  Kept as a Vec so lookups are
    // stable-ordered and easy to iterate.
    let module_prefixes: Vec<(String, String)> = modules.iter()
        .map(|m| (format!("{}/{}/", normalized, m), m.clone()))
        .collect();

    // Classify a path: which module (if any) does it belong to?
    let module_of = |path: &str| -> Option<&str> {
        module_prefixes.iter()
            .find(|(p, _)| path.starts_with(p))
            .map(|(_, m)| m.as_str())
    };

    // Pull every structural edge for the repo in one query.  Empty prefix
    // → `LIKE '%'` matches every row.  For RWATP that's ~3,000 rows —
    // trivially small for in-memory aggregation.
    let all_edges = store.structural_edges_from_prefix("", repo_path)?;

    // Aggregation state.
    let mut cell_agg:     std::collections::HashMap<(String, String), CellAcc> = std::collections::HashMap::new();
    let mut external_agg: std::collections::HashMap<(String, String), ExtAcc>  = std::collections::HashMap::new();
    let mut platform_agg: std::collections::HashMap<(String, String), PlatAcc> = std::collections::HashMap::new();

    for e in &all_edges {
        let src_mod = match module_of(&e.source_file) { Some(m) => m.to_string(), None => continue };

        // Case A — external target.
        if e.target_file.starts_with("UNRESOLVED:external:") {
            let key = (src_mod, e.target_file.clone());
            let acc = external_agg.entry(key).or_default();
            acc.edge_count += 1;
            acc.distinct_source_files.insert(e.source_file.clone());
            continue;
        }

        // Case B — target belongs to a module.
        if let Some(tgt_mod) = module_of(&e.target_file) {
            if tgt_mod == src_mod {
                // Internal cohesion, not coupling.
                continue;
            }
            let key = (src_mod, tgt_mod.to_string());
            let acc = cell_agg.entry(key).or_default();
            acc.edge_count += 1;
            acc.distinct_sources.insert(e.source_file.clone());
            acc.distinct_targets.insert(e.target_file.clone());
            *acc.kind_counts.entry(e.kind.clone()).or_insert(0) += 1;
            continue;
        }

        // Case C — target is in the repo but outside any module.
        // Aggregate by the first path segment (e.g. `src/common`).
        let plat_target = first_two_segments(&e.target_file);
        if plat_target.is_empty() { continue; }
        let key = (src_mod, plat_target);
        let acc = platform_agg.entry(key).or_default();
        acc.edge_count += 1;
        acc.distinct_sources.insert(e.source_file.clone());
        acc.distinct_targets.insert(e.target_file.clone());
    }

    // Build the sparse cells list.
    let mut cells: Vec<ModuleCouplingCell> = cell_agg.into_iter()
        .map(|((src, tgt), acc)| {
            let mut kinds: Vec<ModuleCouplingKindBreakdown> = acc.kind_counts.into_iter()
                .map(|(kind, edge_count)| ModuleCouplingKindBreakdown { kind, edge_count })
                .collect();
            kinds.sort_by(|a, b| b.edge_count.cmp(&a.edge_count).then(a.kind.cmp(&b.kind)));
            ModuleCouplingCell {
                source_module:         src,
                target_module:         tgt,
                edge_count:            acc.edge_count,
                distinct_source_files: acc.distinct_sources.len(),
                distinct_target_files: acc.distinct_targets.len(),
                kinds,
            }
        })
        .collect();
    cells.sort_by(|a, b| b.edge_count.cmp(&a.edge_count)
        .then(a.source_module.cmp(&b.source_module))
        .then(a.target_module.cmp(&b.target_module)));

    // Derived fan-out / fan-in.
    let mut fan_out_map: std::collections::HashMap<String, (usize, std::collections::HashSet<String>)> = std::collections::HashMap::new();
    let mut fan_in_map:  std::collections::HashMap<String, (usize, std::collections::HashSet<String>)> = std::collections::HashMap::new();
    for cell in &cells {
        let e = fan_out_map.entry(cell.source_module.clone()).or_default();
        e.0 += cell.edge_count;
        e.1.insert(cell.target_module.clone());
        let e = fan_in_map.entry(cell.target_module.clone()).or_default();
        e.0 += cell.edge_count;
        e.1.insert(cell.source_module.clone());
    }
    let mut fan_out: Vec<ModuleFanIndicator> = fan_out_map.into_iter()
        .map(|(m, (edges, partners))| ModuleFanIndicator { module: m, edges, fan: partners.len() })
        .collect();
    fan_out.sort_by(|a, b| b.edges.cmp(&a.edges).then(a.module.cmp(&b.module)));
    let mut fan_in: Vec<ModuleFanIndicator> = fan_in_map.into_iter()
        .map(|(m, (edges, partners))| ModuleFanIndicator { module: m, edges, fan: partners.len() })
        .collect();
    fan_in.sort_by(|a, b| b.edges.cmp(&a.edges).then(a.module.cmp(&b.module)));

    let mut external_dependencies: Vec<ExternalDependencyRow> = external_agg.into_iter()
        .map(|((src, tgt), acc)| ExternalDependencyRow {
            source_module:         src,
            external_target:       tgt,
            edge_count:            acc.edge_count,
            distinct_source_files: acc.distinct_source_files.len(),
        })
        .collect();
    external_dependencies.sort_by(|a, b| b.edge_count.cmp(&a.edge_count)
        .then(a.source_module.cmp(&b.source_module))
        .then(a.external_target.cmp(&b.external_target)));

    let mut platform_usage: Vec<PlatformUsageRow> = platform_agg.into_iter()
        .map(|((src, tgt), acc)| PlatformUsageRow {
            source_module:         src,
            platform_target:       tgt,
            edge_count:            acc.edge_count,
            distinct_source_files: acc.distinct_sources.len(),
            distinct_target_files: acc.distinct_targets.len(),
        })
        .collect();
    platform_usage.sort_by(|a, b| b.edge_count.cmp(&a.edge_count)
        .then(a.source_module.cmp(&b.source_module))
        .then(a.platform_target.cmp(&b.platform_target)));

    Ok(ModuleCouplingReport {
        schema_version: MODULE_COUPLING_SCHEMA_VERSION,
        subject:        subject.to_string(),
        modules,
        cells,
        fan_out,
        fan_in,
        external_dependencies,
        platform_usage,
    })
}

#[derive(Default)]
struct CellAcc {
    edge_count:       usize,
    distinct_sources: std::collections::HashSet<String>,
    distinct_targets: std::collections::HashSet<String>,
    kind_counts:      std::collections::HashMap<String, usize>,
}

#[derive(Default)]
struct ExtAcc {
    edge_count:            usize,
    distinct_source_files: std::collections::HashSet<String>,
}

#[derive(Default)]
struct PlatAcc {
    edge_count:       usize,
    distinct_sources: std::collections::HashSet<String>,
    distinct_targets: std::collections::HashSet<String>,
}

// ─── Authors Aggregation (v0.8a — B4) ───────────────────────────────────────
//
// Aggregates `commits.author_name` + `commits.author_email` for a subject
// (repo root, directory, or file).  Purely a projection over existing
// evidence — no new schema, no new ontology.
//
// Repo-isolation invariant: every SQL path is scoped by
// `commits.repo_path = ?`.  See the storage docstrings on
// `authors_for_prefix`, `authors_for_file`, `authors_for_identity`.
//
// Subject-resolution rules (documented on `AuthorScope`):
//   - Empty subject or "." or "/"           → Prefix with empty pattern
//   - Directory (subject ends "/" or resolves to no file rows)
//                                           → Prefix with `subject/`
//   - File with materialised FileIdentity   → Identity (rename-safe)
//   - File without FileIdentity             → ExactFile
//   - Historical file path (was renamed away) → resolve current path
//     via `current_path_if_historical`, populate `redirect_note`, and
//     then apply the file-subject rules to the current path
//
// Callers may override the file-vs-directory heuristic with an explicit
// `subject_kind`; this is used by the CLI's `--kind` flag when the path
// doesn't exist on disk (e.g. querying a deleted file).

const AUTHORS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorsSubjectKind {
    /// Let `compute_authors` decide (repo root / directory / file) based
    /// on the shape of the subject and what Atlas has ingested.
    Auto,
    /// Force directory-prefix aggregation.  Trailing `/` is added if
    /// absent.  Empty subject is treated as the repo root.
    Directory,
    /// Force file-scoped aggregation.  Uses the identity chain when
    /// available, exact-file otherwise.
    File,
}

/// Assemble an `AuthorsReport` for `subject` in `repo_path`.
///
/// See the module-level notes on `AuthorScope` and the subject-resolution
/// rules for how the scope is chosen.
pub fn compute_authors(
    subject:      &str,
    subject_kind: AuthorsSubjectKind,
    repo_path:    &str,
    store:        &Store,
) -> Result<AuthorsReport> {
    // Normalise the input: trim whitespace only.  Preserve trailing `/`
    // because it disambiguates directory-vs-file when the path doesn't
    // exist on disk.  `.` and `./` and `/` collapse to the repo root.
    let raw = subject.trim();
    let is_repo_root = raw.is_empty() || raw == "." || raw == "./" || raw == "/";

    // Repo root — always Prefix scope with empty pattern.
    if is_repo_root {
        return build_report(
            subject,
            AuthorScope::Prefix,
            "whole repo (LIKE '%')".to_string(),
            store.authors_for_prefix("", repo_path)?,
            None,
        );
    }

    let trailing_slash = raw.ends_with('/');
    let normalized = raw.trim_end_matches('/').to_string();

    // Historical-path redirect — applies only when the caller wants file
    // resolution (or the path resolves to a file identity even when auto).
    // Directory subjects have no identity concept.
    let (working_path, redirect_note) = match subject_kind {
        AuthorsSubjectKind::Directory => (normalized.clone(), None),
        _ => match store.current_path_if_historical(&normalized, repo_path)? {
            Some(current) => {
                let identity_id = store
                    .resolve_path_to_identity(&current, repo_path)?
                    .unwrap_or(-1);
                (
                    current.clone(),
                    Some(HistoricalRedirect {
                        original_subject: normalized.clone(),
                        current_path:     current,
                        identity_id,
                    }),
                )
            }
            None => (normalized.clone(), None),
        },
    };

    // Force directory scope when requested, or when the caller added a
    // trailing slash.
    let force_dir = matches!(subject_kind, AuthorsSubjectKind::Directory) || trailing_slash;

    // Force file scope when requested (skip the auto directory-detection).
    let force_file = matches!(subject_kind, AuthorsSubjectKind::File);

    if force_dir {
        let pattern = format!("{}/", working_path);
        return build_report(
            subject,
            AuthorScope::Prefix,
            format!("prefix ('{}%')", pattern),
            store.authors_for_prefix(&pattern, repo_path)?,
            redirect_note,
        );
    }

    // Auto or forced-file: try file-scoped resolution.
    if let Some(identity_id) = store.resolve_path_to_identity(&working_path, repo_path)? {
        let n_paths = store.identity_path_observation_count(identity_id, repo_path)?;
        return build_report(
            subject,
            AuthorScope::Identity,
            format!("identity {} (spans {} path observation{})",
                identity_id,
                n_paths,
                if n_paths == 1 { "" } else { "s" },
            ),
            store.authors_for_identity(identity_id, repo_path)?,
            redirect_note,
        );
    }

    // Auto mode with no identity — decide file vs. directory by looking
    // for any file-path rows that start with `working_path/`.  If we find
    // any, treat as a directory prefix; otherwise fall back to exact-file.
    // Force-file skips this check and goes straight to exact-file scope.
    if !force_file {
        let subtree_pattern = format!("{}/", working_path);
        let subtree_rows = store.authors_for_prefix(&subtree_pattern, repo_path)?;
        if !subtree_rows.is_empty() {
            return build_report(
                subject,
                AuthorScope::Prefix,
                format!("prefix ('{}%')", subtree_pattern),
                subtree_rows,
                None, // directory subject → never a historical redirect
            );
        }
    }

    // Exact-file scope (either force_file or auto-with-no-subtree-rows).
    build_report(
        subject,
        AuthorScope::ExactFile,
        format!("exact file ('{}')", working_path),
        store.authors_for_file(&working_path, repo_path)?,
        redirect_note,
    )
}

/// Convert a repo-scoped Vec<AuthorAggregateRow> into an AuthorsReport.
/// Rows arrive already sorted (commits DESC, name ASC) from the SQL layer.
fn build_report(
    subject:      &str,
    scope:        AuthorScope,
    scope_detail: String,
    rows:         Vec<AuthorAggregateRow>,
    redirect:     Option<HistoricalRedirect>,
) -> Result<AuthorsReport> {
    let total_commits = rows.iter().map(|r| r.commit_count).sum();
    let total_authors = rows.len();
    let authors = rows.into_iter()
        .map(|r| AuthorAggregate {
            author_name:  r.author_name,
            author_email: r.author_email,
            commit_count: r.commit_count,
            first_touch:  r.first_touch,
            last_touch:   r.last_touch,
        })
        .collect();
    Ok(AuthorsReport {
        schema_version: AUTHORS_SCHEMA_VERSION,
        subject:        subject.to_string(),
        scope,
        scope_detail,
        authors,
        total_commits,
        total_authors,
        redirect_note:  redirect,
    })
}

/// Return the first TWO path segments of `path` joined by `/`, or the
/// first segment if the path has no second.  Empty string if the path
/// has no segments (shouldn't happen for a valid file_path).
///
/// Used to aggregate platform-layer usage: `src/common/enum/foo.ts` →
/// `src/common`, so hundreds of individual imports collapse into a
/// small set of platform destinations.
fn first_two_segments(path: &str) -> String {
    let mut it = path.split('/');
    let first = it.next().unwrap_or("");
    if first.is_empty() { return String::new(); }
    match it.next() {
        Some(second) if !second.is_empty() => format!("{}/{}", first, second),
        _ => first.to_string(),
    }
}

// ─── Inspect (v0.7d) ─────────────────────────────────────────────────────────
//
// Attaches existing Atlas evidence to a single spatial subject (file or
// directory subtree).  Read-only, transient — no schema changes, no
// persistence.  See docs/decisions/2026-08-08-atlas-inspect.md.

const INSPECT_SCHEMA_VERSION: u32 = 1;

/// Assemble an `InspectionDocument` for `subject` in `repo_path`.
///
/// `subject` is interpreted repo-relative.  Leading and trailing slashes are
/// stripped.  The kind is auto-detected from disk: a directory that exists
/// becomes `Directory`; a file that exists becomes `File`; a path that does
/// not exist defaults to `Directory` (aggregation-as-prefix, with
/// `exists_on_disk = false` making the situation explicit to the consumer).
pub fn inspect(subject: &str, repo_path: &str, store: &Store) -> Result<InspectionDocument> {
    let requested = normalize_inspect_subject(subject);

    // File-subject identity redirect (Item 1).
    //
    // If the caller supplied a historical file path that FileIdentity knows
    // has moved, transparently query under the current path AND record the
    // redirect in the output so the caller can see both addresses.
    //
    // Directory subjects deliberately do NOT redirect: Atlas has no
    // directory identity concept.  Subtree aggregation uses the current
    // tree's path prefix — files that used to live under this subtree but
    // have been moved out are NOT included, and files that were once
    // outside and have been moved in ARE included.  This subtree semantic
    // is documented on the `structural_depends_on` field of
    // `InspectionDocument` and on `commits_under_prefix` in storage.
    let (relative_path, historical_redirect) = {
        // A historical file path no longer exists on disk (it was renamed
        // away), so we cannot use `.is_file()` as the trigger.  Instead ask
        // FileIdentity directly: does this path resolve to an identity whose
        // current occupant lives at a different path?  Only files have
        // identities in Atlas, so a positive answer implies "file subject".
        match store.current_path_if_historical(&requested, repo_path)? {
            Some(current) => {
                let identity_id = store
                    .resolve_path_to_identity(&requested, repo_path)?
                    .unwrap_or(0);
                let redirect = atlas_ir::HistoricalRedirect {
                    original_subject: requested.clone(),
                    current_path:     current.clone(),
                    identity_id,
                };
                (current, Some(redirect))
            }
            None => (requested, None),
        }
    };

    let abs = if relative_path.is_empty() {
        Path::new(repo_path).to_path_buf()
    } else {
        Path::new(repo_path).join(&relative_path)
    };
    let exists_on_disk = abs.exists();
    let kind = if exists_on_disk && abs.is_file() {
        InspectionSubjectKind::File
    } else {
        InspectionSubjectKind::Directory
    };

    // Prefix used for every subtree query.
    //   Directory subject → path ending in '/' (or "" for repo root).
    //   File subject      → exact file path, no trailing slash.
    let prefix = match kind {
        InspectionSubjectKind::File => relative_path.clone(),
        InspectionSubjectKind::Directory => {
            if relative_path.is_empty() { String::new() } else { format!("{}/", relative_path) }
        }
    };

    // ── Coverage (measured, not asserted) ────────────────────────────────
    let commit_count = store.commit_count(repo_path).unwrap_or(0);
    let pr_count     = store.pr_count(repo_path).unwrap_or(0);
    let issue_count  = store.issue_count(repo_path).unwrap_or(0);
    let edge_count   = store.structural_edge_count(repo_path).unwrap_or(0);
    let doc_count    = store.document_count(repo_path).unwrap_or(0);
    let profile_seen = Path::new(repo_path).join("package.json").exists()
        || Path::new(repo_path).join("Cargo.toml").exists();
    let coverage = InspectionCoverage {
        git_history:      commit_count > 0,
        github_prs:       pr_count > 0,
        github_issues:    issue_count > 0,
        structural_edges: edge_count > 0,
        documentation:    doc_count > 0,
        profile_claims:   profile_seen,
        working_tree:     exists_on_disk,
    };

    // ── File-only fields (delegate to build_context) ─────────────────────
    let mut role: Option<ArtifactRole> = None;
    let mut identity: Option<FileIdentity> = None;
    let mut coupling: Vec<CouplingEntry> = Vec::new();

    let mut recent_activity: Vec<CommitSummary> = Vec::new();
    let mut touch_count: i64 = 0;
    let mut related_history = RelatedHistory { pull_requests: vec![], issues: vec![] };
    let mut children: Vec<InspectionChild> = Vec::new();
    let mut hot_files_within: Vec<CouplingEntry> = Vec::new();

    match kind {
        InspectionSubjectKind::File => {
            role = Some(classify_artifact_role(&relative_path));
            if let Ok(ctx) = build_context(&relative_path, repo_path, store) {
                // Reuse `ctx.identity.touch_count`, which is identity-scoped
                // when a chain exists (spans pre- and post-rename commits) and
                // falls back to path-scoped otherwise.  Prior code called
                // `store.touch_count(current_path)` — path-scoped only — which
                // silently dropped the identity's earlier commits.
                touch_count     = ctx.identity.touch_count;
                identity        = Some(ctx.identity);
                coupling        = ctx.coupling;
                recent_activity = ctx.recent_activity;
                related_history = ctx.related_history;
            }
        }
        InspectionSubjectKind::Directory => {
            children = list_inspect_children(&abs);

            let commits = store.commits_under_prefix(&prefix, repo_path).unwrap_or_default();
            touch_count = commits.len() as i64;
            recent_activity = commits.iter().take(10).map(|c| CommitSummary {
                short_hash: c.short_hash.clone(),
                message:    c.message.clone(),
                author:     c.author_name.clone(),
                timestamp:  c.timestamp,
            }).collect();

            let prs = store.prs_under_prefix(&prefix, repo_path).unwrap_or_default();
            let issues = store.issues_under_prefix(&prefix, repo_path).unwrap_or_default();
            related_history = RelatedHistory {
                pull_requests: prs.into_iter().map(|p| PrSummary {
                    number:           p.number,
                    title:            p.title,
                    state:            p.state,
                    merge_commit_sha: p.merge_commit_sha,
                    linked_issues:    store
                        .issue_numbers_for_pr(p.number, repo_path)
                        .unwrap_or_default(),
                }).collect(),
                issues: issues.into_iter().map(|i| IssueSummary {
                    number: i.number,
                    title:  i.title,
                    state:  i.state,
                }).collect(),
            };

            // Hot files within — filter existing hot_files rather than a new SQL query.
            let all_hot = store.hot_files(repo_path, 1000).unwrap_or_default();
            hot_files_within = all_hot.into_iter()
                .filter(|hf| inspect_path_is_inside(&hf.file_path, &prefix, kind))
                .take(10)
                .map(|hf| CouplingEntry { file_path: hf.file_path, change_count: hf.touch_count })
                .collect();
        }
    }

    // ── Structural edges — partitioned into internal / depends_on / used_by ──
    let (structural_depends_on, structural_used_by, structural_internal) = {
        let out_rows = store.structural_edges_from_prefix(&prefix, repo_path).unwrap_or_default();
        let in_rows  = store.structural_edges_to_prefix(&prefix, repo_path).unwrap_or_default();

        let mut internal:    Vec<InspectionEdge> = Vec::new();
        let mut depends_on:  Vec<InspectionEdge> = Vec::new();
        let mut used_by:     Vec<InspectionEdge> = Vec::new();

        for e in out_rows {
            let target_inside = inspect_path_is_inside(&e.target_file, &prefix, kind);
            let edge = InspectionEdge {
                source_file:   e.source_file,
                target_file:   e.target_file,
                kind:          e.kind,
                source_symbol: e.source_symbol,
                target_symbol: e.target_symbol,
            };
            if target_inside { internal.push(edge); } else { depends_on.push(edge); }
        }
        for e in in_rows {
            // Skip edges whose source is also inside — those were already
            // recorded as `internal` via the from-prefix query.
            if inspect_path_is_inside(&e.source_file, &prefix, kind) { continue; }
            used_by.push(InspectionEdge {
                source_file:   e.source_file,
                target_file:   e.target_file,
                kind:          e.kind,
                source_symbol: e.source_symbol,
                target_symbol: e.target_symbol,
            });
        }
        (depends_on, used_by, internal)
    };

    // ── Documents inside the subject ────────────────────────────────────
    let documents = store
        .documents_under_prefix(&prefix, repo_path)
        .unwrap_or_default()
        .into_iter()
        .map(|(fp, dt, ti)| InspectionDocumentRef { file_path: fp, doc_type: dt, title: ti })
        .collect();

    // ── Profile claims — ambient plus subject-matching Module ────────────
    let all_claims = inspect_repository(repo_path).unwrap_or_default();
    let subject_module = subject_module_name(&relative_path, kind);
    let profile_claims: Vec<ProfileClaim> = all_claims.into_iter()
        .filter(|c| {
            matches!(
                c.kind,
                ProfileClaimKind::Runtime | ProfileClaimKind::Language | ProfileClaimKind::PackageManager
            ) || (
                matches!(c.kind, ProfileClaimKind::Module)
                && subject_module.as_ref().map(|n| &c.value == n).unwrap_or(false)
            )
        })
        .collect();

    Ok(InspectionDocument {
        schema_version: INSPECT_SCHEMA_VERSION,
        subject: subject.to_string(),
        relative_path,
        kind,
        exists_on_disk,
        role,
        identity,
        coupling,
        children,
        hot_files_within,
        recent_activity,
        touch_count,
        related_history,
        structural_depends_on,
        structural_used_by,
        structural_internal,
        documents,
        profile_claims,
        historical_redirect,
        coverage,
    })
}

/// Normalise a caller-supplied subject to a repo-relative path with no
/// leading or trailing slash.  Empty string means "the repository root".
fn normalize_inspect_subject(subject: &str) -> String {
    let trimmed = subject.trim();
    let stripped = trimmed
        .trim_start_matches('/')
        .trim_end_matches('/');
    // Preserve normal path separators; do not attempt to resolve `..`.
    stripped.to_string()
}

/// True iff `path` lies inside the subject subtree.
///   File subject:  exact equality with prefix.
///   Directory subject: repository root (empty prefix) or path starts with `prefix`.
fn inspect_path_is_inside(path: &str, prefix: &str, kind: InspectionSubjectKind) -> bool {
    match kind {
        InspectionSubjectKind::File => path == prefix,
        InspectionSubjectKind::Directory => prefix.is_empty() || path.starts_with(prefix),
    }
}

fn list_inspect_children(dir: &Path) -> Vec<InspectionChild> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut sorted: Vec<_> = entries.flatten().collect();
    sorted.sort_by_key(|e| e.file_name());
    let mut out = Vec::with_capacity(sorted.len());
    for entry in sorted {
        let Ok(ft) = entry.file_type() else { continue };
        // Skip `.git` inside inspect output for the same reason `walk_tree` does.
        let name = entry.file_name().to_string_lossy().into_owned();
        if ft.is_dir() && name == ".git" { continue; }
        let kind = if ft.is_dir() {
            TreeNodeKind::Directory
        } else if ft.is_file() {
            TreeNodeKind::File
        } else {
            continue; // symlinks and others skipped
        };
        out.push(InspectionChild { name, kind });
    }
    out
}

/// Extract the module name for `Module` ProfileClaim matching.  Only the
/// first path segment under `src/` is considered — the inspector only
/// records top-level `src/` subdirectories as `Module` claims, so a subject
/// like `src/modules/identity/` maps to module name `modules`.
fn subject_module_name(relative_path: &str, kind: InspectionSubjectKind) -> Option<String> {
    if !matches!(kind, InspectionSubjectKind::Directory) { return None; }
    let rest = relative_path.strip_prefix("src/")?;
    let first = rest.split('/').next()?;
    if first.is_empty() { None } else { Some(first.to_string()) }
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
    let doc_count   = store.document_count(repo_path)?;

    let coverage = SearchCoverage {
        file_paths:            true,
        commit_history:        repo_commits > 0,
        pull_requests:         pr_count > 0,
        issues:                issue_count > 0,
        engineering_decisions: doc_count > 0,
        source_code:           false,
        working_tree:          false,
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
        "decision_body"   => (MatchSource::DecisionBody,  EvidenceType::Engineering),
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

/// PageRank over a small directed graph (≤100 nodes, investigation subgraph).
///
/// `edges`: (from_index, to_index) pairs — an edge from A to B means A imports/calls B.
/// For investigation ranking, high PageRank ≈ "many important files depend on this file".
fn pagerank(nodes: &[&str], edges: &[(usize, usize)], iterations: usize, damping: f32) -> Vec<f32> {
    let n = nodes.len();
    if n == 0 { return vec![]; }
    if n == 1 { return vec![1.0]; }

    let mut scores = vec![1.0_f32 / n as f32; n];

    // Out-degree per node (number of outgoing edges).
    let mut out_deg = vec![0usize; n];
    for &(from, _) in edges { out_deg[from] += 1; }

    let teleport = (1.0 - damping) / n as f32;

    for _ in 0..iterations {
        let mut new_scores = vec![teleport; n];
        for &(from, to) in edges {
            if out_deg[from] > 0 {
                new_scores[to] += damping * scores[from] / out_deg[from] as f32;
            }
        }
        // Dangling nodes (out_deg == 0) distribute their rank uniformly.
        let dangling_sum: f32 = (0..n)
            .filter(|&i| out_deg[i] == 0)
            .map(|i| scores[i])
            .sum();
        if dangling_sum > 0.0 {
            let spread = damping * dangling_sum / n as f32;
            for s in &mut new_scores { *s += spread; }
        }
        scores = new_scores;
    }

    scores
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

// ─── Campaign Engine ──────────────────────────────────────────────────────────

/// One gap observation extracted from a single benchmark file's frontmatter.
#[derive(Clone)]
struct GapObs {
    gap_id:         String,
    classification: String,
    description:    String,
    implementation: String,
    success:        String,
    threshold:      u32,
    benchmark_id:   String,
    repository:     String,
}

/// Parse YAML frontmatter (`---` … `---`) into a flat key→value map.
/// Handles quoted values; skips indented lines (nested YAML).
fn parse_frontmatter(content: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") { return map; }
    let after = &trimmed[3..];
    let end = match after.find("---") {
        Some(e) => e,
        None    => return map,
    };
    for line in after[..end].lines() {
        if line.starts_with(' ') || line.starts_with('\t') { continue; }
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_string();
            let raw = line[colon + 1..].trim();
            let value = if raw.len() >= 2
                && ((raw.starts_with('"') && raw.ends_with('"'))
                    || (raw.starts_with('\'') && raw.ends_with('\'')))
            {
                raw[1..raw.len() - 1].to_string()
            } else {
                raw.to_string()
            };
            if !key.is_empty() {
                map.insert(key, value);
            }
        }
    }
    map
}

/// Extract zero or more gap observations from a benchmark's parsed frontmatter.
///
/// Supports two layouts:
///   Single-gap:  `gap_id`, `gap_classification`, `gap_description`, …
///   Multi-gap:   `gap_0_id`, `gap_0_classification`, … `gap_1_id`, …
fn extract_gap_obs(
    fm: &std::collections::HashMap<String, String>,
    benchmark_id: &str,
    repository: &str,
) -> Vec<GapObs> {
    let mut obs = Vec::new();

    // Single-gap format
    if let Some(id) = fm.get("gap_id").filter(|s| !s.is_empty()) {
        obs.push(GapObs {
            gap_id:         id.clone(),
            classification: fm.get("gap_classification").cloned().unwrap_or_default(),
            description:    fm.get("gap_description").cloned().unwrap_or_default(),
            implementation: fm.get("gap_implementation").cloned().unwrap_or_default(),
            success:        fm.get("gap_success").cloned().unwrap_or_default(),
            threshold:      fm.get("gap_threshold").and_then(|s| s.parse().ok()).unwrap_or(3),
            benchmark_id:   benchmark_id.to_string(),
            repository:     repository.to_string(),
        });
    }

    // Multi-gap format: gap_0_id, gap_1_id, …
    for i in 0..10usize {
        let id_key = format!("gap_{i}_id");
        match fm.get(&id_key).filter(|s| !s.is_empty()) {
            Some(id) => obs.push(GapObs {
                gap_id:         id.clone(),
                classification: fm.get(&format!("gap_{i}_classification")).cloned().unwrap_or_default(),
                description:    fm.get(&format!("gap_{i}_description")).cloned().unwrap_or_default(),
                implementation: fm.get(&format!("gap_{i}_implementation")).cloned().unwrap_or_default(),
                success:        fm.get(&format!("gap_{i}_success")).cloned().unwrap_or_default(),
                threshold:      fm.get(&format!("gap_{i}_threshold")).and_then(|s| s.parse().ok()).unwrap_or(3),
                benchmark_id:   benchmark_id.to_string(),
                repository:     repository.to_string(),
            }),
            None => break,
        }
    }

    obs
}

/// Determine the next campaign Atlas is ready to execute.
///
/// Derives gap candidates entirely from benchmark frontmatter (`docs/benchmarks/*.md`)
/// and decision record frontmatter (`docs/decisions/*.md`). No manually maintained
/// registry — the benchmark files ARE the evidence.
pub fn campaign_next(repo_path: &str) -> Result<CampaignBrief> {
    // ── 1. Collect gap observations from all formal benchmarks ────────────────
    let mut observations: Vec<GapObs> = Vec::new();
    let benchmark_dir = Path::new(repo_path).join("docs/benchmarks");

    if let Ok(entries) = std::fs::read_dir(&benchmark_dir) {
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|e| e == "md").unwrap_or(false))
            .filter(|p| p.file_name().map(|n| n != "TEMPLATE.md").unwrap_or(false))
            .collect();
        paths.sort(); // date-prefixed filenames → chronological order

        for path in paths {
            let content = std::fs::read_to_string(&path)?;
            let fm = parse_frontmatter(&content);
            let benchmark_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            let repository = fm.get("repository").cloned().unwrap_or_default();
            observations.extend(extract_gap_obs(&fm, &benchmark_id, &repository));
        }
    }

    // ── 2. Collect implemented gap IDs from decision records ──────────────────
    let mut implemented: HashSet<String> = HashSet::new();
    let decision_dir = Path::new(repo_path).join("docs/decisions");

    if let Ok(entries) = std::fs::read_dir(&decision_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                let content = std::fs::read_to_string(&path)?;
                let fm = parse_frontmatter(&content);
                if fm.get("status").map(|s| s == "Implemented").unwrap_or(false) {
                    if let Some(gap_id) = fm.get("implements_gap").filter(|s| !s.is_empty()) {
                        implemented.insert(gap_id.clone());
                    }
                }
            }
        }
    }

    // ── 3. Aggregate observations → one GapEntry per gap ID ──────────────────
    // IndexMap preserves insertion order (chronological by first observation).
    let mut defs:  indexmap::IndexMap<String, GapObs>            = indexmap::IndexMap::new();
    let mut repos: indexmap::IndexMap<String, HashSet<String>>   = indexmap::IndexMap::new();
    let mut bms:   indexmap::IndexMap<String, Vec<String>>       = indexmap::IndexMap::new();

    for obs in observations {
        defs.entry(obs.gap_id.clone()).or_insert_with(|| obs.clone());
        repos.entry(obs.gap_id.clone()).or_default().insert(obs.repository.clone());
        bms.entry(obs.gap_id.clone()).or_default().push(obs.benchmark_id.clone());
    }

    let mut gaps: Vec<GapEntry> = defs
        .into_iter()
        .map(|(id, def)| {
            let n = bms.get(&id).map(|v| v.len()).unwrap_or(0) as u32;
            let mut repositories: Vec<String> = repos
                .get(&id)
                .map(|s| { let mut v: Vec<_> = s.iter().cloned().collect(); v.sort(); v })
                .unwrap_or_default();
            repositories.retain(|r| !r.is_empty());
            let benchmarks = bms.get(&id).cloned().unwrap_or_default();

            let status = if implemented.contains(&id) {
                "implemented"
            } else if n >= def.threshold {
                "earned"
            } else if n >= 2 {
                "candidate"
            } else {
                "watch"
            }
            .to_string();

            // Prettify id as display name: "cross-repo-contracts" → "Cross Repo Contracts"
            let name = id
                .split('-')
                .map(|w| {
                    let mut chars = w.chars();
                    match chars.next() {
                        None    => String::new(),
                        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");

            GapEntry {
                id,
                name,
                classification:          def.classification,
                description:             def.description,
                n,
                threshold:               def.threshold,
                status,
                repositories,
                benchmarks,
                suggested_implementation: def.implementation,
                success_criterion:        def.success,
            }
        })
        .collect();

    // Sort: non-implemented first (by N desc), implemented last
    gaps.sort_by(|a, b| {
        a.is_implemented()
            .cmp(&b.is_implemented())
            .then(b.n.cmp(&a.n))
            .then(a.id.cmp(&b.id))
    });

    // ── 4. Determine outcome ──────────────────────────────────────────────────
    let top_earned = gaps
        .iter()
        .filter(|g| !g.is_implemented())
        .find(|g| g.is_earned())
        .cloned();

    let outcome = match top_earned {
        Some(gap) => CampaignOutcome::Ready { gap },
        None => {
            let candidates = gaps.iter().filter(|g| !g.is_implemented()).cloned().collect();
            CampaignOutcome::NoneEarned { candidates }
        }
    };

    Ok(CampaignBrief { outcome, all_gaps: gaps })
}

// ── Issue-driven implementation planning ──────────────────────────────────────

/// Context assembled from a GitHub issue + Atlas investigation.
/// Returned from plan_from_issue; consumed by the CLI's AI synthesis layer.
pub struct IssuePlanContext {
    pub issue_number: i64,
    pub title:        String,
    pub body:         String,
    pub anchors_used: Vec<String>,
    pub doc:          InvestigationDocument,
}

/// Extract investigation anchors from an issue title and body.
///
/// Strategy:
///   - Issue title words are primary (most signal-dense).
///   - Technical words from the body (length >= 5, not stopwords) supplement.
///   - Deduped, max 8 total to keep the investigation focused.
pub fn extract_issue_anchors(title: &str, body: &str) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut anchors: Vec<String> = Vec::new();

    let add = |word: &str, seen: &mut std::collections::HashSet<String>, anchors: &mut Vec<String>| {
        if anchors.len() >= 12 { return; }
        let w = word.to_lowercase();
        if w.len() < 3 { return; }
        if ISSUE_ANCHOR_STOPWORDS.contains(&w.as_str()) { return; }
        if seen.insert(w.clone()) {
            anchors.push(w);
        }
    };

    // Title first — split on non-alphanumeric
    for word in title.split(|c: char| !c.is_alphanumeric()) {
        add(word, &mut seen, &mut anchors);
    }

    // Body supplement — only technical words (length >= 5)
    if anchors.len() < 12 {
        for word in body.split(|c: char| !c.is_alphanumeric()) {
            if word.len() >= 5 {
                add(word, &mut seen, &mut anchors);
            }
            if anchors.len() >= 12 { break; }
        }
    }

    anchors
}

/// Look up an issue by number, run an investigation anchored on the issue's
/// vocabulary, and return the combined context for AI synthesis.
///
/// Returns None when the issue is not in the DB (caller should try GitHub fallback).
pub fn plan_from_issue(
    issue_number: i64,
    repo_path:    &str,
    store:        &Store,
) -> Result<Option<IssuePlanContext>> {
    let Some((title, body)) = store.get_issue(issue_number, repo_path)? else {
        return Ok(None);
    };

    let anchors = extract_issue_anchors(&title, &body);
    let anchor_refs: Vec<&str> = anchors.iter().map(String::as_str).collect();
    let doc = investigate(&anchor_refs, repo_path, store)?;

    Ok(Some(IssuePlanContext {
        issue_number,
        title,
        body,
        anchors_used: anchors,
        doc,
    }))
}
