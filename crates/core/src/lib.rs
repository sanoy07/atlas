use anyhow::Result;
use atlas_git::{GitHub, GitRepo};
use atlas_parser::{gh_json, git_log};
use atlas_storage::Store;
use tracing::info;

pub fn ingest_git(repo_path: &str, store: &Store) -> Result<usize> {
    let repo = GitRepo::open(repo_path)?;
    let raw = repo.log_raw(10_000)?;
    let commits = git_log::parse(&raw)?;
    let count = commits.len();

    info!("parsed {} commits from {}", count, repo_path);

    for commit in &commits {
        store.insert_commit(commit, repo_path)?;
    }

    Ok(count)
}

pub fn ingest_github(repo_path: &str, store: &Store) -> Result<usize> {
    let gh = GitHub::new(repo_path);

    let pr_raw = gh.pull_requests_raw()?;
    let prs = gh_json::parse_prs(&pr_raw)?;
    let pr_count = prs.len();
    for pr in &prs {
        store.insert_pull_request(pr, repo_path)?;
    }

    let issue_raw = gh.issues_raw()?;
    let issues = gh_json::parse_issues(&issue_raw)?;
    for issue in &issues {
        store.insert_issue(issue, repo_path)?;
    }

    info!("ingested {} PRs, {} issues from GitHub", pr_count, issues.len());
    Ok(pr_count)
}
