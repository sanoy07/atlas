use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id:    String,
    pub kind:  EntityKind,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub number: i64,
    pub title:  String,
    pub state:  String,
    pub body:   Option<String>,
    pub author: String,
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
        };
        let back: PullRequest = serde_json::from_str(&serde_json::to_string(&pr).unwrap()).unwrap();
        assert_eq!(back.number, 1);
        assert_eq!(back.merge_commit_sha, Some("def456".into()));
    }
}
