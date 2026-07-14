use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id:    String,
    pub kind:  EntityKind,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Commit,
    File,
    PullRequest,
    Issue,
    Author,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub from_id: String,
    pub to_id:   String,
    pub kind:    RelationshipKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipKind {
    Modifies,   // commit → file
    Merges,     // pr    → commit
    Closes,     // pr    → issue
    AuthoredBy, // commit → author
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub entity_id: String,
    pub source:    EvidenceSource,
    pub raw:       String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    Git,
    GitHub,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub path:       String,
    pub name:       String,
    pub remote_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    pub hash:          String,
    pub short_hash:    String,
    pub message:       String,
    pub author_name:   String,
    pub author_email:  String,
    pub timestamp:     DateTime<Utc>,
    pub files_changed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct File {
    pub path:      String,
    pub extension: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number:           i64,
    pub title:            String,
    pub state:            String,
    pub body:             Option<String>,
    pub author:           String,
    pub merge_commit_sha: Option<String>,
    pub created_at:       Option<DateTime<Utc>>,
    pub merged_at:        Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub number:     i64,
    pub title:      String,
    pub state:      String,
    pub body:       Option<String>,
    pub author:     String,
    pub created_at: Option<DateTime<Utc>>,
}

// ─── Rename Evidence ─────────────────────────────────────────────────────────
//
// Raw evidence that Git detected a rename between two paths in a specific commit.
// This is evidence, not fact: Git computes similarity heuristically, and the
// similarity_score reflects one detection mechanism, not Atlas-level identity
// confidence.  Do not promote this score to a user-visible confidence percentage.

#[derive(Debug, Clone, PartialEq)]
pub struct RenameEvidence {
    /// Full commit hash of the commit in which the rename occurred.
    pub commit_hash:      String,
    /// The path before the rename.
    pub old_path:         String,
    /// The path after the rename.
    pub new_path:         String,
    /// Git's internal similarity score (0–100).  100 = byte-identical content.
    pub similarity_score: u8,
    /// Which detection mechanism produced this record (always "git-rename" for now).
    pub detection_source: String,
}

// ─── Structural Observation (v0.5b) ──────────────────────────────────────────

/// Which structural extractor produced this observation.
/// Named strings so new extractors can be added without breaking storage.
/// Current values: "typescript-es-import", "typescript-static-call",
///                 "typescript-mongoose-ref".
pub type ExtractorId = String;

/// The kind of direct structural relationship observed between two artifacts.
/// Conservative first-pass set: only what the rwatp-core ground-truth trace
/// confirmed is observable with static text analysis of this codebase.
/// READS_MODEL / WRITES_MODEL deliberately deferred — see v0.5b design notes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StructuralEdgeKind {
    /// One file imports symbols or has a side-effect dependency on another file.
    Imports,
    /// A static method on an imported class is called directly.
    CallsStatic,
    /// An instance method is called on a lowercase-imported singleton/service.
    CallsInstance,
    /// A Mongoose-style query method is called on an imported model class.
    ReferencesModel,
}

/// Provenance of a single structural observation — where and how it was seen.
/// Preserved at the record level so multiple evidence sources for the same edge
/// can be accumulated and distinguished later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralEvidence {
    pub source_file: String,
    pub line:        Option<u32>,
    pub snippet:     String,
    pub extractor:   ExtractorId,
}

/// A single observed structural relationship between two artifacts.
/// `target_file` may be prefixed with "UNRESOLVED:" when the import path
/// could not be resolved to a known file — this is explicit, not silent loss.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralEdge {
    pub source_file:   String,
    pub source_symbol: Option<String>,
    pub target_file:   String,
    pub target_symbol: Option<String>,
    pub kind:          StructuralEdgeKind,
    pub evidence:      StructuralEvidence,
}

// ─── Search / Anchor Retrieval (v0.5a) ───────────────────────────────────────

/// The epistemic class of a match — how it was produced and how certain it is.
/// These must stay distinct in all output: mixing planes silently is the
/// same error that motivated the v0.4.1 identity-consistency fixes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceType {
    /// Directly observable from source structure (file paths, code).
    Observed,
    /// Asserted in a commit, PR, issue, or documentation text.
    Documentary,
    /// Captured in a decision record or ADR — engineering rationale committed to the repo.
    Engineering,
    /// Inferred from commit co-change or historical coupling patterns.
    Historical,
}

