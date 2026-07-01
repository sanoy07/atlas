use anyhow::Result;
use atlas_ir::{Issue, PullRequest};
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
}

#[derive(Deserialize)]
struct RawIssue {
    number: i64,
    title:  String,
    state:  String,
    body:   Option<String>,
    author: RawAuthor,
}

#[derive(Deserialize)]
struct RawAuthor {
    login: String,
}

#[derive(Deserialize)]
struct RawCommit {
    oid: String,
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
        })
        .collect())
}

pub fn parse_issues(json: &str) -> Result<Vec<Issue>> {
    let raw: Vec<RawIssue> = serde_json::from_str(json)?;
    Ok(raw
        .into_iter()
        .map(|i| Issue {
            number: i.number,
            title:  i.title,
            state:  i.state,
            body:   i.body,
            author: i.author.login,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

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
