use anyhow::{Context, Result};
use atlas_connectors::{Capability, Connector, RawPayload};
use atlas_ir::EntityKind;
use std::process::Command;

// ─── GitHub PR connector ──────────────────────────────────────────────────────

pub struct GitHubPrConnector {
    pub repo_path: String,
}

impl GitHubPrConnector {
    pub fn new(repo_path: &str) -> Self {
        Self { repo_path: repo_path.to_string() }
    }

    fn pull_requests_raw(&self) -> Result<String> {
        let out = Command::new("gh")
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
            out.status.success(),
            "gh pr list failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        Ok(String::from_utf8(out.stdout)?)
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
        let out = Command::new("gh")
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
            out.status.success(),
            "gh issue list failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        Ok(String::from_utf8(out.stdout)?)
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