/// Where in the knowledge corpus the match was found.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MatchSource {
    FilePath,
    CommitMessage,
    PrTitle,
    PrBody,
    IssueTitle,
    IssueBody,
    DecisionBody,
}

/// A single match between one anchor term and one location in the corpus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorMatch {
    /// The anchor term that produced this match.
    pub anchor:        String,
    /// Where the match was found.
    pub source:        MatchSource,
    /// Stable identifier for the matched record (file path, commit hash, PR/issue number).
    pub source_id:     String,
    /// Short text window around the match (or the full field when short).
    pub snippet:       String,
    /// Epistemic class of this evidence.
    pub evidence_type: EvidenceType,
}

/// Which corpus sections were available and searched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchCoverage {
    pub file_paths:            bool,
    pub commit_history:        bool,
    pub pull_requests:         bool,
    pub issues:                bool,
    pub engineering_decisions: bool,
    pub source_code:           bool,  // always false until v0.5b
    pub working_tree:          bool,  // always false until ingested
}

/// The assembled output of an anchor-retrieval search.
/// schema_version increments when the JSON shape changes incompatibly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchDocument {
    pub schema_version: u32,
    pub anchors:        Vec<String>,
    pub matches:        Vec<AnchorMatch>,
    pub coverage:       SearchCoverage,
}

// ─── Investigation Document (v0.5c) ──────────────────────────────────────────

/// Deterministic, path-based classification of an artifact's role.
/// Assigned by `classify_artifact_role()` using path heuristics only.
/// Does not represent semantic meaning — a file can have any role in reality;
/// this is evidence for presentation layering, not authoritative ontology.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRole {
    /// Source file that is not classified as any supporting role.
    ProductionSource,
    Test,
    Migration,
    Seeder,
    /// A utility or maintenance script (scripts/ prefix, not migration/seeder).
    Script,
    /// Sample code or frontend usage examples that are not production artifacts.
    Example,
    Schema,
    Validation,
    Permission,
    Documentation,
    Generated,
    Unknown,
}

/// Why a given file was included in an investigation.
/// Every artifact must carry provenance — the ranking layer later can use this.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum CandidateReason {
    /// A search anchor matched this file's path, a commit touching it, or
    /// a PR/issue linked to it.
    AnchorMatch {
        anchor: String,
        /// e.g. "file_path", "commit_message", "pr_title", "pr_body"
        via: String,
    },
    /// This file is 1 structural hop from a seed candidate.
    StructuralNeighbor {
        from_file: String,
        /// "imports", "calls_static", "references_model"
        kind: String,
        /// "outgoing" (from_file → this) or "incoming" (this → from_file)
        direction: String,
    },
}

/// A single artifact under investigation, with the reasons it was included.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateArtifact {
    pub file:    String,
    pub role:    ArtifactRole,
    pub reasons: Vec<CandidateReason>,
}

/// Compact representation of a structural edge for investigation output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralEdgeSummary {
    pub file:   String,
    pub kind:   String,
    pub symbol: Option<String>,
}

/// All observed structural relationships for one candidate file,
/// scoped to other candidates in this investigation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralObservation {
    pub file:     String,
    pub outgoing: Vec<StructuralEdgeSummary>,
    pub incoming: Vec<StructuralEdgeSummary>,
}

/// A decision record or ADR whose body matched one or more anchor terms.
/// Surfaces engineering rationale alongside code candidates in investigations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedDecision {
    pub title:   String,
    pub path:    String,
    pub snippet: String,
}

/// A PR or issue that matched one or more anchor terms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentaryEvidence {
    /// "pr" or "issue"
    pub kind:            String,
    pub number:          i64,
    pub title:           String,
    pub matched_anchors: Vec<String>,
    pub snippets:        Vec<String>,
}

/// Historical summary for a candidate file within this investigation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalEntry {
    pub file:                   String,
    pub touch_count:            i64,
    /// Other candidate files that co-changed with this one (threshold ≥ 2).
    pub co_changed_candidates:  Vec<String>,
}

/// A vocabulary bridge found by searching the documentary corpus for an anchor
/// term that did not match any file paths directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptExpansion {
    /// The original anchor term that had no direct file-path match.
    pub original_term: String,
    /// Which documentary item provided the bridge context (e.g. "PR #47", "Issue #55").
    pub bridge_source: String,
    /// Short text window around the anchor in the bridge document.
    pub bridge_snippet: String,
    /// Repository-confirmed vocabulary terms extracted from the bridge context.
    pub verified_expansions: Vec<VerifiedExpansion>,
}

