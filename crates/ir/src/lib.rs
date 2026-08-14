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
    /// Parent commit hashes.  Empty for root commits, one for normal commits,
    /// two or more for merges.  Preserved so the commit DAG is queryable.
    #[serde(default)]
    pub parents:       Vec<String>,
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
    /// A class/type explicitly implements an interface (TypeScript `implements`).
    /// `target_symbol` is the interface name; `target_file` is the resolved
    /// interface file when the name was imported, else UNRESOLVED:type:Name.
    Implements,
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

/// Weighted signal breakdown used to rank this candidate.
/// Each component is in [0.0, 1.0]; `total` is the weighted sum.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub lexical:    f32,
    pub structural: f32,
    pub historical: f32,
    pub centrality: f32,
    pub total:      f32,
}

/// A single artifact under investigation, with the reasons it was included.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateArtifact {
    pub file:    String,
    pub role:    ArtifactRole,
    pub reasons: Vec<CandidateReason>,
    pub score:   ScoreBreakdown,
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
    /// Lexicon-based vocabulary expansions added before concept resolution.
    pub lexicon_expansions:   Vec<LexiconExpansion>,
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
    /// Candidate files that appeared in git history but no longer exist on disk.
    /// Surfaced so callers know what was filtered rather than silently dropped.
    pub deleted_candidates:   Vec<String>,
    /// Per-anchor identity redirects.  When a user supplies a file-path anchor
    /// that Atlas recognises as historical (renamed away), the current
    /// canonical path is added to `effective_anchors` AND recorded here so
    /// the output preserves the original user input.  Empty when no anchor
    /// triggered a redirect.
    #[serde(default)]
    pub anchor_redirects:     Vec<AnchorRedirect>,
}

/// A single anchor was recognised as a historical file path and re-anchored
/// to its current canonical path.  Both are surfaced so the user knows which
/// address they supplied and which address Atlas actually searched under.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorRedirect {
    pub original_anchor: String,
    pub current_path:    String,
    pub identity_id:     i64,
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

// ─── Repository Lexicon ───────────────────────────────────────────────────────

/// How a vocabulary relationship was discovered.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LexiconRelKind {
    /// "op" ↔ "operation" — detected by prefix/suffix matching rules
    Abbreviation,
    /// "MutableRepo" ↔ "mutablerepo" — same token, different casing
    CaseVariant,
    /// "op_store" is a compound of "op" and "store"
    CompoundComponent,
    /// Term A appears in commit messages that touch files named with term B (≥3 commits)
    CommitBridge,
    /// Term appears in PR/issue bodies alongside repository file-path terms
    DocumentAlias,
    /// Two files in the same directory share tokens — sibling naming convention
    ModuleSibling,
    /// "tokenization" → "tokenize" — regular English derivation suffix (*ize → *ization, etc.)
    MorphologicalVariant,
}

impl LexiconRelKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Abbreviation         => "abbreviation",
            Self::CaseVariant          => "case_variant",
            Self::CompoundComponent    => "compound_component",
            Self::CommitBridge         => "commit_bridge",
            Self::DocumentAlias        => "document_alias",
            Self::ModuleSibling        => "module_sibling",
            Self::MorphologicalVariant => "morphological_variant",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "abbreviation"          => Some(Self::Abbreviation),
            "case_variant"          => Some(Self::CaseVariant),
            "compound_component"    => Some(Self::CompoundComponent),
            "commit_bridge"         => Some(Self::CommitBridge),
            "document_alias"        => Some(Self::DocumentAlias),
            "module_sibling"        => Some(Self::ModuleSibling),
            "morphological_variant" => Some(Self::MorphologicalVariant),
            _                       => None,
        }
    }
}

/// A single vocabulary relationship in the lexicon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LexiconRelationship {
    pub from_term:          String,
    pub to_term:            String,
    pub kind:               LexiconRelKind,
    /// 0.0–1.0; CaseVariant = 1.0, Abbreviation ≈ 0.90, CommitBridge depends on co-occurrence
    pub confidence:         f32,
    pub co_occurrence_count: u32,
}

/// One term that the lexicon expanded an anchor into, shown in investigation output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LexiconExpansion {
    /// The original anchor term that was expanded.
    pub original_term:   String,
    /// How it was expanded.
    pub kind:            LexiconRelKind,
    /// The resolved term added to the effective anchor set.
    pub resolved_term:   String,
    /// Confidence of the relationship.
    pub confidence:      f32,
    /// Example evidence: a file path or commit hash that grounded this expansion.
    pub grounded_in:     String,
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

// ─── Show Record (v0.8c — B3 drill-down, transient) ────────────────────────
//
// Produced by `atlas show <subject>`.  Represents ONE concrete Atlas
// record with its immediate provenance and its deterministically-linked
// neighbours.  Each linked row carries a `token` that the caller can pass
// back into `atlas show` — the drill-down affordance is a first-class
// field, not implied.
//
// Not persisted.  No new ontology concept — every field is a projection
// over existing tables (commits, pull_requests, issues, files,
// file_identities, file_path_observations, structural_edges, documents,
// configuration_artifacts, ingest_runs).

