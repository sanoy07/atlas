use anyhow::Result;
use atlas_connectors::Connector;
use atlas_git::{GitHubIssueConnector, GitHubPrConnector, GitRepo};
use atlas_parser::{gh_json, git_log};
use atlas_storage::Store;
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

pub fn ingest_github(repo_path: &str, store: &Store) -> Result<usize> {
    let pr_conn = GitHubPrConnector::new(repo_path);
    let prs     = gh_json::parse_prs(&pr_conn.fetch_raw()?.data)?;
    let pr_count = prs.len();

    info!("connector={} parsed={} PRs", pr_conn.name(), pr_count);
    for pr in &prs {
        store.insert_pull_request(pr, repo_path)?;
    }

    let issue_conn = GitHubIssueConnector::new(repo_path);
    let issues     = gh_json::parse_issues(&issue_conn.fetch_raw()?.data)?;

    info!("connector={} parsed={} issues", issue_conn.name(), issues.len());
    for issue in &issues {
        store.insert_issue(issue, repo_path)?;
    }

    Ok(pr_count)
}