/// A candidate expansion term that was both extracted from documentary evidence
/// and confirmed to exist in at least one repository file path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedExpansion {
    pub term:        String,
    /// File path that confirmed this term exists in the repository vocabulary.
    pub verified_in: String,
}

/// A candidate that Atlas can observe but cannot connect structurally
/// to the rest of the investigation.  The absence of an observed edge
/// is a fact, not an inference — coverage boundaries are always stated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedConnection {
    pub subject:                 String,
    pub documentary_indication:  Option<String>,
    pub observation:             String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationCoverage {
    pub git_history:    bool,
    pub github_prs:     bool,
    pub github_issues:  bool,
    pub file_paths:     bool,
    pub es_imports:     bool,
    pub static_calls:   bool,
    pub model_refs:     bool,
}

/// The assembled output of an anchor-driven investigation.
/// Combines anchor retrieval, structural observation, and historical/documentary
/// evidence into a single provenance-preserving document.
/// schema_version increments when the JSON shape changes incompatibly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationDocument {
    pub schema_version:       u32,
    /// Original anchors as supplied by the user.
    pub anchors:              Vec<String>,
    /// Effective anchors after concept resolution (original + verified expansions).
    /// Equals `anchors` when concept resolution found no expansions.
    pub effective_anchors:    Vec<String>,
    /// Documentary vocabulary bridges found for anchors with no direct file-path match.
    pub concept_expansions:   Vec<ConceptExpansion>,
    /// ProductionSource artifacts — core implementation neighborhood.
    pub core_candidates:      Vec<CandidateArtifact>,
    /// Non-ProductionSource artifacts — tests, migrations, schemas, etc.
    pub supporting_artifacts: Vec<CandidateArtifact>,
    pub observed_structure:   Vec<StructuralObservation>,
    pub documentary:          Vec<DocumentaryEvidence>,
    pub historical:           Vec<HistoricalEntry>,
    pub unresolved:           Vec<UnresolvedConnection>,
    /// Decision records and ADRs whose bodies matched the investigation anchors.
    /// Engineering rationale that explains why the observed code is the way it is.
    pub related_decisions:    Vec<RelatedDecision>,
    pub coverage:             InvestigationCoverage,
}

// ─── Context Document ─────────────────────────────────────────────────────────

