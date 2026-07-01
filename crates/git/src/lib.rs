pub mod github;
pub use github::GitHub;

use anyhow::{Context, Result};
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
                // \x1e = ASCII record separator; safe inside git pretty-format
                "--format=\x1e%H\x1f%h\x1f%an\x1f%ae\x1f%at\x1f%s",
                "--name-only",
            ])
            .output()
            .context("git log failed")?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_rejects_non_repo() {
        let result = GitRepo::open("/tmp");
        assert!(result.is_err());
    }
}
