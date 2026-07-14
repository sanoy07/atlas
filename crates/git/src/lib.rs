pub mod github;
pub use github::{GitHubIssueConnector, GitHubPrConnector};

use anyhow::{Context, Result};
use atlas_connectors::{Capability, Connector, RawPayload};
use atlas_ir::EntityKind;
use std::process::Command;

pub struct GitRepo {
    pub path: String,
}

impl GitRepo {
    pub fn open(path: &str) -> Result<Self> {
        let out = Command::new("git")
            .args(["-C", path, "rev-parse", "--git-dir"])
            .output()
            .context("git not found in PATH")?;

        anyhow::ensure!(out.status.success(), "not a git repository: {}", path);
        Ok(Self { path: path.to_string() })
    }

    pub fn log_raw(&self, limit: usize) -> Result<String> {
        let out = Command::new("git")
            .args([
                "-C",
                &self.path,
                "log",
                &format!("--max-count={}", limit),
                "--format=\x1e%H\x1f%h\x1f%an\x1f%ae\x1f%at\x1f%s",
                "--name-only",
            ])
            .output()
            .context("git log failed")?;

        Ok(String::from_utf8(out.stdout)?)
    }

    /// Run `git log --name-status -M` and return the raw output for rename parsing.
    ///
    /// Format: each commit begins with `\x1e<full_hash>`, followed by name-status
    /// lines (`R{score}\t{old}\t{new}` for renames, `M`, `A`, `D`, etc. for others).
    /// The caller is responsible for filtering to only the R lines.
    pub fn log_renames_raw(&self) -> Result<String> {
        let out = Command::new("git")
            .args([
                "-C",
                &self.path,
                "log",
                "--format=\x1e%H",
                "--name-status",
                "-M50",
            ])
            .output()
            .context("git log --name-status failed")?;

        Ok(String::from_utf8(out.stdout)?)
    }

    pub fn remote_url(&self) -> Option<String> {
        Command::new("git")
            .args(["-C", &self.path, "remote", "get-url", "origin"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

impl Connector for GitRepo {
    fn name(&self) -> &str {
        "git"
    }

    fn capability(&self) -> Capability {
        Capability {
            name:     "Repository History",
            produces: vec![EntityKind::Commit, EntityKind::File, EntityKind::Author],
        }
    }

    fn fetch_raw(&self) -> Result<RawPayload> {
        Ok(RawPayload { data: self.log_raw(10_000)? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_rejects_non_repo() {
        assert!(GitRepo::open("/tmp").is_err());
    }

    #[test]
    fn git_connector_identity() {
        let repo = GitRepo::open(".").unwrap();
        assert_eq!(repo.name(), "git");
        assert_eq!(repo.capability().name, "Repository History");
        assert!(!repo.capability().produces.is_empty());
    }
}