/// The assembled, typed output of the context engine.
/// Single unit passed to CLI, JSON, AI, or future consumers — never raw SQL rows.
#[derive(Debug, Clone, Serialize)]
pub struct ContextDocument {
    /// Incremented when the JSON shape changes incompatibly.
    /// Consumers should check this before deserializing.
    pub schema_version:  u32,
    pub subject:         String,
    pub identity:        FileIdentity,
    pub recent_activity: Vec<CommitSummary>,
    pub related_history: RelatedHistory,
    pub coupling:        Vec<CouplingEntry>,
    pub documentary:     Vec<CouplingEntry>,
    pub significance:    Option<FileSignificance>,
    pub evidence:        EvidenceSummary,
    pub coverage:        CoverageMap,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileIdentity {
    pub first_commit:       Option<CommitSummary>,
    pub last_commit:        Option<CommitSummary>,
    pub touch_count:        i64,
    /// When `is_historical_path` is true: the canonical current path for this artifact.
    /// None when the queried path is already the current path, or no identity chain exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_path:       Option<String>,
    /// True when the queried path is a historical address — the artifact has since moved.
    /// Consumer should navigate to `current_path` for the live version.
    #[serde(skip_serializing_if = "is_false")]
    pub is_historical_path: bool,
}

fn is_false(b: &bool) -> bool { !b }

#[derive(Debug, Clone, Serialize)]
pub struct CommitSummary {
    pub short_hash: String,
    pub message:    String,
    pub author:     String,
    pub timestamp:  i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelatedHistory {
    pub pull_requests: Vec<PrSummary>,
    pub issues:        Vec<IssueSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrSummary {
    pub number:           i64,
    pub title:            String,
    pub state:            String,
    pub merge_commit_sha: Option<String>,
    pub linked_issues:    Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IssueSummary {
    pub number: i64,
    pub title:  String,
    pub state:  String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CouplingEntry {
    pub file_path:    String,
    pub change_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileSignificance {
    pub rank:        usize,
    pub total_files: usize,
    pub touch_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceSummary {
    pub commits:     usize,
    pub prs:         usize,
    pub issues:      usize,
    pub co_changes:  usize,
    pub total_facts: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CoverageMap {
    pub git_history:      CoverageStatus,
    pub rename_tracking:  CoverageStatus,
    pub github_prs:       CoverageStatus,
    pub github_issues:    CoverageStatus,
    pub documentation:    CoverageStatus,
    pub working_tree:     CoverageStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum CoverageStatus {
    /// Source was ingested and has data.
    Available,
    /// Source is recognised but not yet ingested.
    NotIngested,
    /// Detectable only through co-change proximity, not direct ingestion.
    CoChangeOnly,
    /// Data exists but is scoped to exact file paths — rename/move history is not tracked.
    /// Queries return correct facts about the current path but may be missing pre-rename history.
    PathScoped,
}

// ─── Campaign Engine ──────────────────────────────────────────────────────────

/// One entry from docs/gaps.toml — a tracked investigation gap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapEntry {
    pub id:                      String,
    pub name:                    String,
    pub classification:          String,
    pub description:             String,
    pub n:                       u32,
    pub threshold:               u32,
    /// "watch" | "candidate" | "earned" | "implemented"
    pub status:                  String,
    pub repositories:            Vec<String>,
    pub benchmarks:              Vec<String>,
    pub suggested_implementation: String,
    pub success_criterion:       String,
}

impl GapEntry {
    pub fn is_implemented(&self) -> bool { self.status == "implemented" }
    pub fn is_earned(&self)      -> bool { self.status == "earned" || (self.n >= self.threshold && !self.is_implemented()) }
    pub fn needs_to_earn(&self)  -> bool { !self.is_implemented() && !self.is_earned() }
    pub fn remaining(&self)      -> u32  { self.threshold.saturating_sub(self.n) }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CampaignOutcome {
    /// At least one gap has n >= threshold and is not implemented — ready to code.
    Ready { gap: GapEntry },
    /// No gap is earned yet — show the closest candidates.
    NoneEarned { candidates: Vec<GapEntry> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignBrief {
    pub outcome: CampaignOutcome,
    /// All gaps, sorted by N descending, for context.
    pub all_gaps: Vec<GapEntry>,
}

// ─── Project Registry (v0.7a) ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub id:          i64,
    pub name:        String,
    pub description: Option<String>,
}

/// Provenance of the existence claim for a repository.
/// Records HOW Atlas knows this repository exists — separate from whether
/// it can be accessed.  A user-confirmed repository may have no local path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExistenceSource {
    /// Atlas observed a readable git repository at the registered local path.
    LocalObserved,
    /// A user explicitly stated this repository exists.
    /// Atlas has not observed it.  No implementation facts may be assumed.
    UserConfirmed,
}

impl ExistenceSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LocalObserved => "local_observed",
            Self::UserConfirmed => "user_confirmed",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "local_observed" => Some(Self::LocalObserved),
            "user_confirmed" => Some(Self::UserConfirmed),
            _                => None,
        }
    }
}

/// Whether Atlas can read local files for this repository.
/// Orthogonal to ExistenceSource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccessState {
    /// A readable git repository exists at local_path.
    Accessible,
    /// No local path available, or path is not a readable git repository.
    NotAccessible,
}

impl AccessState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Accessible    => "accessible",
            Self::NotAccessible => "not_accessible",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "accessible"     => Some(Self::Accessible),
            "not_accessible" => Some(Self::NotAccessible),
            _                => None,
        }
    }
}

/// Whether Atlas has ingested knowledge from this repository into its DB.
/// Only meaningful when AccessState is Accessible.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IngestionState {
    /// Git history (and optionally GitHub/TypeScript) has been ingested.
    Ingested,
    /// Repository is accessible but has not been ingested yet.
    NotIngested,
    /// Not applicable — repository is not accessible.
    NotApplicable,
}

impl IngestionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ingested      => "ingested",
            Self::NotIngested   => "not_ingested",
            Self::NotApplicable => "not_applicable",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ingested"       => Some(Self::Ingested),
            "not_ingested"   => Some(Self::NotIngested),
            "not_applicable" => Some(Self::NotApplicable),
            _                => None,
        }
    }
}

