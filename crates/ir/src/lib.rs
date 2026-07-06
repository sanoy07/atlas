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
    pub first_commit: Option<CommitSummary>,
    pub last_commit:  Option<CommitSummary>,
    pub touch_count:  i64,
}

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
    pub git_history:   CoverageStatus,
    pub github_prs:    CoverageStatus,
    pub github_issues: CoverageStatus,
    pub documentation: CoverageStatus,
    pub working_tree:  CoverageStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum CoverageStatus {
    /// Source was ingested and has data.
    Available,
    /// Source is recognised but not yet ingested.
    NotIngested,
    /// Detectable only through co-change proximity, not direct ingestion.
    CoChangeOnly,
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