#[derive(Debug, Clone, Serialize)]
pub struct ShowRecord {
    pub schema_version: u32,
    /// The original argument the caller passed.
    pub subject_input: String,
    /// The resolved concrete subject.
    pub subject: ShowSubject,
    /// If the caller passed a historical file path, the redirect that
    /// steered `show` to the current canonical path.  Reuses the same
    /// `HistoricalRedirect` type used by `inspect`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_note: Option<HistoricalRedirect>,
    /// Ordered drill-down sections.  Each section is a set of concrete
    /// rows from ONE source table.  No aggregation across tables.
    pub sections: Vec<ShowSection>,
    /// Flattened next-hop links surfaced anywhere in the record,
    /// deduplicated and sorted.  Every token here is a valid argument
    /// to `atlas show`.
    pub links: Vec<ShowLink>,
    /// When this subject was ingested and by which run.
    pub provenance: ShowProvenance,
}

/// Concrete resolved subject.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShowSubject {
    Commit(CommitSubject),
    Pr(PrSubject),
    Issue(IssueSubject),
    File(FileSubject),
    Identity(IdentitySubject),
    Document(DocumentSubject),
    ConfigArtifact(ConfigArtifactSubject),
    IngestRun(IngestRunSubject),
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitSubject {
    pub hash:         String,
    pub short_hash:   String,
    pub author_name:  String,
    pub author_email: String,
    pub timestamp:    i64,
    pub message:      String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrSubject {
    pub number:           i64,
    pub title:            String,
    pub state:            String,
    pub author:           String,
    pub merge_commit_sha: Option<String>,
    pub created_at:       Option<i64>,
    pub merged_at:        Option<i64>,
    pub body_excerpt:     String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IssueSubject {
    pub number:       i64,
    pub title:        String,
    pub state:        String,
    pub author:       String,
    pub created_at:   Option<i64>,
    pub body_excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileSubject {
    pub relative_path:   String,
    pub analysis_status: Option<String>,
    pub identity_id:     Option<i64>,
    pub role:            Option<ArtifactRole>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentitySubject {
    pub identity_id:  i64,
    pub current_path: Option<String>,
    pub path_history_count: usize,
    pub commit_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentSubject {
    pub file_path: String,
    pub doc_type:  String,
    pub title:     String,
    /// Truncated body unless the CLI was invoked with `--full`.
    pub body_excerpt: String,
    pub body_bytes:   usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigArtifactSubject {
    pub file_path:     String,
    pub artifact_kind: String,
    pub sha256:        String,
    pub raw_bytes:     usize,
    pub body_excerpt:  String,
    pub ingested_at:   i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IngestRunSubject {
    pub id:              i64,
    pub started_at:      i64,
    pub ended_at:        Option<i64>,
    pub atlas_version:   String,
    pub git_head:        Option<String>,
    pub git_branch:      Option<String>,
    pub requested_scope: String,
    pub exit_status:     String,
}

/// One drill-down section — a set of concrete rows from a single source table.
#[derive(Debug, Clone, Serialize)]
pub struct ShowSection {
    /// e.g. "PARENTS", "CHANGED FILES", "STRUCTURAL EDGES (outgoing)".
    pub title:             String,
    pub kind:              ShowSectionKind,
    /// Human-readable name of the source table for provenance.
    pub provenance_table:  String,
    pub rows:              Vec<ShowRow>,
    /// If the section was truncated for display, how many additional
    /// rows were omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated_count:   Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShowSectionKind {
    /// Section content is a direct SELECT over one table — no computation.
    Deterministic,
    /// Section content is derived from ≥1 join or aggregation.
    Derived,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShowRow {
    pub display: String,
    /// If this row is itself a followable subject, the link the caller
    /// can pass back to `atlas show`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link:    Option<ShowLink>,
}

/// A next-hop the caller can pass back into `atlas show`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub struct ShowLink {
    /// e.g. "commit 1217d25" or "PR #159"
    pub label: String,
    /// Exact string the caller passes to `atlas show <token>`.
    pub token: String,
    /// One of: commit | pr | issue | file | identity | document | config | run.
    pub kind:  String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShowProvenance {
    pub repo_path:   String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingested_at: Option<i64>,
    /// The most recent `ingest_runs.id` for this repo.  Not necessarily the
    /// run that first observed the subject — Atlas doesn't track that yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_run_id: Option<i64>,
}

// ─── Module Coupling Report (v0.8b — B2 aggregation, transient) ────────────
//
// Produced by `atlas coupling`.  Aggregates `structural_edges` into
// module-to-module coupling records, where "module" = an immediate child
// directory of the subject (default `src/modules`).
//
// Canonical representation is a SPARSE list of non-zero coupling cells.
// The dense N×N matrix is a render-time convenience for small N — it is
// NOT stored in this struct.  This scales to repositories with hundreds
// of modules without producing an N²-sized report.
//
// Not persisted.  Not a new Atlas ontology concept.  Serialize-only.
//
// Language discipline: cell values are described as "observed edges"
// — not "coupling strength" or "tight coupling".  Whether a count
// constitutes an architectural concern is a semantic call left to the
// caller.

#[derive(Debug, Clone, Serialize)]
pub struct ModuleCouplingReport {
    pub schema_version: u32,
    /// Original subject as the caller supplied it.
    pub subject: String,
    /// Immediate child directories of `subject`, alphabetical.
    pub modules: Vec<String>,
    /// Sparse list of non-zero source→target module edge aggregates,
    /// sorted by `edge_count` descending then by (source, target).
    /// This is the canonical representation.
    pub cells: Vec<ModuleCouplingCell>,
    /// Derived per-source aggregates.
    pub fan_out: Vec<ModuleFanIndicator>,
    /// Derived per-target aggregates.
    pub fan_in: Vec<ModuleFanIndicator>,
    /// Edges from a module to `UNRESOLVED:external:*` targets.
    /// One row per (source_module, external_target), sorted by count desc.
    pub external_dependencies: Vec<ExternalDependencyRow>,
    /// Edges from a module to a repo file NOT under `subject/*` (typically
    /// `src/common/`, `src/infrastructure/`, `src/graphql/`, …).
    /// Aggregated by the first path segment after the repo root, so
    /// hundreds of individual files collapse to a small set of platform
    /// destinations.  Sorted by count desc.
    pub platform_usage: Vec<PlatformUsageRow>,
}

/// One (source_module → target_module) coupling record.  Present only
/// when at least one observed edge crosses this boundary.
#[derive(Debug, Clone, Serialize)]
pub struct ModuleCouplingCell {
    pub source_module: String,
    pub target_module: String,
    /// Total observed structural edges.  See `ModuleCouplingKindBreakdown`
    /// for the per-kind split.
    pub edge_count: usize,
    /// Distinct source files (in `source_module`) that produced ≥1 edge.
    pub distinct_source_files: usize,
    /// Distinct target files (in `target_module`) reached by ≥1 edge.
    pub distinct_target_files: usize,
    /// Per-kind counts.  Kinds with zero edges are omitted.
    /// Kinds observed today: `imports`, `calls_static`, `calls_instance`,
    /// `references_model`.
    pub kinds: Vec<ModuleCouplingKindBreakdown>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleCouplingKindBreakdown {
    pub kind: String,
    pub edge_count: usize,
}

/// Derived per-module aggregate.  `fan` counts distinct partner modules.
#[derive(Debug, Clone, Serialize)]
pub struct ModuleFanIndicator {
    pub module: String,
    pub edges: usize,
    /// Number of distinct partner modules.
    pub fan: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExternalDependencyRow {
    pub source_module: String,
    pub external_target: String,
    pub edge_count: usize,
    pub distinct_source_files: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlatformUsageRow {
    pub source_module: String,
    /// First path segment of the target (e.g. `src/common`, `src/infrastructure`).
    pub platform_target: String,
    pub edge_count: usize,
    pub distinct_source_files: usize,
    pub distinct_target_files: usize,
}

// ─── Peer Structure Report (v0.8a — B1 aggregation, transient) ─────────────
//
// Produced by `atlas conventions <path>`.  Aggregates file-existence
// evidence across peer directories to report REPEATED STRUCTURAL PATTERNS.
//
// Deliberately does NOT use the word "convention" in field/type names or
// output.  A prevalence is just a count; whether a prevalence constitutes
// a convention is a semantic judgement left to the reader.  This report
// exposes the raw counts and lets the caller decide.
//
// Not persisted.  Not a new Atlas ontology concept.  Serialize-only,
// following the same pattern as `RepositoryTree`, `InspectionDocument`,
// and `InvestigationDocument`.

#[derive(Debug, Clone, Serialize)]
pub struct PeerStructureReport {
    pub schema_version: u32,
    /// Original subject as the caller supplied it.
    pub subject: String,
    /// The directory whose immediate children form the peer set.
    /// Equals `subject` when the caller passed a parent; equals
    /// `parent(subject)` when the caller passed one of the peer children.
    pub peer_parent: String,
    /// Every immediate child directory of `peer_parent`, sorted alphabetical.
    /// This is the FULL peer set.  No exclusion of "stubs" or
    /// "low-complexity" peers — the denominator for every prevalence
    /// in `patterns` is `peers.len()`.
    pub peers: Vec<String>,
    /// Structural elements observed in ≥ 2 peers, sorted by prevalence
    /// descending, then element name for determinism.  Elements observed
    /// in only 1 peer appear in `singletons` instead.
    pub patterns: Vec<PeerStructurePattern>,
    /// Elements observed in exactly 1 peer — surfaced separately so
    /// "unique to X" is not confused with a repeated pattern.
    pub singletons: Vec<PeerStructurePattern>,
    /// Per-peer deviations: elements this peer lacks that ≥ the
    /// deviation threshold of its peers have.  The threshold is stated
    /// explicitly on the report so consumers can judge the classification.
    pub deviations: Vec<PeerStructureDeviation>,
    /// The threshold used for producing `deviations`, expressed as a
    /// fraction (num/den).  Default: strict majority (num*2 > den).
    pub deviation_threshold_num: usize,
    pub deviation_threshold_den: usize,
    /// Derived observation, reported SEPARATELY from prevalence counts:
    /// peers with fewer than `file_count_threshold` files.  Explicit so
    /// consumers can decide whether to trust the classification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub low_complexity_note: Option<LowComplexityNote>,
}

/// A single observed element and the peers that carry it.
#[derive(Debug, Clone, Serialize)]
pub struct PeerStructurePattern {
    /// e.g. `services/`, `graphql/permissions.ts`, `graphql/*.typeDefs.ts`
    pub element: String,
    /// Sorted, alphabetical.
    pub present_in: Vec<String>,
    /// `present_in.len()`.
    pub prevalence_num: usize,
    /// The peer count (denominator).
    pub prevalence_den: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerStructureDeviation {
    /// Which peer is missing the element.
    pub peer: String,
    /// The element that peer lacks.
    pub element: String,
    /// How many of the OTHER peers have the element.
    pub peer_prevalence_num: usize,
    /// `peers.len()`.
    pub peer_prevalence_den: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LowComplexityNote {
    /// Peers with strictly fewer files than this are listed below.
    pub file_count_threshold: usize,
    /// `(peer_name, file_count)`, sorted alphabetical.
    pub low_complexity_peers: Vec<(String, usize)>,
}

// ─── Inspection Document (v0.7d — evidence attached to a spatial subject) ───
//
// Produced fresh on every `atlas inspect <path>` call by aggregating existing
// Atlas evidence within a single path (file or directory subtree).  Not
// persisted.  Answers "what does Atlas know that is *inside* this path?"
// rather than "what is *related* to this concept?" (that is `investigate`).

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InspectionSubjectKind {
    File,
    Directory,
}

/// A compact projection of `StructuralEdge` for inspection output.  Carries
/// both endpoints and their symbols; drops evidence line/snippet noise which
/// is not useful in a subject-level view.
#[derive(Debug, Clone, Serialize)]
pub struct InspectionEdge {
    pub source_file:   String,
    pub target_file:   String,
    /// One of "imports", "calls_static", "calls_instance", "references_model".
    pub kind:          String,
    pub source_symbol: Option<String>,
    pub target_symbol: Option<String>,
}

/// One immediate child of a directory subject, sorted alphabetically by name.
#[derive(Debug, Clone, Serialize)]
pub struct InspectionChild {
    pub name: String,
    pub kind: TreeNodeKind,
}

/// One ingested document whose `file_path` lives inside the inspection subject.
#[derive(Debug, Clone, Serialize)]
pub struct InspectionDocumentRef {
    pub file_path: String,
    /// "decision" | "adr" | "readme" | "doc"
    pub doc_type:  String,
    pub title:     String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectionCoverage {
    pub git_history:      bool,
    pub github_prs:       bool,
    pub github_issues:    bool,
    pub structural_edges: bool,
    pub documentation:    bool,
    pub profile_claims:   bool,
    /// True when the subject path resolves to something readable on disk.
    pub working_tree:     bool,
}

/// The assembled inspection view for one path.  Every field is populated
/// from existing Atlas evidence — no new observations are generated here.
/// schema_version increments on incompatible JSON shape changes.
#[derive(Debug, Clone, Serialize)]
pub struct InspectionDocument {
    pub schema_version:  u32,
    /// The path as supplied by the caller (may be relative, may include trailing '/').
    pub subject:         String,
    /// The subject normalised to a repo-relative, forward-slash path
    /// with no leading or trailing slash.  Empty string when the subject
    /// is the repository root itself.
    pub relative_path:   String,
    pub kind:            InspectionSubjectKind,
    /// Whether the subject was found on disk at inspection time.
    pub exists_on_disk:  bool,

    // ── File-only ────────────────────────────────────────────────────────
    /// Deterministic path-based role classification.  File subjects only.
    pub role:             Option<ArtifactRole>,
    /// Identity chain summary for file subjects (from `build_context`).
    pub identity:         Option<FileIdentity>,
    /// Co-change partners for file subjects (from `build_context`).
    pub coupling:         Vec<CouplingEntry>,

    // ── Directory-only ───────────────────────────────────────────────────
    /// Immediate children, sorted alphabetically, with `RepoAwareness`
    /// exclusions applied.  Empty for file subjects.
    pub children:         Vec<InspectionChild>,
    /// Top hot files within the subject subtree.  Empty for file subjects.
    pub hot_files_within: Vec<CouplingEntry>,

    // ── Both ─────────────────────────────────────────────────────────────
    /// Most recent commits touching the subject (subtree or single file).
    pub recent_activity:  Vec<CommitSummary>,
    /// Count of distinct commits touching the subject.
    pub touch_count:      i64,
    /// PRs + issues reachable through those commits.
    pub related_history:  RelatedHistory,

    /// Boundary-crossing edges from within the subject subtree to files
    /// outside it.  For file subjects, all outgoing edges.
    pub structural_depends_on: Vec<InspectionEdge>,
    /// Boundary-crossing edges from outside the subject subtree to files
    /// inside it.  For file subjects, all incoming edges.
    pub structural_used_by:    Vec<InspectionEdge>,
    /// Edges wholly within the subject subtree (both endpoints inside).
    /// Cohesion signal.  Always empty for file subjects.
    pub structural_internal:   Vec<InspectionEdge>,

    /// Documents whose `file_path` lives inside the subject subtree.
    pub documents:       Vec<InspectionDocumentRef>,
    /// Ambient repository claims (Runtime, Language, PackageManager) plus
    /// any observed Module claim that matches the subject.
    pub profile_claims:  Vec<ProfileClaim>,

    /// If the caller supplied a historical path (file subject only), this
    /// records the redirect: the original address, the current canonical
    /// path Atlas actually queried, and the identity id.  `None` when no
    /// redirect happened (path was current or unknown to FileIdentity).
    ///
    /// Directory subjects never populate this — Atlas has no directory
    /// identity concept, and subtree aggregation uses the current-tree
    /// prefix (see the doc-comment on `structural_depends_on`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub historical_redirect: Option<HistoricalRedirect>,

    pub coverage:        InspectionCoverage,
}

/// Explicit record that a file subject was renamed and Atlas queried the
/// current path instead of what the user typed.  Never invented — only
/// present when `FileIdentity` resolves the historical → current mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalRedirect {
    pub original_subject: String,
    pub current_path:     String,
    pub identity_id:      i64,
}

// ─── Repository Tree (v0.7c — transient spatial view) ───────────────────────
//
// Produced fresh on every `atlas tree` call from the working tree on disk.
// Never persisted, never joined against ingestion evidence.  Purpose: give
// downstream commands (e.g. Step 3 `atlas inspect`) a stable navigation
// coordinate system.  This is Structure, not Evidence.

/// A single entry in the on-disk repository tree.
#[derive(Debug, Clone, Serialize)]
pub struct TreeNode {
    /// Basename (e.g. "src", "main.rs", "identity").
    pub name:          String,
    /// Path relative to the repository root, forward-slash normalised.
    /// Empty string for the root node itself.  No leading or trailing slash.
    pub relative_path: String,
    pub kind:          TreeNodeKind,
    /// Sorted alphabetically by `name` (case-sensitive) for deterministic output.
    /// Empty for `File` nodes and for directories at the depth limit.
    pub children:      Vec<TreeNode>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TreeNodeKind {
    Directory,
    File,
}

/// The assembled repository tree.  Recomputed each run; never persisted.
/// schema_version increments only on incompatible JSON shape changes.
#[derive(Debug, Clone, Serialize)]
pub struct RepositoryTree {
    pub schema_version: u32,
    /// Canonical absolute path to the repository root.
    pub repo_path:      String,
    pub root:           TreeNode,
    /// None when no depth limit was supplied (unlimited walk).
    pub depth_limit:    Option<u32>,
    /// Repo-relative paths of directories that were skipped, so consumers can
    /// see what the tree does NOT cover.  Sorted, deduplicated.
    pub excluded:       Vec<String>,
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

// ─── Authors Report (v0.8a — B4 aggregation, transient) ─────────────────────
//
// Produced by `atlas authors <path>`.  Aggregates `commits.author_name`
// and `commits.author_email` for a subject (repo root, directory subtree,
// or file) via a repo-scoped GROUP BY on `commits + commit_files`, or the
// rename-safe identity chain when the subject is a file with a materialised
// FileIdentity.
//
// Language discipline: rows are OBSERVED COMMITS by an author, not a
// claim about ownership, expertise, or contribution weight.  `(name, email)`
// tuples are the identity — no alias merging.
//
// Not persisted.  Not a new Atlas ontology concept.  Serialize-only,
// following the same pattern as `ModuleCouplingReport` and
// `PeerStructureReport`.

/// Which lens the aggregation used.  Reported explicitly so the caller
/// sees whether numbers came from a path prefix, an exact file, or the
/// rename-safe identity chain.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthorScope {
    /// Subtree — SQL: `commit_files.file_path LIKE 'prefix/%'`.
    /// Also used for the repo root (empty prefix → `LIKE '%'`).
    Prefix,
    /// Exact file — SQL: `commit_files.file_path = ?`.
    /// Used when the subject is a file with no FileIdentity chain.
    ExactFile,
    /// Rename-safe identity chain — SQL join through
    /// `file_identity_commits`.  Used when the subject is a file whose
    /// FileIdentity was materialised at ingest.
    Identity,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorsReport {
    pub schema_version: u32,
    /// Original subject as the caller supplied it.
    pub subject:        String,
    /// Which aggregation lens was used (see `AuthorScope`).
    pub scope:          AuthorScope,
    /// Human-readable description of the scope — the exact LIKE pattern,
    /// the exact file path, or `identity <id>` (`n` path observations).
    /// Purely for display; never re-parsed.
    pub scope_detail:   String,
    /// Sorted by `commit_count` DESC, then `author_name` ASC.  `(name, email)`
    /// tuples are the identity — no alias merging.
    pub authors:        Vec<AuthorAggregate>,
    /// Sum of `commit_count` across all authors.  Equals the number of
    /// DISTINCT commits touching the subject (a commit that touched N
    /// files in the subtree counts as 1, not N).
    pub total_commits:  usize,
    /// `authors.len()` — number of distinct `(name, email)` tuples.
    pub total_authors:  usize,
    /// Populated only when the caller passed a historical file path;
    /// reused from the `inspect` / `show` redirect note pattern.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_note:  Option<HistoricalRedirect>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorAggregate {
    pub author_name:  String,
    pub author_email: String,
    /// DISTINCT commits touching the subject by this `(name, email)`.
    pub commit_count: usize,
    /// Unix seconds; earliest commit this author had on the subject.
    pub first_touch:  i64,
    /// Unix seconds; latest commit this author had on the subject.
    pub last_touch:   i64,
}

// ─── Modules Inventory (v0.8d — B5 aggregation, transient) ──────────────────
//
// Produced by `atlas modules [path]`.  Enumerates immediate child directories
// of a subject (default `src/modules`) and attaches deterministic counts from
// existing evidence (files, commits, structural edges).  Not a semantic
// domain classifier.  Not persisted.

#[derive(Debug, Clone, Serialize)]
pub struct ModulesReport {
    pub schema_version: u32,
    /// Caller-supplied subject (default `src/modules`).
    pub subject: String,
    /// Immediate child directory names under `subject`, alphabetical.
    pub modules: Vec<ModuleEntry>,
    /// Explicit rule used to discover modules.
    pub discovery_rule: String,
    pub total_modules: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleEntry {
    /// Directory name (e.g. `identity`), not full path.
    pub name: String,
    /// Full path under subject (e.g. `src/modules/identity`).
    pub path: String,
    /// DETERMINISTIC: count of files under this module path in `files`.
    pub file_count: usize,
    /// DETERMINISTIC: immediate subdirectories of this module.
    pub subdirectories: Vec<String>,
    /// DETERMINISTIC: distinct commits that touched any file under the module
    /// (via commits + commit_files, repo-scoped).
    pub observed_commit_count: usize,
    /// DETERMINISTIC: structural edges with source under this module.
    pub outgoing_edge_count: usize,
    /// DETERMINISTIC: structural edges with target under this module.
    pub incoming_edge_count: usize,
    /// DETERMINISTIC: count of files classified as ArtifactRole::Test under
    /// this module path (path-heuristic classification only).
    pub in_module_test_file_count: usize,
    /// DERIVED: true when `in_module_test_file_count > 0` OR a top-level
    /// `tests/<name>/` tree exists in the files table.
    pub has_associated_tests: bool,
    /// Rule text explaining how `has_associated_tests` was derived.
    pub test_association_rule: String,
}

// ─── Test ↔ Module Linkage (v0.8e — B6 aggregation, transient) ──────────────

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TestLinkageKind {
    /// Test path is physically under the module directory.
    DirectPathPrefix,
    /// Test lives under conventional `tests/<module>/…` (or similar) and the
    /// named module exists as an immediate child of the modules subject.
    ConventionalTestsDir,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceClass {
    Deterministic,
    Derived,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestModuleLink {
    pub test_path: String,
    pub module_name: String,
    pub module_path: String,
    pub linkage_kind: TestLinkageKind,
    pub evidence_class: EvidenceClass,
    /// Human-readable statement of the exact rule applied.
    pub rule: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestModuleReport {
    pub schema_version: u32,
    /// Modules subject used for discovery (default `src/modules`).
    pub modules_subject: String,
    /// Optional path filter the caller supplied (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_filter: Option<String>,
    pub modules: Vec<String>,
    pub links: Vec<TestModuleLink>,
    /// Test files classified as tests that matched no linkage rule.
    pub unlinked_tests: Vec<String>,
    pub total_test_files: usize,
    pub total_links: usize,
    pub linkage_rules: Vec<String>,
}

// ─── NPM Dependency → Source Linkage (v0.8f — B7 aggregation, transient) ───

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencySection {
    Dependencies,
    DevDependencies,
    OptionalDependencies,
    PeerDependencies,
    /// Package observed via structural edges but not found in package.json.
    Undeclared,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageSourceObservation {
    pub source_file: String,
    pub target_spec: String,
    pub edge_kind: String,
    pub evidence_snippet: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageDependencyEntry {
    pub package_name: String,
    pub section: DependencySection,
    /// Version string from package.json when declared; empty when undeclared.
    pub declared_version: String,
    /// True when package.json lists this package (section ≠ Undeclared).
    pub is_declared: bool,
    /// True when ≥1 structural edge targets UNRESOLVED:external:<package>…
    pub is_observed: bool,
    pub observations: Vec<PackageSourceObservation>,
    pub observation_count: usize,
    pub distinct_source_files: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DependencyLinkageReport {
    pub schema_version: u32,
    /// Where declared dependencies were read from.
    pub declaration_source: String,
    /// Whether declaration content came from configuration_artifacts or disk.
    pub declaration_provenance: String,
    pub packages: Vec<PackageDependencyEntry>,
    pub total_declared: usize,
    pub total_observed: usize,
    pub declared_and_observed: usize,
    pub declared_unobserved: usize,
    pub observed_undeclared: usize,
    pub methodology: Vec<String>,
}

// ─── Cross-directory Co-change Cohorts (v0.8g — B8 aggregation, transient) ─

#[derive(Debug, Clone, Serialize)]
pub struct DirectoryPairCochange {
    pub directory_a: String,
    pub directory_b: String,
    /// Distinct commits that touched both directories.
    pub cochange_commit_count: usize,
    pub evidence_class: EvidenceClass,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirectoryCohort {
    /// Member directory names (relative to subject), sorted.
    pub members: Vec<String>,
    /// Minimum pairwise co-change count among members that connected the cohort.
    pub min_edge_cochange: usize,
    /// Sum of pairwise co-change counts over edges used in the cohort graph.
    pub total_edge_cochange: usize,
    pub evidence_class: EvidenceClass,
}

#[derive(Debug, Clone, Serialize)]
pub struct CohortsReport {
    pub schema_version: u32,
    pub subject: String,
    /// Immediate child directories considered as cohort candidates.
    pub directories: Vec<String>,
    /// Minimum co-change commit count for a pair to form an edge.
    pub cochange_threshold: usize,
    pub pairs: Vec<DirectoryPairCochange>,
    /// Connected components of the pair graph (size ≥ 2).
    pub cohorts: Vec<DirectoryCohort>,
    /// Directories with no pair above threshold (listed explicitly; not discarded).
    pub singletons: Vec<String>,
    pub methodology: Vec<String>,
}

// ─── Anomalies (v0.8h — B9 aggregation, transient) ─────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnomalyKind {
    /// Peer lacks an element present in a strict majority of peers (B1).
    PeerStructureDeviation,
    /// Module has no associated tests under documented linkage rules (B6).
    MissingAssociatedTests,
    /// Declared package.json dependency with zero structural observations (B7).
    DeclaredDependencyUnobserved,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnomalyEntry {
    pub kind: AnomalyKind,
    pub subject: String,
    /// What was observed that constitutes a deviation.
    pub observation: String,
    /// The peer pattern / rule it differs from.
    pub expected: String,
    pub evidence_class: EvidenceClass,
    /// Concrete supporting counts/paths.
    pub evidence: Vec<String>,
    pub threshold_note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnomaliesReport {
    pub schema_version: u32,
    pub subject: String,
    pub anomalies: Vec<AnomalyEntry>,
    pub total_anomalies: usize,
    pub methodology: Vec<String>,
}

// ─── Configuration Provenance (v0.8i — B10 aggregation, transient) ─────────

#[derive(Debug, Clone, Serialize)]
pub struct ConfigHistoryCommit {
    pub hash: String,
    pub short_hash: String,
    pub message: String,
    pub author_name: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigArtifactReport {
    pub schema_version: u32,
    pub file_path: String,
    /// True when a row exists in configuration_artifacts.
    pub artifact_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingested_at: Option<i64>,
    /// Content length in bytes when artifact is present (content itself not
    /// duplicated here unless requested via full show path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_byte_len: Option<usize>,
    /// Commits that touched this exact path (repo-scoped).
    pub touching_commits: Vec<ConfigHistoryCommit>,
    pub touching_commit_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_touch: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_touch: Option<i64>,
    /// Historical redirect when the path was renamed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_note: Option<HistoricalRedirect>,
    /// Identity-scoped commit count when a FileIdentity exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_commit_count: Option<usize>,
    /// Explicit limitations (e.g. no historical content snapshots).
    pub limitations: Vec<String>,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigInventoryReport {
    pub schema_version: u32,
    /// Artifacts from configuration_artifacts for this repo.
    pub artifacts: Vec<ConfigArtifactSummary>,
    pub total_artifacts: usize,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigArtifactSummary {
    pub file_path: String,
    pub artifact_kind: String,
    pub sha256: String,
    pub ingested_at: i64,
    pub touching_commit_count: usize,
}

// ─── Reasoning / Investigation Loop (v0.9 — evidence packet + claims) ───────
//
// Transient IR for the local-AI investigation loop.  AI proposes hypotheses;
// Atlas verifies them against the evidence packet.  Nothing here is persisted
// as repository truth.  schema_version increments on incompatible shape change.

/// Pointer to a concrete piece of Atlas evidence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceRef {
    /// e.g. "commit" | "pr" | "issue" | "file" | "structural_edge" | "document" | "config"
    pub kind: String,
    /// Kind-specific id (hash, "pr#12", path, …).
    pub id: String,
    /// One-line human summary (not a claim of truth beyond the evidence itself).
    pub summary: String,
    /// Unix seconds when known; None if not applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
}

/// A single chronology event for investigation (ordered by timestamp).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChronologyEvent {
    pub timestamp: i64,
    pub kind: String,
    pub id: String,
    pub summary: String,
    /// Whether this event is primarily intent (issue/PR) or implementation (commit).
    pub role: String,
}

/// One ranked evidence item for the reasoning packet (C4-ER).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedEvidenceItem {
    pub rank: u32,
    pub ref_: EvidenceRef,
    /// implementation | intent | structural | documentary | historical
    pub event_semantics: String,
    pub dimensions: EvidenceDimensions,
    pub weight: f32,
    pub ranking_notes: Vec<String>,
}

/// Temporal supersession note: newer implementation may supersede older intent/impl.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupersessionNote {
    pub earlier_id: String,
    pub later_id: String,
    pub relationship: String,
    pub note: String,
}

/// Bounded evidence assembled for reasoning.  Never the full repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePacket {
    pub schema_version: u32,
    pub question: String,
    pub repo_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_head: Option<String>,
    /// Anchors used for deterministic retrieval.
    pub anchors: Vec<String>,
    /// Deterministic investigation document (candidates, structure, docs).
    pub investigation: InvestigationDocument,
    /// Time-ordered events related to top candidates / issue.
    pub chronology: Vec<ChronologyEvent>,
    /// Module names under src/modules when available (empty if none).
    pub modules_present: Vec<String>,
    /// Explicit unknowns / coverage gaps.
    pub limitations: Vec<String>,
    /// Cap notes (how the packet was bounded).
    pub bounds: Vec<String>,
    /// C4-ER: ranked evidence for the model (not a dump).
    #[serde(default)]
    pub ranked_evidence: Vec<RankedEvidenceItem>,
    /// C4-ER: temporal supersession relations among chronology events.
    #[serde(default)]
    pub supersession: Vec<SupersessionNote>,
    /// C4-ER: verification notes applied to AI claims this packet will judge.
    #[serde(default)]
    pub verification_policy: Vec<String>,
}

/// Status after Atlas verification of an AI-proposed claim.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    /// All cited evidence refs resolve inside the packet.
    Supported,
    /// Packet contains evidence that conflicts with the statement (path missing, etc.).
    Contradicted,
    /// Partially grounded or interpretive; not fully checkable deterministically.
    Plausible,
    /// No usable evidence refs or not verifiable.
    Unresolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedClaim {
    pub id: String,
    pub subject: String,
    pub statement: String,
    /// e.g. "structural" | "historical" | "documentary" | "derived" | "causal"
    pub kind: String,
    pub evidence_refs: Vec<EvidenceRef>,
    pub method: String,
    #[serde(default)]
    pub temporal_scope: String,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub status: ClaimStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hypothesis {
    pub id: String,
    pub statement: String,
    pub status: ClaimStatus,
    pub supporting: Vec<EvidenceRef>,
    pub contradicting: Vec<EvidenceRef>,
    #[serde(default)]
    pub claims: Vec<ProposedClaim>,
}

/// Structured response expected from a reasoning provider (local AI).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiReasoningResponse {
    #[serde(default)]
    pub hypotheses: Vec<Hypothesis>,
    /// Paths/modules/symbols the model wants Atlas to retrieve next.
    #[serde(default)]
    pub requested_subjects: Vec<String>,
    #[serde(default)]
    pub questions: Vec<String>,
    #[serde(default)]
    pub proposed_claims: Vec<ProposedClaim>,
    #[serde(default)]
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationRound {
    pub round: u32,
    pub purpose: String,
    /// True when a model was invoked this round.
    pub ai_invoked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_ai_response: Option<AiReasoningResponse>,
    pub verified_claims: Vec<ProposedClaim>,
}

// ─── Section C — Map / Focus / Impact (v0.9a — claim-oriented orientation) ──
//
// Composes B1–B10 + inspect/structural/history into product surfaces.
// Epistemic layers are explicit: observed | derived | inferred | unknown.
// Not architectural meaning models (Section D).  Transient only.

/// How strongly Atlas can stand behind a map/focus/impact statement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EpistemicLayer {
    /// Directly represented in stored evidence (counts, edges, paths).
    Observed,
    /// Deterministic aggregation/threshold over observed evidence.
    Derived,
    /// Interpretive ranking/heuristic (still must cite evidence; not LLM fact).
    Inferred,
    /// Explicit coverage gap.
    Unknown,
}

/// A single orientation claim for Map/Focus/Impact (distinct from AI ProposedClaim).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrientationClaim {
    pub id: String,
    pub subject: String,
    pub statement: String,
    pub layer: EpistemicLayer,
    pub evidence: Vec<EvidenceRef>,
    pub method: String,
    #[serde(default)]
    pub limitations: Vec<String>,
}

/// Dimensions used when ranking impact / evidence relevance (deterministic inputs).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceDimensions {
    /// 0–1: path/module/symbol directness to the subject.
    pub subject_relevance: f32,
    /// 0–1: recency weight from timestamps when available (0 if unknown).
    pub temporal_recency: f32,
    /// 0–1: structural graph connectivity to subject.
    pub structural_connectivity: f32,
    /// 0–1: co-change / historical co-touch.
    pub historical_cochange: f32,
    /// 0–1: presence of corroborating independent sources (tests, docs, PRs).
    pub corroboration: f32,
    /// Explicit note: provenance class of primary evidence.
    pub provenance_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactNeighbor {
    pub path: String,
    pub reasons: Vec<String>,
    pub layer: EpistemicLayer,
    pub dimensions: EvidenceDimensions,
    /// Composite sort key (not a claim of importance beyond method).
    pub rank_score: f32,
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapReport {
    pub schema_version: u32,
    pub repo_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_head: Option<String>,
    /// Parent used for module discovery (src/modules or src fallback).
    pub modules_subject: String,
    pub modules: Vec<String>,
    pub claims: Vec<OrientationClaim>,
    pub hot_files: Vec<(String, i64)>,
    pub top_coupling: Vec<(String, String, usize)>,
    pub config_artifacts: Vec<String>,
    pub coverage_notes: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusReport {
    pub schema_version: u32,
    pub subject: String,
    pub subject_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect_note: Option<HistoricalRedirect>,
    pub claims: Vec<OrientationClaim>,
    pub incoming: Vec<String>,
    pub outgoing: Vec<String>,
    pub related_tests: Vec<String>,
    pub packages_observed: Vec<String>,
    pub recent_commits: Vec<String>,
    pub authors: Vec<String>,
    pub related_docs: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactReport {
    pub schema_version: u32,
    pub subject: String,
    pub neighbors: Vec<ImpactNeighbor>,
    pub claims: Vec<OrientationClaim>,
    pub dimensions_methodology: Vec<String>,
    pub limitations: Vec<String>,
}

/// Final product of the evidence → (optional local AI) → verify loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningInvestigationResult {
    pub schema_version: u32,
    pub question: String,
    /// "local_ai" | "deterministic_only"
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub packet: EvidencePacket,
    pub rounds: Vec<InvestigationRound>,
    pub hypotheses: Vec<Hypothesis>,
    pub claims: Vec<ProposedClaim>,
    pub likely_area: Vec<String>,
    pub chronology: Vec<ChronologyEvent>,
    pub affected_components: Vec<String>,
    pub relevant_issues_prs: Vec<String>,
    pub what_atlas_knows: Vec<String>,
    pub what_atlas_does_not_know: Vec<String>,
    pub next_investigation: Vec<String>,
    /// Human-readable synthesis from AI when present; never stored as fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
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

            parents:       vec![],        };
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