/// A repository registered within a project.
/// `local_path` is the join key to all existing repo_path-scoped tables.
/// `role_label` is user-provided descriptive text only — never used for
/// any classification logic.  Observed capabilities come from ProfileClaims.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryRecord {
    pub id:               i64,
    pub project_id:       i64,
    pub name:             String,
    /// Free-form user description of this repository's role.  Informational only.
    pub role_label:       Option<String>,
    /// Canonical absolute local path — matches `repo_path` in existing tables.
    /// None when access_state is NotAccessible.
    pub local_path:       Option<String>,
    pub remote_url:       Option<String>,
    pub existence_source: ExistenceSource,
    pub access_state:     AccessState,
    pub ingestion_state:  IngestionState,
}

// ─── Repository Census (v0.7a) ────────────────────────────────────────────────

/// The kind of fact a profile claim records.
/// Minimum set derived from the five accessible RWATP repositories.
/// New variants added only when a new RWATP repository produces evidence
/// that cannot be expressed by an existing variant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProfileClaimKind {
    Runtime,           // e.g. "Node.js"
    Language,          // e.g. "TypeScript", "JavaScript"
    EntryPoint,        // e.g. "src/server.ts"
    Framework,         // e.g. "Express", "Apollo Server", "Next.js"
    Persistence,       // e.g. "MongoDB", "Redis"
    Messaging,         // e.g. "Google Pub/Sub"
    Auth,              // e.g. "Firebase Admin", "AWS JWT"
    EmailProvider,     // e.g. "SendGrid", "Nodemailer"
    TemplateEngine,    // e.g. "Handlebars"
    BlockchainClient,  // e.g. "Polymesh SDK"
    Module,            // a top-level src/ subdirectory name
    PackageManager,    // e.g. "npm", "Bun" (from lockfile presence)
}

impl ProfileClaimKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Runtime          => "runtime",
            Self::Language         => "language",
            Self::EntryPoint       => "entry_point",
            Self::Framework        => "framework",
            Self::Persistence      => "persistence",
            Self::Messaging        => "messaging",
            Self::Auth             => "auth",
            Self::EmailProvider    => "email_provider",
            Self::TemplateEngine   => "template_engine",
            Self::BlockchainClient => "blockchain_client",
            Self::Module           => "module",
            Self::PackageManager   => "package_manager",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "runtime"           => Some(Self::Runtime),
            "language"          => Some(Self::Language),
            "entry_point"       => Some(Self::EntryPoint),
            "framework"         => Some(Self::Framework),
            "persistence"       => Some(Self::Persistence),
            "messaging"         => Some(Self::Messaging),
            "auth"              => Some(Self::Auth),
            "email_provider"    => Some(Self::EmailProvider),
            "template_engine"   => Some(Self::TemplateEngine),
            "blockchain_client" => Some(Self::BlockchainClient),
            "module"            => Some(Self::Module),
            "package_manager"   => Some(Self::PackageManager),
            _                   => None,
        }
    }
}

/// The specific evidence that produced a profile claim.
/// Tagged enum — each variant carries exactly the fields needed to explain
/// how the observation was made.  No field is inferred or generated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClaimEvidence {
    /// A dependency (or devDependency) entry in package.json.
    PackageJsonDependency { package: String, version: String },
    /// A named script entry in package.json scripts.
    PackageJsonScript { script_name: String, command: String },
    /// The `main` field in package.json.
    PackageJsonMain { value: String },
    /// A specific file exists at this relative path.
    FileExists { relative_path: String },
    /// A directory exists at this relative path.
    DirectoryExists { relative_path: String },
    /// Source files with this extension were found; one example is provided.
    SourceExtension { extension: String, example_path: String },
    /// A lockfile with this filename is present at the repository root.
    LockfilePresent { filename: String },
}

/// A single typed, evidence-backed claim about a repository's characteristics.
/// Atlas emits only claims it can attribute to a specific, observable source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileClaim {
    pub kind:     ProfileClaimKind,
    pub value:    String,
    pub evidence: ClaimEvidence,
}

/// Census output for one repository.
/// Accessible repositories carry observed profile claims.
/// Unavailable repositories carry only their RepositoryRecord — no claims.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryCensusEntry {
    pub repository:   RepositoryRecord,
    /// Empty when access_state is NotAccessible.
    pub claims:       Vec<ProfileClaim>,
    /// Unix timestamp of last successful inspection.  None if never inspected.
    pub inspected_at: Option<i64>,
}

