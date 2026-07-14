use anyhow::Result;
use atlas_ir::{Issue, PullRequest};
use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Deserialize)]
struct RawPr {
    number:       i64,
    title:        String,
    state:        String,
    body:         Option<String>,
    author:       RawAuthor,
    #[serde(rename = "mergeCommit")]
    merge_commit: Option<RawCommit>,
    #[serde(rename = "closingIssuesReferences", default)]
    closing_issues: Vec<RawIssueRef>,
    #[serde(rename = "createdAt", default)]
    created_at: Option<String>,
    #[serde(rename = "mergedAt", default)]
    merged_at: Option<String>,
}

#[derive(Deserialize)]
struct RawIssue {
    number: i64,
    title:  String,
    state:  String,
    body:   Option<String>,
    author: RawAuthor,
    #[serde(rename = "createdAt", default)]
    created_at: Option<String>,
}

#[derive(Deserialize)]
struct RawAuthor {
    login: String,
}

#[derive(Deserialize)]
struct RawCommit {
    oid: String,
}

#[derive(Deserialize)]
struct RawIssueRef {
    number: i64,
}

/// Parsed output of `gh pr view <number> --json title,body,closingIssuesReferences,files`.
pub struct PrDetail {
    pub title:                 String,
    pub body:                  String,
    pub linked_issue_numbers:  Vec<i64>,
    pub changed_files:         Vec<String>,
}

#[derive(Deserialize)]
struct RawPrView {
    title:   String,
    body:    Option<String>,
    #[serde(rename = "closingIssuesReferences", default)]
    closing_issues: Vec<RawIssueRef>,
    #[serde(default)]
    files: Vec<RawPrFile>,
}

#[derive(Deserialize)]
struct RawPrFile {
    path: String,
}

/// Parse a single-PR JSON object produced by `gh pr view <n> --json ...`.
pub fn parse_pr_detail(json: &str) -> Result<PrDetail> {
    let raw: RawPrView = serde_json::from_str(json)?;
    Ok(PrDetail {
        title:                raw.title,
        body:                 raw.body.unwrap_or_default(),
        linked_issue_numbers: raw.closing_issues.into_iter().map(|i| i.number).collect(),
        changed_files:        raw.files.into_iter().map(|f| f.path).collect(),
    })
}

