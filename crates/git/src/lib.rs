pub mod github;
pub use github::{GitHubIssueConnector, GitHubPrConnector};

use anyhow::{Context, Result};
use atlas_connectors::{Capability, Connector, RawPayload};
use atlas_ir::EntityKind;
use std::process::Command;

/// Which portion of the git history to ingest.
///
/// Callers must pick explicitly rather than getting a silent default whose
/// completeness they cannot audit.  The chosen scope is recorded per run
/// (see `ingest_runs.requested_scope`) so downstream queries know exactly
/// what portion of history is present in the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitScope {
    /// Only commits reachable from the current HEAD.  Default for
    /// `atlas ingest .`; historically the only supported mode.
    HeadOnly,
    /// All commits reachable from any local ref (branches + tags).
    /// Corresponds to `git log --all`.
    AllRefs,
}

impl GitScope {
    pub fn as_str(&self) -> &'static str {
        match self { Self::HeadOnly => "head_only", Self::AllRefs => "all_refs" }
    }
}

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

    /// Return the current branch name, or `None` if the repo is in detached
    /// HEAD state or the command fails.
    pub fn current_branch(&self) -> Option<String> {
        Command::new("git")
            .args(["-C", &self.path, "symbolic-ref", "--short", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Return the current HEAD commit hash, or `None` if the command fails.
    pub fn head_commit(&self) -> Option<String> {
        Command::new("git")
            .args(["-C", &self.path, "rev-parse", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Count commits reachable from HEAD but not from `commit`.
    ///
    /// Returns `None` when `commit` is not present in this repository (a
    /// rebase, a force-push, or a database ingested from a different clone),
    /// which callers must distinguish from a genuine zero.
    pub fn commits_ahead_of(&self, commit: &str) -> Option<usize> {
        let range = format!("{}..HEAD", commit);
        Command::new("git")
            .args(["-C", &self.path, "rev-list", "--count", &range])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse().ok())
    }

    /// Return the number of commits `git log` would report at the given scope,
    /// so callers can decide whether the hard-coded ingestion cap will truncate.
    pub fn commit_count(&self, scope: GitScope) -> Result<usize> {
        let mut args = vec!["-C", &self.path, "rev-list", "--count"];
        args.push(if scope == GitScope::AllRefs { "--all" } else { "HEAD" });
        let out = Command::new("git").args(&args).output().context("git rev-list --count failed")?;
        let text = String::from_utf8(out.stdout)?;
        Ok(text.trim().parse().unwrap_or(0))
    }

    /// Historical entry point — HeadOnly scope with a legacy 10,000 cap.
    /// Prefer `log_raw_scoped` for new code.
    pub fn log_raw(&self, limit: usize) -> Result<String> {
        self.log_raw_scoped(GitScope::HeadOnly, limit)
    }

    /// Fetch git log at the requested scope, capped at `limit` commits.
    ///
    /// Format:
    ///   `\x1e<full_hash>\x1f<short_hash>\x1f<parents>\x1f<author>\x1f<email>\x1f<ts>\x1f<subject>`
    ///   then a blank line, then one file path per line.
    ///
    /// `<parents>` is a space-separated list of parent hashes (empty for root
    /// commits, one hash for normal commits, two+ for merges).
    pub fn log_raw_scoped(&self, scope: GitScope, limit: usize) -> Result<String> {
        let scope_arg = match scope { GitScope::HeadOnly => "HEAD", GitScope::AllRefs => "--all" };
        let out = Command::new("git")
            .args([
                "-C",
                &self.path,
                "log",
                scope_arg,
                &format!("--max-count={}", limit),
                "--format=\x1e%H\x1f%h\x1f%P\x1f%an\x1f%ae\x1f%at\x1f%s",
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