/// The assembled, evidence-backed census for an entire project.
/// schema_version increments when the JSON shape changes incompatibly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCensus {
    pub schema_version: u32,
    pub project:        ProjectRecord,
    pub entries:        Vec<RepositoryCensusEntry>,
}

// ─── Review Context Document (v0.7b) ─────────────────────────────────────────

/// One co-change partner of a PR file, with an in-PR flag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CochangeEntry {
    pub file:  String,
    pub count: i64,
    /// True when this co-change partner is itself also changed in the PR.
    pub in_pr: bool,
}

/// Per-file evidence assembled for a PR file: structural edges, co-changes,
/// and historical summary.  All structural edges are shown unscoped — not
/// filtered to "other PR files" — so the reviewer sees the full call graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrFileContext {
    pub file:                String,
    pub role:                ArtifactRole,
    pub touch_count:         i64,
    pub last_commit_message: Option<String>,
    /// Outgoing structural edges (imports, calls_static, calls_instance, references_model).
    pub structural_out:      Vec<StructuralEdgeSummary>,
    /// Incoming structural edges (files that import/call this file).
    pub structural_in:       Vec<StructuralEdgeSummary>,
    /// Files that co-changed with this one (min 2 commits), highest count first.
    pub cochanges:           Vec<CochangeEntry>,
}

/// Coverage booleans for the assembled review context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewCoverage {
    /// Git history was ingested and queries succeeded.
    pub git_history:      bool,
    /// GitHub PRs were ingested (documentary searches are meaningful).
    pub github_prs:       bool,
    /// GitHub issues were ingested.
    pub github_issues:    bool,
    /// Structural edges exist for at least one changed file.
    pub structural_edges: bool,
}

/// Assembled evidence for reviewing a specific PR.
///
/// Changed files are the mandatory seeds — no anchor scoring, no MAX_SEEDS
/// trimming, no concept-resolution lottery.  Every file in the PR is included;
/// structural and historical context is attached per-file without filtering.
///
/// schema_version increments when the JSON shape changes incompatibly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewContextDocument {
    pub schema_version:      u32,
    pub pr_number:           u64,
    pub pr_title:            String,
    pub pr_body:             String,
    /// Issue numbers that this PR closes/fixes (from closingIssuesReferences).
    pub linked_issue_numbers: Vec<i64>,
    /// One entry per changed file in the PR.
    pub pr_files:            Vec<PrFileContext>,
    /// PRs and issues from the documentary corpus that reference terms from
    /// the PR title.  Excludes the reviewed PR itself.
    pub documentary:         Vec<DocumentaryEvidence>,
    pub coverage:            ReviewCoverage,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_roundtrips_json() {
        let c = Commit {
            hash:          "abc".into(),
            short_hash:    "abc".into(),
            message:       "init".into(),
            author_name:   "Alice".into(),
            author_email:  "a@x.com".into(),
            timestamp:     DateTime::from_timestamp(0, 0).unwrap(),
            files_changed: vec!["src/main.rs".into()],
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: Commit = serde_json::from_str(&json).unwrap();
        assert_eq!(back.hash, c.hash);
        assert_eq!(back.files_changed, c.files_changed);
    }

    #[test]
    fn pull_request_roundtrips_json() {
        let pr = PullRequest {
            number:           1,
            title:            "Add feature".into(),
            state:            "merged".into(),
            body:             None,
            author:           "bob".into(),
            merge_commit_sha: Some("def456".into()),
            created_at:       Some(DateTime::from_timestamp(1_700_000_000, 0).unwrap()),
            merged_at:        Some(DateTime::from_timestamp(1_700_000_100, 0).unwrap()),
        };
        let back: PullRequest = serde_json::from_str(&serde_json::to_string(&pr).unwrap()).unwrap();
        assert_eq!(back.number, 1);
        assert_eq!(back.merge_commit_sha, Some("def456".into()));
        assert_eq!(back.created_at, pr.created_at);
        assert_eq!(back.merged_at, pr.merged_at);
    }

    #[test]
    fn issue_roundtrips_json() {
        let issue = Issue {
            number:     10,
            title:      "Bug".into(),
            state:      "CLOSED".into(),
            body:       None,
            author:     "alice".into(),
            created_at: Some(DateTime::from_timestamp(1_700_000_000, 0).unwrap()),
        };
        let back: Issue = serde_json::from_str(&serde_json::to_string(&issue).unwrap()).unwrap();
        assert_eq!(back.number, 10);
        assert_eq!(back.created_at, issue.created_at);
    }
}
