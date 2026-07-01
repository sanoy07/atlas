use anyhow::{Context, Result};
use std::process::Command;

pub struct GitHub {
    pub repo_path: String,
}

impl GitHub {
    pub fn new(repo_path: &str) -> Self {
        Self { repo_path: repo_path.to_string() }
    }

    pub fn pull_requests_raw(&self) -> Result<String> {
        let out = Command::new("gh")
            .args([
                "pr", "list",
                "--state", "all",
                "--limit", "500",
                "--json", "number,title,state,body,author,mergeCommit",
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

    pub fn issues_raw(&self) -> Result<String> {
        let out = Command::new("gh")
            .args([
                "issue", "list",
                "--state", "all",
                "--limit", "500",
                "--json", "number,title,state,body,author",
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
