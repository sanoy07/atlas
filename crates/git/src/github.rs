use anyhow::{Context, Result};
use atlas_connectors::{Capability, Connector, RawPayload};
use atlas_ir::EntityKind;
use serde::Deserialize;
use std::process::Command;

// ─── Lightweight structs for enrichment ──────────────────────────────────────

#[derive(Deserialize)]
struct PrListNumber {
    number: i64,
}

#[derive(Deserialize)]
struct IssueListNumber {
    number: i64,
}

// ─── GitHub PR connector ──────────────────────────────────────────────────────

pub struct GitHubPrConnector {
    pub repo_path: String,
}

impl GitHubPrConnector {
    pub fn new(repo_path: &str) -> Self {
        Self { repo_path: repo_path.to_string() }
    }

    fn pull_requests_raw(&self) -> Result<String> {
        // Step 1: bulk list — all fields supported by `gh pr list --json`
        let list_out = Command::new("gh")
            .args([
                "pr", "list",
                "--state", "all",
                "--limit", "500",
                "--json", "number,title,state,body,author,mergeCommit,closingIssuesReferences,createdAt,mergedAt",
            ])
            .current_dir(&self.repo_path)
            .output()
            .context("gh not found — run: nix profile install nixpkgs#gh")?;

        anyhow::ensure!(
            list_out.status.success(),
            "gh pr list failed: {}",
            String::from_utf8_lossy(&list_out.stderr)
        );

        let list_json = String::from_utf8(list_out.stdout)?;

        // Step 2: extract PR numbers for enrichment calls
        let numbers: Vec<PrListNumber> = serde_json::from_str(&list_json)
            .context("failed to parse gh pr list output")?;

        if numbers.is_empty() {
            return Ok(list_json);
        }

        // Step 3: parse list into mutable JSON values so we can inject reviews/comments
        let mut prs: Vec<serde_json::Value> = serde_json::from_str(&list_json)
            .context("failed to re-parse gh pr list as JSON values")?;

        // Step 4: per-PR enrichment — comments, reviews, review decision
        // gh pr view supports these fields; gh pr list does not.
        for (pr, item) in prs.iter_mut().zip(numbers.iter()) {
            let detail_out = Command::new("gh")
                .args([
                    "pr", "view",
                    &item.number.to_string(),
                    "--json", "comments,reviews,reviewDecision",
                ])
                .current_dir(&self.repo_path)
                .output();

            if let Ok(out) = detail_out {
                if out.status.success() {
                    if let Ok(detail) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                        if let Some(reviews) = detail.get("reviews") {
                            pr["reviews"] = reviews.clone();
                        }
                        if let Some(comments) = detail.get("comments") {
                            pr["comments"] = comments.clone();
                        }
                        if let Some(decision) = detail.get("reviewDecision") {
                            pr["reviewDecision"] = decision.clone();
                        }
                    }
                }
            }
            // Non-fatal: if enrichment fails for a PR, the base list data is still used.
        }

        serde_json::to_string(&prs).context("failed to serialize enriched PR list")
    }
}

impl Connector for GitHubPrConnector {
    fn name(&self) -> &str {
        "github-pr"
    }

    fn capability(&self) -> Capability {
        Capability {
            name:     "Collaboration Metadata",
            produces: vec![EntityKind::PullRequest, EntityKind::Author],
        }
    }

    fn fetch_raw(&self) -> Result<RawPayload> {
        Ok(RawPayload { data: self.pull_requests_raw()? })
    }
}

// ─── GitHub Issue connector ───────────────────────────────────────────────────

pub struct GitHubIssueConnector {
    pub repo_path: String,
}

impl GitHubIssueConnector {
    pub fn new(repo_path: &str) -> Self {
        Self { repo_path: repo_path.to_string() }
    }

    fn issues_raw(&self) -> Result<String> {
        // Step 1: bulk list
        let list_out = Command::new("gh")
            .args([
                "issue", "list",
                "--state", "all",
                "--limit", "500",
                "--json", "number,title,state,body,author,createdAt",
            ])
            .current_dir(&self.repo_path)
            .output()
            .context("gh not found")?;

        anyhow::ensure!(
            list_out.status.success(),
            "gh issue list failed: {}",
            String::from_utf8_lossy(&list_out.stderr)
        );

        let list_json = String::from_utf8(list_out.stdout)?;

        // Step 2: extract issue numbers
        let numbers: Vec<IssueListNumber> = serde_json::from_str(&list_json)
            .context("failed to parse gh issue list output")?;

        if numbers.is_empty() {
            return Ok(list_json);
        }

        let mut issues: Vec<serde_json::Value> = serde_json::from_str(&list_json)
            .context("failed to re-parse gh issue list as JSON values")?;

        // Step 3: per-issue enrichment — comments
        for (issue, item) in issues.iter_mut().zip(numbers.iter()) {
            let detail_out = Command::new("gh")
                .args([
                    "issue", "view",
                    &item.number.to_string(),
                    "--json", "comments",
                ])
                .current_dir(&self.repo_path)
                .output();

            if let Ok(out) = detail_out {
                if out.status.success() {
                    if let Ok(detail) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                        if let Some(comments) = detail.get("comments") {
                            issue["comments"] = comments.clone();
                        }
                    }
                }
            }
        }

        serde_json::to_string(&issues).context("failed to serialize enriched issue list")
    }
}

impl Connector for GitHubIssueConnector {
    fn name(&self) -> &str {
        "github-issue"
    }

    fn capability(&self) -> Capability {
        Capability {
            name:     "Issue Tracking",
            produces: vec![EntityKind::Issue, EntityKind::Author],
        }
    }

    fn fetch_raw(&self) -> Result<RawPayload> {
        Ok(RawPayload { data: self.issues_raw()? })
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_connector_identity() {
        let conn = GitHubPrConnector::new(".");
        assert_eq!(conn.name(), "github-pr");
        assert_eq!(conn.capability().name, "Collaboration Metadata");
        assert!(!conn.capability().produces.is_empty());
    }

    #[test]
    fn issue_connector_identity() {
        let conn = GitHubIssueConnector::new(".");
        assert_eq!(conn.name(), "github-issue");
        assert_eq!(conn.capability().name, "Issue Tracking");
        assert!(!conn.capability().produces.is_empty());
    }
}