fn parse_gh_timestamp(s: &Option<String>) -> Option<DateTime<Utc>> {
    s.as_deref()
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

pub fn parse_prs(json: &str) -> Result<Vec<PullRequest>> {
    let raw: Vec<RawPr> = serde_json::from_str(json)?;
    Ok(raw
        .into_iter()
        .map(|p| PullRequest {
            number:           p.number,
            title:            p.title,
            state:            p.state,
            body:             p.body,
            author:           p.author.login,
            merge_commit_sha: p.merge_commit.map(|c| c.oid),
            created_at:       parse_gh_timestamp(&p.created_at),
            merged_at:        parse_gh_timestamp(&p.merged_at),
        })
        .collect())
}

/// Returns `(pr_number, issue_number)` pairs extracted from `closingIssuesReferences`.
pub fn parse_pr_issue_links(json: &str) -> Result<Vec<(i64, i64)>> {
    let raw: Vec<RawPr> = serde_json::from_str(json)?;
    Ok(raw
        .into_iter()
        .flat_map(|pr| {
            let pr_number = pr.number;
            pr.closing_issues
                .into_iter()
                .map(move |issue| (pr_number, issue.number))
        })
        .collect())
}

pub fn parse_issues(json: &str) -> Result<Vec<Issue>> {
    let raw: Vec<RawIssue> = serde_json::from_str(json)?;
    Ok(raw
        .into_iter()
        .map(|i| Issue {
            number:     i.number,
            title:      i.title,
            state:      i.state,
            body:       i.body,
            author:     i.author.login,
            created_at: parse_gh_timestamp(&i.created_at),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Canonical fixture JSON — mirrors the fixture repo used in integration tests.
    const FIXTURE_PRS: &str = r#"[{
        "number": 12,
        "title": "Add user authentication",
        "state": "MERGED",
        "body": "Closes #10",
        "author": {"login": "alice"},
        "mergeCommit": {"oid": "bbbbbbbbbbbb"},
        "closingIssuesReferences": [{"number": 10}],
        "createdAt": "2023-10-01T10:00:00Z",
        "mergedAt": "2023-10-03T16:45:00Z"
    }]"#;

    const FIXTURE_ISSUES: &str = r#"[{
        "number": 10,
        "title": "Add user authentication",
        "state": "CLOSED",
        "body": null,
        "author": {"login": "alice"},
        "createdAt": "2023-10-01T09:00:00Z"
    }]"#;

    #[test]
    fn fixture_pr_parses_correctly() {
        let prs = parse_prs(FIXTURE_PRS).unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 12);
        assert_eq!(prs[0].title, "Add user authentication");
        assert_eq!(prs[0].state, "MERGED");
        assert_eq!(prs[0].author, "alice");
        assert_eq!(prs[0].merge_commit_sha, Some("bbbbbbbbbbbb".into()));
        assert!(prs[0].created_at.is_some(), "created_at should be parsed");
        assert!(prs[0].merged_at.is_some(), "merged_at should be parsed");
    }

    #[test]
    fn pr_timestamps_round_trip_correctly() {
        let prs = parse_prs(FIXTURE_PRS).unwrap();
        // created_at should be 2023-10-01T10:00:00Z → unix 1696154400
        assert_eq!(prs[0].created_at.unwrap().timestamp(), 1_696_154_400);
        // merged_at should be 2023-10-03T16:45:00Z → unix 1696351500
        assert_eq!(prs[0].merged_at.unwrap().timestamp(), 1_696_351_500);
    }

    #[test]
    fn issue_timestamp_parsed() {
        let issues = parse_issues(FIXTURE_ISSUES).unwrap();
        assert!(issues[0].created_at.is_some(), "created_at should be parsed");
        assert_eq!(issues[0].created_at.unwrap().timestamp(), 1_696_150_800);
    }

    #[test]
    fn fixture_issue_links_parsed() {
        let links = parse_pr_issue_links(FIXTURE_PRS).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], (12, 10));
    }

    #[test]
    fn fixture_issues_parse_correctly() {
        let issues = parse_issues(FIXTURE_ISSUES).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].number, 10);
        assert_eq!(issues[0].title, "Add user authentication");
        assert_eq!(issues[0].state, "CLOSED");
        assert_eq!(issues[0].author, "alice");
    }

    #[test]
    fn parses_pr_with_merge_commit() {
        let json = r#"[{
            "number": 42,
            "title": "Add feature",
            "state": "MERGED",
            "body": null,
            "author": {"login": "alice"},
            "mergeCommit": {"oid": "abc123"}
        }]"#;
        let prs = parse_prs(json).unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 42);
        assert_eq!(prs[0].merge_commit_sha, Some("abc123".into()));
    }

    #[test]
    fn parses_pr_without_merge_commit() {
        let json = r#"[{
            "number": 1,
            "title": "Draft",
            "state": "OPEN",
            "body": null,
            "author": {"login": "bob"},
            "mergeCommit": null
        }]"#;
        let prs = parse_prs(json).unwrap();
        assert!(prs[0].merge_commit_sha.is_none());
    }

    #[test]
    fn pr_without_closing_issues_field_returns_empty_links() {
        let json = r#"[{
            "number": 5,
            "title": "Refactor",
            "state": "MERGED",
            "body": null,
            "author": {"login": "bob"},
            "mergeCommit": {"oid": "def456"}
        }]"#;
        let links = parse_pr_issue_links(json).unwrap();
        assert!(links.is_empty());
    }

    #[test]
    fn malformed_json_returns_error() {
        let result = parse_prs("{not valid json}");
        assert!(result.is_err());
    }

    #[test]
    fn parses_issues() {
        let json = r#"[{
            "number": 7,
            "title": "Bug report",
            "state": "OPEN",
            "body": "details",
            "author": {"login": "carol"}
        }]"#;
        let issues = parse_issues(json).unwrap();
        assert_eq!(issues[0].title, "Bug report");
        assert_eq!(issues[0].author, "carol");
    }
}
