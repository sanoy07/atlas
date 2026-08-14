//! Freshness of the evidence graph relative to the working repository.
//!
//! Atlas's structural graph is a snapshot taken at the last `atlas ingest`.
//! Nothing invalidates it when the repository moves on, so a graph that
//! predates HEAD keeps answering structural questions with yesterday's code —
//! silently, and with exactly the same confident formatting as a fresh one.
//! That is the one failure mode where Atlas violates its own epistemic
//! invariant: it states a relationship with more certainty than the evidence
//! supports, because the evidence is no longer about the current tree.
//!
//! The signal is the git HEAD recorded in `ingest_runs` versus the HEAD now.
//! It is deterministic, cheap, and requires no schema change.
//!
//! Deliberately *not* covered: uncommitted working-tree edits. Structural
//! extraction reads the working tree, so an uncommitted refactor does age the
//! graph — but a dirty tree is the normal state during development, and
//! warning on it would train users to ignore the warning.

use anyhow::Result;
use atlas_git::GitRepo;
use atlas_storage::Store;
use serde::Serialize;

/// What the recorded ingest HEAD says about the graph's currency.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Freshness {
    /// No ingest run recorded for this repository.
    NeverIngested,
    /// Ingested HEAD matches the current HEAD.
    Current { head: String },
    /// The repository has moved on since the graph was built.
    Stale {
        ingested_head: String,
        current_head:  String,
        /// `None` when the ingested commit is unreachable (rebase, force-push,
        /// or a DB built from a different clone) — an unknown gap, not zero.
        commits_behind: Option<usize>,
    },
    /// Freshness could not be established; the reason is reportable, not silent.
    Unknown { reason: String },
}

/// Freshness plus the provenance a user needs to act on it.
#[derive(Debug, Clone, Serialize)]
pub struct FreshnessReport {
    pub repo_path:     String,
    pub freshness:     Freshness,
    /// Unix seconds of the last ingest run's start, when one exists.
    pub ingested_at:   Option<i64>,
    pub atlas_version: Option<String>,
    pub git_branch:    Option<String>,
}

impl FreshnessReport {
    /// True when query results may not describe the current tree.
    pub fn is_stale(&self) -> bool {
        matches!(
            self.freshness,
            Freshness::Stale { .. } | Freshness::NeverIngested
        )
    }

    /// A one-line warning for consumers that must not present stale evidence
    /// as current, or `None` when the graph is trustworthy.
    ///
    /// Returned rather than printed so both the CLI and any future machine
    /// consumer render it in their own idiom.
    pub fn warning(&self) -> Option<String> {
        match &self.freshness {
            Freshness::Current { .. } | Freshness::Unknown { .. } => None,
            Freshness::NeverIngested => Some(
                "no evidence graph for this repository — run `atlas ingest . --typescript`"
                    .to_string(),
            ),
            Freshness::Stale { commits_behind, ingested_head, .. } => {
                let gap = match commits_behind {
                    Some(n) => format!("{} commit(s) behind", n),
                    None => format!(
                        "built at {}, which is no longer reachable",
                        short(ingested_head)
                    ),
                };
                Some(format!(
                    "evidence graph is {} — re-run `atlas ingest . --typescript` for current results",
                    gap
                ))
            }
        }
    }
}

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(8)]
}

/// Compare the graph's recorded ingest HEAD against the repository's HEAD.
pub fn compute_freshness(repo_path: &str, store: &Store) -> Result<FreshnessReport> {
    let run = store.latest_ingest_run(repo_path)?;

    let (ingested_at, atlas_version, git_branch, ingested_head) = match &run {
        Some(r) => (
            Some(r.started_at),
            Some(r.atlas_version.clone()),
            r.git_branch.clone(),
            r.git_head.clone(),
        ),
        None => (None, None, None, None),
    };

    let freshness = match (&run, GitRepo::open(repo_path).ok()) {
        (None, _) => Freshness::NeverIngested,
        (Some(_), None) => Freshness::Unknown {
            reason: "not a readable git repository".to_string(),
        },
        (Some(_), Some(git)) => match (ingested_head.as_deref(), git.head_commit()) {
            (Some(ingested), Some(current)) if ingested == current => {
                Freshness::Current { head: current }
            }
            (Some(ingested), Some(current)) => Freshness::Stale {
                commits_behind: git.commits_ahead_of(ingested),
                ingested_head:  ingested.to_string(),
                current_head:   current,
            },
            (None, _) => Freshness::Unknown {
                reason: "last ingest recorded no git HEAD".to_string(),
            },
            (_, None) => Freshness::Unknown {
                reason: "cannot read current git HEAD".to_string(),
            },
        },
    };

    Ok(FreshnessReport {
        repo_path: repo_path.to_string(),
        freshness,
        ingested_at,
        atlas_version,
        git_branch,
    })
}
