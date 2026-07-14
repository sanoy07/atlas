use anyhow::Result;
use atlas_ir::{
    AccessState, Commit, ExistenceSource, IngestionState, Issue, ProfileClaim, ProfileClaimKind,
    ProjectRecord, PullRequest, RenameEvidence, RepositoryRecord, StructuralEdge, StructuralEdgeKind,
};
use rusqlite::{params, Connection, OptionalExtension};
use tracing::debug;

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA)?;
        // Add columns that didn't exist in the initial schema; errors are ignored
        // because SQLite has no ALTER TABLE ADD COLUMN IF NOT EXISTS.
        for stmt in [
            "ALTER TABLE pull_requests ADD COLUMN created_at INTEGER",
            "ALTER TABLE pull_requests ADD COLUMN merged_at  INTEGER",
            "ALTER TABLE issues        ADD COLUMN created_at INTEGER",
        ] {
            let _ = self.conn.execute(stmt, []);
        }
        debug!("schema migration complete");
        Ok(())
    }

    pub fn insert_commit(&self, commit: &Commit, repo_path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO commits
               (hash, short_hash, message, author_name, author_email, timestamp, repo_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                commit.hash,
                commit.short_hash,
                commit.message,
                commit.author_name,
                commit.author_email,
                commit.timestamp.timestamp(),
                repo_path,
            ],
        )?;

        for path in &commit.files_changed {
            self.conn.execute(
                "INSERT OR IGNORE INTO files (path, repo_path) VALUES (?1, ?2)",
                params![path, repo_path],
            )?;
            self.conn.execute(
                "INSERT OR IGNORE INTO commit_files (commit_hash, file_path) VALUES (?1, ?2)",
                params![commit.hash, path],
            )?;
        }

        Ok(())
    }

    pub fn insert_pull_request(&self, pr: &PullRequest, repo_path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO pull_requests
               (number, title, state, body, author, merge_commit_sha, repo_path, created_at, merged_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                pr.number,
                pr.title,
                pr.state,
                pr.body,
                pr.author,
                pr.merge_commit_sha,
                repo_path,
                pr.created_at.as_ref().map(|dt| dt.timestamp()),
                pr.merged_at.as_ref().map(|dt| dt.timestamp()),
            ],
        )?;
        Ok(())
    }

    pub fn insert_issue(&self, issue: &Issue, repo_path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO issues
               (number, title, state, body, author, repo_path, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                issue.number,
                issue.title,
                issue.state,
                issue.body,
                issue.author,
                repo_path,
                issue.created_at.as_ref().map(|dt| dt.timestamp()),
            ],
        )?;
        Ok(())
    }

    pub fn commits_for_file(&self, file_path: &str, repo_path: &str) -> Result<Vec<CommitRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.hash, c.short_hash, c.message, c.author_name, c.timestamp
             FROM commits c
             JOIN commit_files cf ON c.hash = cf.commit_hash
             WHERE cf.file_path = ?1 AND c.repo_path = ?2
             ORDER BY c.timestamp DESC",
        )?;

        let rows = stmt.query_map(params![file_path, repo_path], |row| {
            Ok(CommitRow {
                hash:        row.get(0)?,
                short_hash:  row.get(1)?,
                message:     row.get(2)?,
                author_name: row.get(3)?,
                timestamp:   row.get(4)?,
            })
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn prs_for_file(&self, file_path: &str, repo_path: &str) -> Result<Vec<PrRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT pr.number, pr.title, pr.state, pr.author,
                    pr.merge_commit_sha, pr.created_at, pr.merged_at
             FROM pull_requests pr
             JOIN commits c ON c.hash = pr.merge_commit_sha AND c.repo_path = pr.repo_path
             JOIN commit_files cf ON cf.commit_hash = c.hash
             WHERE cf.file_path = ?1 AND pr.repo_path = ?2
             ORDER BY pr.merged_at ASC, pr.number ASC",
        )?;

        let rows = stmt.query_map(params![file_path, repo_path], |row| {
            Ok(PrRow {
                number:           row.get(0)?,
                title:            row.get(1)?,
                state:            row.get(2)?,
                author:           row.get(3)?,
                merge_commit_sha: row.get(4)?,
                created_at:       row.get(5)?,
                merged_at:        row.get(6)?,
            })
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Files that change in the same commits as `file_path`, ordered by co-change frequency.
    /// Only returns files that co-changed at least `min_count` times.
    pub fn co_changes_for_file(
        &self,
        file_path: &str,
        repo_path: &str,
        min_count: i64,
    ) -> Result<Vec<CoChangeRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT cf2.file_path, COUNT(*) as change_count
             FROM commits c
             JOIN commit_files cf1 ON cf1.commit_hash = c.hash
             JOIN commit_files cf2 ON cf2.commit_hash = c.hash AND cf2.file_path != cf1.file_path
             WHERE cf1.file_path = ?1 AND c.repo_path = ?2
             GROUP BY cf2.file_path
             HAVING change_count >= ?3
             ORDER BY change_count DESC, cf2.file_path ASC
             LIMIT 20",
        )?;

        let rows = stmt.query_map(params![file_path, repo_path, min_count], |row| {
            Ok(CoChangeRow {
                file_path:    row.get(0)?,
                change_count: row.get(1)?,
            })
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The commit that first introduced `file_path` into this repository.
    pub fn first_seen(&self, file_path: &str, repo_path: &str) -> Result<Option<CommitRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.hash, c.short_hash, c.message, c.author_name, c.timestamp
             FROM commits c
             JOIN commit_files cf ON c.hash = cf.commit_hash
             WHERE cf.file_path = ?1 AND c.repo_path = ?2
             ORDER BY c.timestamp ASC
             LIMIT 1",
        )?;

        let mut rows = stmt.query_map(params![file_path, repo_path], |row| {
            Ok(CommitRow {
                hash:        row.get(0)?,
                short_hash:  row.get(1)?,
                message:     row.get(2)?,
                author_name: row.get(3)?,
                timestamp:   row.get(4)?,
            })
        })?;

        Ok(rows.next().transpose()?)
    }

    /// The most recent commit that touched `file_path`.
    pub fn last_seen(&self, file_path: &str, repo_path: &str) -> Result<Option<CommitRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.hash, c.short_hash, c.message, c.author_name, c.timestamp
             FROM commits c
             JOIN commit_files cf ON c.hash = cf.commit_hash
             WHERE cf.file_path = ?1 AND c.repo_path = ?2
             ORDER BY c.timestamp DESC
             LIMIT 1",
        )?;

        let mut rows = stmt.query_map(params![file_path, repo_path], |row| {
            Ok(CommitRow {
                hash:        row.get(0)?,
                short_hash:  row.get(1)?,
                message:     row.get(2)?,
                author_name: row.get(3)?,
                timestamp:   row.get(4)?,
            })
        })?;

        Ok(rows.next().transpose()?)
    }

    /// Number of commits that have touched `file_path`.
    pub fn touch_count(&self, file_path: &str, repo_path: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*)
             FROM commits c
             JOIN commit_files cf ON c.hash = cf.commit_hash
             WHERE cf.file_path = ?1 AND c.repo_path = ?2",
            params![file_path, repo_path],
            |r| r.get(0),
        )?)
    }

    /// Files ordered by how many distinct commits have touched them (most active first).
    pub fn hot_files(&self, repo_path: &str, limit: i64) -> Result<Vec<HotFileRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT cf.file_path, COUNT(*) as touch_count
             FROM commit_files cf
             JOIN commits c ON c.hash = cf.commit_hash
             WHERE c.repo_path = ?1
             GROUP BY cf.file_path
             ORDER BY touch_count DESC, cf.file_path ASC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![repo_path, limit], |row| {
            Ok(HotFileRow {
                file_path:   row.get(0)?,
                touch_count: row.get(1)?,
            })
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Commits that touched a file, ordered oldest first (for timeline view).
    pub fn commits_for_file_asc(&self, file_path: &str, repo_path: &str) -> Result<Vec<CommitRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.hash, c.short_hash, c.message, c.author_name, c.timestamp
             FROM commits c
             JOIN commit_files cf ON c.hash = cf.commit_hash
             WHERE cf.file_path = ?1 AND c.repo_path = ?2
             ORDER BY c.timestamp ASC",
        )?;

        let rows = stmt.query_map(params![file_path, repo_path], |row| {
            Ok(CommitRow {
                hash:        row.get(0)?,
                short_hash:  row.get(1)?,
                message:     row.get(2)?,
                author_name: row.get(3)?,
                timestamp:   row.get(4)?,
            })
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn commit_count(&self, repo_path: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM commits WHERE repo_path = ?1",
            params![repo_path],
            |r| r.get(0),
        )?)
    }

    pub fn pr_count(&self, repo_path: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM pull_requests WHERE repo_path = ?1",
            params![repo_path],
            |r| r.get(0),
        )?)
    }

    pub fn issue_count(&self, repo_path: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM issues WHERE repo_path = ?1",
            params![repo_path],
            |r| r.get(0),
        )?)
    }

    /// Issue numbers closed by a specific PR.
    pub fn issue_numbers_for_pr(&self, pr_number: i64, repo_path: &str) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT issue_number FROM pr_issues WHERE pr_number = ?1 AND repo_path = ?2",
        )?;
        let rows = stmt.query_map(params![pr_number, repo_path], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn link_pr_to_issue(&self, pr_number: i64, issue_number: i64, repo_path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO pr_issues (pr_number, issue_number, repo_path)
             VALUES (?1, ?2, ?3)",
            params![pr_number, issue_number, repo_path],
        )?;
        Ok(())
    }

    /// Store raw rename evidence observed from Git for a specific repository.
    /// Idempotent: inserting the same (commit, old_path, new_path, repo_path) twice is safe.
    pub fn insert_rename_evidence(&self, ev: &RenameEvidence, repo_path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO rename_evidence
               (commit_hash, old_path, new_path, similarity_score, detection_source, repo_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                ev.commit_hash,
                ev.old_path,
                ev.new_path,
                ev.similarity_score as i64,
                ev.detection_source,
                repo_path,
            ],
        )?;
        Ok(())
    }

    /// All rename evidence records for a repository, ordered by (old_path, commit_hash).
    pub fn all_rename_evidence(&self, repo_path: &str) -> Result<Vec<RenameEvidenceRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT commit_hash, old_path, new_path, similarity_score, detection_source
             FROM rename_evidence
             WHERE repo_path = ?1
             ORDER BY old_path ASC, commit_hash ASC",
        )?;

        let rows = stmt.query_map(params![repo_path], |row| {
            Ok(RenameEvidenceRow {
                commit_hash:      row.get(0)?,
                old_path:         row.get(1)?,
                new_path:         row.get(2)?,
                similarity_score: row.get::<_, i64>(3)? as u8,
                detection_source: row.get(4)?,
            })
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Rename evidence records where `path` was involved (either as source or destination).
    pub fn rename_evidence_for_path(&self, path: &str, repo_path: &str) -> Result<Vec<RenameEvidenceRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT commit_hash, old_path, new_path, similarity_score, detection_source
             FROM rename_evidence
             WHERE repo_path = ?1 AND (old_path = ?2 OR new_path = ?2)
             ORDER BY commit_hash ASC",
        )?;

        let rows = stmt.query_map(params![repo_path, path], |row| {
            Ok(RenameEvidenceRow {
                commit_hash:      row.get(0)?,
                old_path:         row.get(1)?,
                new_path:         row.get(2)?,
                similarity_score: row.get::<_, i64>(3)? as u8,
                detection_source: row.get(4)?,
            })
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ── File identity storage ─────────────────────────────────────────────────

    /// Create a new FileIdentity record and return its id.
    pub fn insert_file_identity(&self, repo_path: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO file_identities (repo_path) VALUES (?1)",
            params![repo_path],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Record that `path` was observed under `identity_id`.
    /// `superseded_by` is None until a rename moves this path to a new one.
    pub fn insert_path_observation(
        &self,
        identity_id:   i64,
        path:          &str,
        introduced_by: &str,
        superseded_by: Option<&str>,
        repo_path:     &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO file_path_observations
               (file_identity_id, path, introduced_by_commit, superseded_by_commit, repo_path)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![identity_id, path, introduced_by, superseded_by, repo_path],
        )?;
        Ok(())
    }

    /// Mark the path observation for `(identity_id, path)` as superseded by a rename commit.
    pub fn supersede_path_observation(
        &self,
        identity_id:  i64,
        path:         &str,
        superseded_by: &str,
        repo_path:    &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE file_path_observations
             SET superseded_by_commit = ?1
             WHERE file_identity_id = ?2 AND path = ?3 AND repo_path = ?4
               AND superseded_by_commit IS NULL",
            params![superseded_by, identity_id, path, repo_path],
        )?;
        Ok(())
    }

    /// All identity ids that have ever occupied `path` (may be >1 in path-reuse scenarios).
    pub fn identities_for_path(&self, path: &str, repo_path: &str) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT file_identity_id
             FROM file_path_observations
             WHERE path = ?1 AND repo_path = ?2
             ORDER BY file_identity_id ASC",
        )?;
        let rows = stmt.query_map(params![path, repo_path], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The identity that currently occupies `path` (observation with no superseding commit).
    /// Returns None if the path is not current for any identity (historical-only) or unknown.
    pub fn resolve_current_path(&self, path: &str, repo_path: &str) -> Result<Option<i64>> {
        self.conn.query_row(
            "SELECT file_identity_id
             FROM file_path_observations
             WHERE path = ?1 AND repo_path = ?2 AND superseded_by_commit IS NULL
             LIMIT 1",
            params![path, repo_path],
            |r| r.get(0),
        ).optional().map_err(Into::into)
    }

    /// The single identity that contains `path` as a non-ambiguous observation.
    /// Returns None if the path belongs to 0 or more than 1 identity (use identities_for_path
    /// to handle reuse cases explicitly).
    pub fn resolve_path_to_identity(&self, path: &str, repo_path: &str) -> Result<Option<i64>> {
        let ids = self.identities_for_path(path, repo_path)?;
        match ids.len() {
            1 => Ok(Some(ids[0])),
            _ => Ok(None),
        }
    }

    /// The identity that occupied `path` at the time of `commit_hash`.
    ///
    /// Resolution uses commit timestamps from the stored history.  This is
    /// correct for linear history but may produce unexpected results on
    /// divergent branches where two commits have the same timestamp.
    pub fn resolve_path_at(
        &self,
        path:        &str,
        commit_hash: &str,
        repo_path:   &str,
    ) -> Result<Option<i64>> {
        self.conn.query_row(
            "SELECT fpo.file_identity_id
             FROM file_path_observations fpo
             JOIN commits c_intro ON c_intro.hash = fpo.introduced_by_commit
             LEFT JOIN commits c_super ON c_super.hash = fpo.superseded_by_commit
             WHERE fpo.path = ?1 AND fpo.repo_path = ?2
               AND c_intro.timestamp <= (SELECT timestamp FROM commits WHERE hash = ?3 LIMIT 1)
               AND (c_super.timestamp IS NULL
                    OR c_super.timestamp > (SELECT timestamp FROM commits WHERE hash = ?3 LIMIT 1))
             LIMIT 1",
            params![path, repo_path, commit_hash],
            |r| r.get(0),
        ).optional().map_err(Into::into)
    }

    /// All path observations for an identity, ordered by introduction time.
    pub fn path_history_for_identity(
        &self,
        identity_id: i64,
        repo_path:   &str,
    ) -> Result<Vec<PathObservationRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT fpo.file_identity_id, fpo.path,
                    fpo.introduced_by_commit, fpo.superseded_by_commit
             FROM file_path_observations fpo
             LEFT JOIN commits c ON c.hash = fpo.introduced_by_commit
             WHERE fpo.file_identity_id = ?1 AND fpo.repo_path = ?2
             ORDER BY COALESCE(c.timestamp, 0) ASC, fpo.path ASC",
        )?;
        let rows = stmt.query_map(params![identity_id, repo_path], |row| {
            Ok(PathObservationRow {
                file_identity_id:     row.get(0)?,
                path:                 row.get(1)?,
                introduced_by_commit: row.get(2)?,
                superseded_by_commit: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Materialize commit membership for every identity in a repo.
    ///
    /// For each path observation, finds commits that touched that path within the
    /// observation's temporal window [introduced_ts, superseded_ts) and records
    /// them in `file_identity_commits`.  The temporal bound prevents the
    /// path-reuse bug: a commit that reuses a path after a rename cannot be
    /// assigned to the earlier identity that previously held that path.
    pub fn populate_identity_commits(&self, repo_path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO file_identity_commits (file_identity_id, commit_hash, repo_path)
             SELECT fpo.file_identity_id, c.hash, fpo.repo_path
             FROM file_path_observations fpo
             JOIN commit_files cf ON cf.file_path = fpo.path
             JOIN commits c ON c.hash = cf.commit_hash AND c.repo_path = fpo.repo_path
             JOIN commits c_intro ON c_intro.hash = fpo.introduced_by_commit
             LEFT JOIN commits c_super ON c_super.hash = fpo.superseded_by_commit
             WHERE fpo.repo_path = ?1
               AND c.timestamp >= c_intro.timestamp
               AND (fpo.superseded_by_commit IS NULL OR c.timestamp < c_super.timestamp)",
            params![repo_path],
        )?;
        Ok(())
    }

    /// All commits that belong to an identity, newest first.
    /// Uses the materialized `file_identity_commits` table — run
    /// `populate_identity_commits` (via `rebuild_file_identities`) first.
    pub fn commits_for_identity(&self, identity_id: i64, repo_path: &str) -> Result<Vec<CommitRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.hash, c.short_hash, c.message, c.author_name, c.timestamp
             FROM file_identity_commits fic
             JOIN commits c ON c.hash = fic.commit_hash AND c.repo_path = fic.repo_path
             WHERE fic.file_identity_id = ?1 AND fic.repo_path = ?2
             ORDER BY c.timestamp DESC",
        )?;
        let rows = stmt.query_map(params![identity_id, repo_path], |row| {
            Ok(CommitRow {
                hash:        row.get(0)?,
                short_hash:  row.get(1)?,
                message:     row.get(2)?,
                author_name: row.get(3)?,
                timestamp:   row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Record a single commit as belonging to an identity.
    /// Prefer `populate_identity_commits` for bulk materialization; use this for tests.
    pub fn insert_file_identity_commit(
        &self,
        identity_id: i64,
        commit_hash: &str,
        repo_path:   &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO file_identity_commits
               (file_identity_id, commit_hash, repo_path)
             VALUES (?1, ?2, ?3)",
            params![identity_id, commit_hash, repo_path],
        )?;
        Ok(())
    }

    /// Rename evidence records linked to a specific identity, via path observations.
    /// Returns each rename edge where the identity's path appears as old_path or new_path,
    /// preserving the epistemic framing: similarity is Git's heuristic, not Atlas confidence.
    pub fn identity_evidence_for(
        &self,
        identity_id: i64,
        repo_path:   &str,
    ) -> Result<Vec<IdentityEvidenceRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT re.commit_hash, re.old_path, re.new_path,
                    re.similarity_score, re.detection_source
             FROM rename_evidence re
             JOIN file_path_observations fpo
               ON (fpo.path = re.old_path OR fpo.path = re.new_path)
             WHERE fpo.file_identity_id = ?1
               AND re.repo_path = ?2
               AND fpo.repo_path = ?2
             ORDER BY re.commit_hash ASC",
        )?;
        let rows = stmt.query_map(params![identity_id, repo_path], |row| {
            Ok(IdentityEvidenceRow {
                source_commit_hash: row.get(0)?,
                old_path:           row.get(1)?,
                new_path:           row.get(2)?,
                similarity:         row.get::<_, i64>(3)? as u8,
                detection_source:   row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Co-changes for a FileIdentity: files that co-appeared in commits belonging to
    /// this identity across its full path history.  Excludes all paths that are part
    /// of the identity itself (old and current paths alike).
    ///
    /// Use this instead of `co_changes_for_file` when an identity chain is known —
    /// it sees the full cross-rename coupling history.
    pub fn co_changes_for_identity(
        &self,
        identity_id: i64,
        repo_path:   &str,
        min_count:   i64,
    ) -> Result<Vec<CoChangeRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT cf2.file_path, COUNT(DISTINCT cf2.commit_hash) as change_count
             FROM file_identity_commits fic
             JOIN commit_files cf2 ON cf2.commit_hash = fic.commit_hash
             WHERE fic.file_identity_id = ?1 AND fic.repo_path = ?2
               AND cf2.file_path NOT IN (
                   SELECT path FROM file_path_observations
                   WHERE file_identity_id = ?1 AND repo_path = ?2
               )
             GROUP BY cf2.file_path
             HAVING change_count >= ?3
             ORDER BY change_count DESC, cf2.file_path ASC
             LIMIT 20",
        )?;
        let rows = stmt.query_map(params![identity_id, repo_path, min_count], |row| {
            Ok(CoChangeRow {
                file_path:    row.get(0)?,
                change_count: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Files ordered by identity-aware commit count (most active first).
    ///
    /// For files with a FileIdentity chain the count spans all path phases (pre- and
    /// post-rename), reported under the current canonical path.  Files that have never
    /// been part of a rename chain fall back to path-scoped counts.  This ensures the
    /// ranking plane matches the touch_count Atlas reports in `build_context`.
    pub fn hot_files_identity_aware(&self, repo_path: &str, limit: i64) -> Result<Vec<HotFileRow>> {
        let mut stmt = self.conn.prepare(
            // Identity-aware: current canonical path + full commit history across all renames.
            "SELECT fpo_cur.path as file_path, COUNT(DISTINCT fic.commit_hash) as touch_count
             FROM file_identities fi
             JOIN file_path_observations fpo_cur
               ON fpo_cur.file_identity_id = fi.id
               AND fpo_cur.repo_path = ?1
               AND fpo_cur.superseded_by_commit IS NULL
             JOIN file_identity_commits fic
               ON fic.file_identity_id = fi.id AND fic.repo_path = ?1
             WHERE fi.repo_path = ?1
             GROUP BY fi.id

             UNION

             -- Path-scoped fallback for files with no identity chain.
             SELECT cf.file_path, COUNT(DISTINCT cf.commit_hash) as touch_count
             FROM commit_files cf
             JOIN commits c ON c.hash = cf.commit_hash
             WHERE c.repo_path = ?1
               AND cf.file_path NOT IN (
                   SELECT path FROM file_path_observations WHERE repo_path = ?1
               )
             GROUP BY cf.file_path

             ORDER BY touch_count DESC, file_path ASC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![repo_path, limit], |row| {
            Ok(HotFileRow {
                file_path:   row.get(0)?,
                touch_count: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// True when `rebuild_file_identities` has materialized at least one identity for this repo.
    /// Used to decide whether `rename_tracking` coverage should be reported as Available.
    pub fn has_materialized_identities(&self, repo_path: &str) -> Result<bool> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM file_identities WHERE repo_path = ?1)",
            params![repo_path],
            |r| r.get(0),
        )?;
        Ok(exists)
    }

    /// Delete all materialized identity state for a repo so it can be rebuilt.
    pub fn clear_file_identities(&self, repo_path: &str) -> Result<()> {
        // Delete in FK order: dependents first.
        self.conn.execute(
            "DELETE FROM file_identity_commits WHERE repo_path = ?1",
            params![repo_path],
        )?;
        self.conn.execute(
            "DELETE FROM file_path_observations WHERE repo_path = ?1",
            params![repo_path],
        )?;
        self.conn.execute(
            "DELETE FROM file_identities WHERE repo_path = ?1",
            params![repo_path],
        )?;
        Ok(())
    }

    /// All rename evidence for a repo joined with commit timestamps, sorted oldest first.
    /// Used by the identity resolver to process edges in causal order.
    pub fn rename_evidence_with_timestamps(&self, repo_path: &str) -> Result<Vec<RenameWithTs>> {
        let mut stmt = self.conn.prepare(
            "SELECT re.commit_hash, re.old_path, re.new_path, re.similarity_score, c.timestamp
             FROM rename_evidence re
             JOIN commits c ON c.hash = re.commit_hash
             WHERE re.repo_path = ?1
             ORDER BY c.timestamp ASC, re.old_path ASC",
        )?;
        let rows = stmt.query_map(params![repo_path], |row| {
            Ok(RenameWithTs {
                commit_hash:      row.get(0)?,
                old_path:         row.get(1)?,
                new_path:         row.get(2)?,
                similarity_score: row.get::<_, i64>(3)? as u8,
                timestamp:        row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Commits that touched `path` strictly after `after_ts` (Unix timestamp), oldest first.
    /// Used by the identity resolver to detect path-reuse after a rename.
    pub fn commits_for_file_after_ts(
        &self,
        path:     &str,
        after_ts: i64,
        repo_path: &str,
    ) -> Result<Vec<CommitRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.hash, c.short_hash, c.message, c.author_name, c.timestamp
             FROM commits c
             JOIN commit_files cf ON c.hash = cf.commit_hash
             WHERE cf.file_path = ?1 AND c.repo_path = ?2 AND c.timestamp > ?3
             ORDER BY c.timestamp ASC",
        )?;
        let rows = stmt.query_map(params![path, repo_path, after_ts], |row| {
            Ok(CommitRow {
                hash:        row.get(0)?,
                short_hash:  row.get(1)?,
                message:     row.get(2)?,
                author_name: row.get(3)?,
                timestamp:   row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Search across all text fields in the corpus for a single anchor term.
    ///
    /// Returns one row per (anchor, source_type, source_id) match — the caller
    /// is responsible for deduplication when multiple anchors are searched.
    /// `text` is the full matched field; snippet extraction happens in the core layer.
    pub fn search_anchor(&self, anchor: &str, repo_path: &str) -> Result<Vec<AnchorMatchRow>> {
        let pattern = format!("%{}%", anchor);
        let mut stmt = self.conn.prepare(
            "SELECT ?1 AS anchor, 'file_path' AS source_type, path AS source_id, path AS text
             FROM files
             WHERE repo_path = ?2 AND path LIKE ?3

             UNION ALL

             SELECT ?1, 'commit_message', hash, message
             FROM commits
             WHERE repo_path = ?2 AND message LIKE ?3

             UNION ALL

             SELECT ?1, 'pr_title', CAST(number AS TEXT), title
             FROM pull_requests
             WHERE repo_path = ?2 AND title LIKE ?3

             UNION ALL

             SELECT ?1, 'pr_body', CAST(number AS TEXT), body
             FROM pull_requests
             WHERE repo_path = ?2 AND body IS NOT NULL AND body LIKE ?3

             UNION ALL

             SELECT ?1, 'issue_title', CAST(number AS TEXT), title
             FROM issues
             WHERE repo_path = ?2 AND title LIKE ?3

             UNION ALL

             SELECT ?1, 'issue_body', CAST(number AS TEXT), body
             FROM issues
             WHERE repo_path = ?2 AND body IS NOT NULL AND body LIKE ?3

             UNION ALL

             SELECT ?1, 'decision_body', file_path, body
             FROM documents
             WHERE repo_path = ?2 AND body LIKE ?3",
        )?;
        let rows = stmt.query_map(params![anchor, repo_path, pattern], |row| {
            Ok(AnchorMatchRow {
                anchor:      row.get(0)?,
                source_type: row.get(1)?,
                source_id:   row.get(2)?,
                text:        row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn insert_document(
        &self,
        file_path: &str,
        doc_type: &str,
        title: &str,
        body: &str,
        repo_path: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO documents (file_path, doc_type, title, body, repo_path)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![file_path, doc_type, title, body, repo_path],
        )?;
        Ok(())
    }

    pub fn document_count(&self, repo_path: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM documents WHERE repo_path = ?1",
            params![repo_path],
            |r| r.get(0),
        )?)
    }

    pub fn document_by_path(&self, file_path: &str, repo_path: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row(
            "SELECT title FROM documents WHERE file_path = ?1 AND repo_path = ?2",
            params![file_path, repo_path],
            |r| r.get(0),
        ).optional()?)
    }

    pub fn insert_structural_edge(&self, edge: &StructuralEdge, repo_path: &str) -> Result<()> {
        let kind = match edge.kind {
            StructuralEdgeKind::Imports          => "imports",
            StructuralEdgeKind::CallsStatic      => "calls_static",
            StructuralEdgeKind::CallsInstance    => "calls_instance",
            StructuralEdgeKind::ReferencesModel  => "references_model",
        };
        self.conn.execute(
            "INSERT OR IGNORE INTO structural_edges
               (repo_path, source_file, source_symbol, target_file, target_symbol,
                kind, evidence_line, evidence_snippet, extractor)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                repo_path,
                edge.source_file,
                edge.source_symbol,
                edge.target_file,
                edge.target_symbol,
                kind,
                edge.evidence.line.map(|l| l as i64),
                edge.evidence.snippet,
                edge.evidence.extractor,
            ],
        )?;
        Ok(())
    }

    pub fn structural_edges_for_file(
        &self,
        source_file: &str,
        repo_path: &str,
    ) -> Result<Vec<StructuralEdgeRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT source_file, source_symbol, target_file, target_symbol,
                    kind, evidence_line, evidence_snippet, extractor
             FROM structural_edges
             WHERE repo_path = ?1 AND source_file = ?2
             ORDER BY kind, target_file",
        )?;
        let rows = stmt.query_map(params![repo_path, source_file], |row| {
            Ok(StructuralEdgeRow {
                source_file:      row.get(0)?,
                source_symbol:    row.get(1)?,
                target_file:      row.get(2)?,
                target_symbol:    row.get(3)?,
                kind:             row.get(4)?,
                evidence_line:    row.get::<_, Option<i64>>(5)?.map(|l| l as u32),
                evidence_snippet: row.get(6)?,
                extractor:        row.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// All structural edges referencing `target_file` (reverse lookup).
    pub fn structural_edges_targeting(
        &self,
        target_file: &str,
        repo_path: &str,
    ) -> Result<Vec<StructuralEdgeRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT source_file, source_symbol, target_file, target_symbol,
                    kind, evidence_line, evidence_snippet, extractor
             FROM structural_edges
             WHERE repo_path = ?1 AND target_file = ?2
             ORDER BY source_file",
        )?;
        let rows = stmt.query_map(params![repo_path, target_file], |row| {
            Ok(StructuralEdgeRow {
                source_file:      row.get(0)?,
                source_symbol:    row.get(1)?,
                target_file:      row.get(2)?,
                target_symbol:    row.get(3)?,
                kind:             row.get(4)?,
                evidence_line:    row.get::<_, Option<i64>>(5)?.map(|l| l as u32),
                evidence_snippet: row.get(6)?,
                extractor:        row.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn structural_edge_count(&self, repo_path: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM structural_edges WHERE repo_path = ?1",
            params![repo_path],
            |row| row.get(0),
        )?)
    }

    /// Fetch all (sibling_file, target_file, target_symbol) triples for peer files
    /// matching `like_pattern` with the given edge `kind`, excluding `exclude_file`.
    pub fn sibling_edges_by_pattern(
        &self,
        repo_path: &str,
        like_pattern: &str,
        exclude_file: &str,
        kind: &str,
    ) -> Result<Vec<SiblingEdgeRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT source_file, target_file, target_symbol
             FROM structural_edges
             WHERE repo_path = ?1
               AND kind = ?4
               AND source_file LIKE ?2
               AND source_file != ?3
             ORDER BY source_file, target_file",
        )?;
        let rows = stmt.query_map(
            params![repo_path, like_pattern, exclude_file, kind],
            |row| {
                Ok(SiblingEdgeRow {
                    sibling_file:  row.get(0)?,
                    target_file:   row.get(1)?,
                    target_symbol: row.get(2)?,
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn issues_for_file(&self, file_path: &str, repo_path: &str) -> Result<Vec<IssueRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT i.number, i.title, i.state, i.author, i.created_at
             FROM issues i
             JOIN pr_issues pi ON pi.issue_number = i.number AND pi.repo_path = i.repo_path
             JOIN pull_requests pr ON pr.number = pi.pr_number AND pr.repo_path = pi.repo_path
             JOIN commits c ON c.hash = pr.merge_commit_sha AND c.repo_path = pr.repo_path
             JOIN commit_files cf ON cf.commit_hash = c.hash
             WHERE cf.file_path = ?1 AND i.repo_path = ?2
             ORDER BY i.created_at ASC, i.number ASC",
        )?;

        let rows = stmt.query_map(params![file_path, repo_path], |row| {
            Ok(IssueRow {
                number:     row.get(0)?,
                title:      row.get(1)?,
                state:      row.get(2)?,
                author:     row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ── Project registry (v0.7a) ──────────────────────────────────────────────

    pub fn create_project(&self, name: &str, description: Option<&str>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO projects (name, description) VALUES (?1, ?2)",
            params![name, description],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_project_by_name(&self, name: &str) -> Result<Option<ProjectRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description FROM projects WHERE name = ?1",
        )?;
        let mut rows = stmt.query_map(params![name], |row| {
            Ok(ProjectRecord {
                id:          row.get(0)?,
                name:        row.get(1)?,
                description: row.get(2)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description FROM projects ORDER BY name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ProjectRecord {
                id:          row.get(0)?,
                name:        row.get(1)?,
                description: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ── Repository registry (v0.7a) ───────────────────────────────────────────

    pub fn register_repository(
        &self,
        project_id:       i64,
        name:             &str,
        role_label:       Option<&str>,
        local_path:       Option<&str>,
        remote_url:       Option<&str>,
        existence_source: &ExistenceSource,
        access_state:     &AccessState,
        ingestion_state:  &IngestionState,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO repositories
               (project_id, name, role_label, local_path, remote_url,
                existence_source, access_state, ingestion_state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                project_id,
                name,
                role_label,
                local_path,
                remote_url,
                existence_source.as_str(),
                access_state.as_str(),
                ingestion_state.as_str(),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_repositories(&self, project_id: i64) -> Result<Vec<RepositoryRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, name, role_label, local_path, remote_url,
                    existence_source, access_state, ingestion_state
             FROM repositories WHERE project_id = ?1
             ORDER BY name ASC",
        )?;
        let rows = stmt.query_map(params![project_id], Self::row_to_repository)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_repository_by_path(&self, local_path: &str) -> Result<Option<RepositoryRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, name, role_label, local_path, remote_url,
                    existence_source, access_state, ingestion_state
             FROM repositories WHERE local_path = ?1 LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![local_path], Self::row_to_repository)?;
        Ok(rows.next().transpose()?)
    }

    pub fn update_ingestion_state(&self, repo_id: i64, state: &IngestionState) -> Result<()> {
        self.conn.execute(
            "UPDATE repositories SET ingestion_state = ?1 WHERE id = ?2",
            params![state.as_str(), repo_id],
        )?;
        Ok(())
    }

    fn row_to_repository(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepositoryRecord> {
        let ex_str: String = row.get(6)?;
        let ac_str: String = row.get(7)?;
        let in_str: String = row.get(8)?;
        Ok(RepositoryRecord {
            id:               row.get(0)?,
            project_id:       row.get(1)?,
            name:             row.get(2)?,
            role_label:       row.get(3)?,
            local_path:       row.get(4)?,
            remote_url:       row.get(5)?,
            existence_source: ExistenceSource::from_str(&ex_str)
                                  .unwrap_or(ExistenceSource::LocalObserved),
            access_state:     AccessState::from_str(&ac_str)
                                  .unwrap_or(AccessState::Accessible),
            ingestion_state:  IngestionState::from_str(&in_str)
                                  .unwrap_or(IngestionState::NotIngested),
        })
    }

    // ── Repository profile claims (v0.7a) ─────────────────────────────────────

    pub fn replace_profile_claims(
        &self,
        repository_id: i64,
        claims:        &[ProfileClaim],
        inspected_at:  i64,
    ) -> Result<()> {
        self.conn.execute(
            "DELETE FROM repository_profile_claims WHERE repository_id = ?1",
            params![repository_id],
        )?;
        for claim in claims {
            let evidence_json = serde_json::to_string(&claim.evidence)
                .map_err(|e| anyhow::anyhow!("evidence serialization failed: {}", e))?;
            self.conn.execute(
                "INSERT INTO repository_profile_claims
                   (repository_id, claim_kind, claim_value, evidence_json, inspected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    repository_id,
                    claim.kind.as_str(),
                    claim.value,
                    evidence_json,
                    inspected_at,
                ],
            )?;
        }
        Ok(())
    }

    pub fn load_profile_claims(
        &self,
        repository_id: i64,
    ) -> Result<(Vec<ProfileClaim>, Option<i64>)> {
        let mut stmt = self.conn.prepare(
            "SELECT claim_kind, claim_value, evidence_json, inspected_at
             FROM repository_profile_claims
             WHERE repository_id = ?1
             ORDER BY id ASC",
        )?;

        let mut claims: Vec<ProfileClaim> = Vec::new();
        let mut inspected_at: Option<i64> = None;

        let rows = stmt.query_map(params![repository_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;

        for row in rows {
            let (kind_str, value, evidence_json, ts) = row?;
            inspected_at = Some(ts);
            if let Some(kind) = ProfileClaimKind::from_str(&kind_str) {
                if let Ok(evidence) =
                    serde_json::from_str::<atlas_ir::ClaimEvidence>(&evidence_json)
                {
                    claims.push(ProfileClaim { kind, value, evidence });
                }
            }
        }

        Ok((claims, inspected_at))
    }
}

#[derive(Debug, Clone)]
pub struct CommitRow {
    pub hash:        String,
    pub short_hash:  String,
    pub message:     String,
    pub author_name: String,
    pub timestamp:   i64,
}

#[derive(Debug)]
pub struct PrRow {
    pub number:           i64,
    pub title:            String,
    pub state:            String,
    pub author:           String,
    pub merge_commit_sha: Option<String>,
    pub created_at:       Option<i64>,
    pub merged_at:        Option<i64>,
}

#[derive(Debug)]
pub struct IssueRow {
    pub number:     i64,
    pub title:      String,
    pub state:      String,
    pub author:     String,
    pub created_at: Option<i64>,
}

/// Raw result from `search_anchor`.  The core layer extracts snippets and
/// maps `source_type` strings to typed IR enums.
#[derive(Debug)]
pub struct AnchorMatchRow {
    pub anchor:      String,
    /// One of: "file_path", "commit_message", "pr_title", "pr_body",
    ///         "issue_title", "issue_body"
    pub source_type: String,
    /// File path, commit hash, or PR/issue number (as string).
    pub source_id:   String,
    /// Full matched text — title, message, body, or path.
    pub text:        String,
}

/// Row returned from sibling_edges_by_pattern.
#[derive(Debug)]
pub struct SiblingEdgeRow {
    pub sibling_file:  String,
    pub target_file:   String,
    pub target_symbol: Option<String>,
}

/// Row returned from structural_edges queries.
/// `kind` is the raw string stored in DB ("imports", "calls_static", "references_model").
#[derive(Debug)]
pub struct StructuralEdgeRow {
    pub source_file:      String,
    pub source_symbol:    Option<String>,
    pub target_file:      String,
    pub target_symbol:    Option<String>,
    pub kind:             String,
    pub evidence_line:    Option<u32>,
    pub evidence_snippet: String,
    pub extractor:        String,
}

#[derive(Debug)]
pub struct CoChangeRow {
    pub file_path:    String,
    pub change_count: i64,
}

#[derive(Debug)]
pub struct HotFileRow {
    pub file_path:   String,
    pub touch_count: i64,
}

#[derive(Debug, PartialEq)]
pub struct RenameEvidenceRow {
    pub commit_hash:      String,
    pub old_path:         String,
    pub new_path:         String,
    pub similarity_score: u8,
    pub detection_source: String,
}

/// A rename evidence record linked to a specific FileIdentity.
/// `similarity` is Git's internal heuristic score (0–100): evidence, not Atlas confidence.
#[derive(Debug, PartialEq)]
pub struct IdentityEvidenceRow {
    pub source_commit_hash: String,
    pub old_path:           String,
    pub new_path:           String,
    pub similarity:         u8,
    pub detection_source:   String,
}

/// A rename edge with the commit timestamp attached, used by the identity resolver.
#[derive(Debug)]
pub struct RenameWithTs {
    pub commit_hash:      String,
    pub old_path:         String,
    pub new_path:         String,
    pub similarity_score: u8,
    pub timestamp:        i64,
}

#[derive(Debug, PartialEq)]
pub struct PathObservationRow {
    pub file_identity_id:     i64,
    pub path:                 String,
    pub introduced_by_commit: String,
    pub superseded_by_commit: Option<String>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS commits (
    hash         TEXT PRIMARY KEY,
    short_hash   TEXT NOT NULL,
    message      TEXT NOT NULL,
    author_name  TEXT NOT NULL,
    author_email TEXT NOT NULL,
    timestamp    INTEGER NOT NULL,
    repo_path    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    path      TEXT NOT NULL,
    repo_path TEXT NOT NULL,
    UNIQUE(path, repo_path)
);

CREATE TABLE IF NOT EXISTS commit_files (
    commit_hash TEXT NOT NULL,
    file_path   TEXT NOT NULL,
    PRIMARY KEY (commit_hash, file_path),
    FOREIGN KEY (commit_hash) REFERENCES commits(hash)
);

CREATE TABLE IF NOT EXISTS pull_requests (
    number           INTEGER NOT NULL,
    title            TEXT NOT NULL,
    state            TEXT NOT NULL,
    body             TEXT,
    author           TEXT,
    merge_commit_sha TEXT,
    repo_path        TEXT NOT NULL,
    created_at       INTEGER,
    merged_at        INTEGER,
    PRIMARY KEY (number, repo_path)
);

CREATE TABLE IF NOT EXISTS issues (
    number     INTEGER NOT NULL,
    title      TEXT NOT NULL,
    state      TEXT NOT NULL,
    body       TEXT,
    author     TEXT,
    repo_path  TEXT NOT NULL,
    created_at INTEGER,
    PRIMARY KEY (number, repo_path)
);

CREATE TABLE IF NOT EXISTS pr_issues (
    pr_number    INTEGER NOT NULL,
    issue_number INTEGER NOT NULL,
    repo_path    TEXT NOT NULL,
    PRIMARY KEY (pr_number, issue_number, repo_path)
);

CREATE TABLE IF NOT EXISTS rename_evidence (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    commit_hash      TEXT NOT NULL,
    old_path         TEXT NOT NULL,
    new_path         TEXT NOT NULL,
    similarity_score INTEGER NOT NULL,
    detection_source TEXT NOT NULL,
    repo_path        TEXT NOT NULL,
    UNIQUE(commit_hash, old_path, new_path, repo_path)
);

CREATE TABLE IF NOT EXISTS file_identities (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_path TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS file_path_observations (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    file_identity_id     INTEGER NOT NULL,
    path                 TEXT NOT NULL,
    introduced_by_commit TEXT NOT NULL,
    superseded_by_commit TEXT,
    repo_path            TEXT NOT NULL,
    FOREIGN KEY (file_identity_id) REFERENCES file_identities(id),
    UNIQUE(path, introduced_by_commit, repo_path)
);

CREATE TABLE IF NOT EXISTS file_identity_commits (
    file_identity_id INTEGER NOT NULL,
    commit_hash      TEXT NOT NULL,
    repo_path        TEXT NOT NULL,
    PRIMARY KEY (file_identity_id, commit_hash, repo_path),
    FOREIGN KEY (file_identity_id) REFERENCES file_identities(id)
);

CREATE TABLE IF NOT EXISTS structural_edges (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_path      TEXT NOT NULL,
    source_file    TEXT NOT NULL,
    source_symbol  TEXT,
    target_file    TEXT NOT NULL,
    target_symbol  TEXT,
    kind           TEXT NOT NULL,
    evidence_line  INTEGER,
    evidence_snippet TEXT NOT NULL,
    extractor      TEXT NOT NULL,
    UNIQUE(repo_path, source_file, target_file, target_symbol, kind, extractor)
);
CREATE INDEX IF NOT EXISTS idx_structural_edges_source ON structural_edges(repo_path, source_file);
CREATE INDEX IF NOT EXISTS idx_structural_edges_target ON structural_edges(repo_path, target_file);

CREATE TABLE IF NOT EXISTS documents (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL,
    doc_type  TEXT NOT NULL,
    title     TEXT NOT NULL,
    body      TEXT NOT NULL,
    repo_path TEXT NOT NULL,
    UNIQUE(file_path, repo_path)
);

CREATE TABLE IF NOT EXISTS projects (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,
    description TEXT
);

CREATE TABLE IF NOT EXISTS repositories (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id       INTEGER NOT NULL,
    name             TEXT NOT NULL,
    role_label       TEXT,
    local_path       TEXT,
    remote_url       TEXT,
    existence_source TEXT NOT NULL DEFAULT 'local_observed',
    access_state     TEXT NOT NULL DEFAULT 'accessible',
    ingestion_state  TEXT NOT NULL DEFAULT 'not_ingested',
    UNIQUE(project_id, name),
    FOREIGN KEY (project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS repository_profile_claims (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    repository_id INTEGER NOT NULL,
    claim_kind    TEXT NOT NULL,
    claim_value   TEXT NOT NULL,
    evidence_json TEXT NOT NULL,
    inspected_at  INTEGER NOT NULL,
    FOREIGN KEY (repository_id) REFERENCES repositories(id)
);

CREATE INDEX IF NOT EXISTS idx_profile_claims_repo
    ON repository_profile_claims(repository_id);
";

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_ir::{Commit, Issue, PullRequest};
    use chrono::DateTime;

    fn test_commit(hash: &str, files: &[&str]) -> Commit {
        Commit {
            hash:          hash.into(),
            short_hash:    hash[..7.min(hash.len())].into(),
            message:       format!("commit {}", &hash[..4]),
            author_name:   "Alice".into(),
            author_email:  "a@x.com".into(),
            timestamp:     DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            files_changed: files.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn test_pr(number: i64, merge_commit_sha: &str) -> PullRequest {
        PullRequest {
            number,
            title:            format!("PR #{}", number),
            state:            "MERGED".into(),
            body:             None,
            author:           "alice".into(),
            merge_commit_sha: Some(merge_commit_sha.into()),
            created_at:       None,
            merged_at:        None,
        }
    }

    fn test_issue(number: i64) -> Issue {
        Issue {
            number,
            title:      format!("Issue #{}", number),
            state:      "CLOSED".into(),
            body:       None,
            author:     "alice".into(),
            created_at: None,
        }
    }

    #[test]
    fn insert_and_query_commit() {
        let store = Store::open(":memory:").unwrap();
        let c = test_commit("abc1234", &["src/main.rs"]);
        store.insert_commit(&c, ".").unwrap();

        let rows = store.commits_for_file("src/main.rs", ".").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].message, c.message);
    }

    #[test]
    fn commit_insert_is_idempotent() {
        let store = Store::open(":memory:").unwrap();
        let c = test_commit("abc1234", &["src/main.rs"]);
        store.insert_commit(&c, ".").unwrap();
        store.insert_commit(&c, ".").unwrap();
        assert_eq!(store.commit_count(".").unwrap(), 1);
    }

    #[test]
    fn unknown_file_returns_empty() {
        let store = Store::open(":memory:").unwrap();
        let rows = store.commits_for_file("no/such/file.rs", ".").unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn pr_linked_via_merge_commit_appears_for_touched_files() {
        let store = Store::open(":memory:").unwrap();

        // Commit B touches auth.ts and user.ts; PR #12 merges via commit B.
        let commit_b = test_commit("bbbbbbbbbbb1", &["auth.ts", "user.ts"]);
        store.insert_commit(&commit_b, ".").unwrap();

        let pr12 = test_pr(12, "bbbbbbbbbbb1");
        store.insert_pull_request(&pr12, ".").unwrap();

        let prs_auth = store.prs_for_file("auth.ts", ".").unwrap();
        assert_eq!(prs_auth.len(), 1);
        assert_eq!(prs_auth[0].number, 12);

        let prs_user = store.prs_for_file("user.ts", ".").unwrap();
        assert_eq!(prs_user.len(), 1);
        assert_eq!(prs_user[0].number, 12);
    }

    #[test]
    fn pr_without_merge_commit_does_not_appear() {
        let store = Store::open(":memory:").unwrap();
        let commit = test_commit("aaa1111", &["auth.ts"]);
        store.insert_commit(&commit, ".").unwrap();

        let open_pr = PullRequest {
            number:           99,
            title:            "open".into(),
            state:            "OPEN".into(),
            body:             None,
            author:           "bob".into(),
            merge_commit_sha: None,
            created_at:       None,
            merged_at:        None,
        };
        store.insert_pull_request(&open_pr, ".").unwrap();

        let prs = store.prs_for_file("auth.ts", ".").unwrap();
        assert!(prs.is_empty());
    }

    #[test]
    fn issue_reachable_from_file_via_pr() {
        let store = Store::open(":memory:").unwrap();

        let commit_b = test_commit("bbbbbbb1111", &["auth.ts"]);
        store.insert_commit(&commit_b, ".").unwrap();

        let pr12 = test_pr(12, "bbbbbbb1111");
        store.insert_pull_request(&pr12, ".").unwrap();

        let issue10 = test_issue(10);
        store.insert_issue(&issue10, ".").unwrap();

        store.link_pr_to_issue(12, 10, ".").unwrap();

        let issues = store.issues_for_file("auth.ts", ".").unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].number, 10);
    }

    #[test]
    fn issue_link_is_idempotent() {
        let store = Store::open(":memory:").unwrap();
        store.insert_issue(&test_issue(10), ".").unwrap();
        store.link_pr_to_issue(12, 10, ".").unwrap();
        store.link_pr_to_issue(12, 10, ".").unwrap();

        // No error and only one row
        let count: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM pr_issues WHERE pr_number=12 AND issue_number=10",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn pr_insert_is_idempotent() {
        let store = Store::open(":memory:").unwrap();
        let pr = test_pr(12, "abc1234");
        store.insert_pull_request(&pr, ".").unwrap();
        store.insert_pull_request(&pr, ".").unwrap();

        let count: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM pull_requests WHERE number=12",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn issue_insert_is_idempotent() {
        let store = Store::open(":memory:").unwrap();
        let issue = test_issue(10);
        store.insert_issue(&issue, ".").unwrap();
        store.insert_issue(&issue, ".").unwrap();

        let count: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM issues WHERE number=10",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn timestamp_survives_roundtrip() {
        let store = Store::open(":memory:").unwrap();
        let ts = DateTime::from_timestamp(1_718_000_000, 0).unwrap();
        let mut c = test_commit("ts_test_hash", &["a.rs"]);
        c.timestamp = ts;
        store.insert_commit(&c, ".").unwrap();

        let rows = store.commits_for_file("a.rs", ".").unwrap();
        assert_eq!(rows[0].timestamp, ts.timestamp());
    }

    #[test]
    fn repo_path_isolation() {
        let store = Store::open(":memory:").unwrap();
        let c = test_commit("abc1234", &["src/lib.rs"]);
        store.insert_commit(&c, "/repo/a").unwrap();

        // same file, different repo — must return nothing
        let rows = store.commits_for_file("src/lib.rs", "/repo/b").unwrap();
        assert!(rows.is_empty());
    }

    // ── Full fixture scenario ───────────────────────────────────────────────

    /// Reproduces the canonical fixture:
    ///   Commit A  → auth.ts
    ///   Commit B  → auth.ts, user.ts   (merge commit for PR #12)
    ///   Commit C  → user.ts
    ///   PR  #12   closes Issue #10
    ///
    /// Asserts that explain("auth.ts") returns:
    ///   commits A, B  |  PR #12  |  Issue #10
    #[test]
    fn fixture_full_scenario() {
        let store = Store::open(":memory:").unwrap();

        let hash_a = "aaaaaaaaaaaa";
        let hash_b = "bbbbbbbbbbbb";
        let hash_c = "cccccccccccc";

        let commit_a = Commit {
            hash:          hash_a.into(),
            short_hash:    "aaaaaaa".into(),
            message:       "Add authentication module".into(),
            author_name:   "Alice".into(),
            author_email:  "alice@example.com".into(),
            timestamp:     DateTime::from_timestamp(1_700_000_001, 0).unwrap(),
            files_changed: vec!["auth.ts".into()],
        };
        let commit_b = Commit {
            hash:          hash_b.into(),
            short_hash:    "bbbbbbb".into(),
            message:       "Add user model, extend auth".into(),
            author_name:   "Alice".into(),
            author_email:  "alice@example.com".into(),
            timestamp:     DateTime::from_timestamp(1_700_000_002, 0).unwrap(),
            files_changed: vec!["auth.ts".into(), "user.ts".into()],
        };
        let commit_c = Commit {
            hash:          hash_c.into(),
            short_hash:    "ccccccc".into(),
            message:       "Add getUser function".into(),
            author_name:   "Alice".into(),
            author_email:  "alice@example.com".into(),
            timestamp:     DateTime::from_timestamp(1_700_000_003, 0).unwrap(),
            files_changed: vec!["user.ts".into()],
        };

        store.insert_commit(&commit_a, ".").unwrap();
        store.insert_commit(&commit_b, ".").unwrap();
        store.insert_commit(&commit_c, ".").unwrap();

        let pr12 = PullRequest {
            number:           12,
            title:            "Add user authentication".into(),
            state:            "MERGED".into(),
            body:             Some("Closes #10".into()),
            author:           "alice".into(),
            merge_commit_sha: Some(hash_b.into()),
            created_at:       Some(DateTime::from_timestamp(1_696_154_400, 0).unwrap()),
            merged_at:        Some(DateTime::from_timestamp(1_696_351_500, 0).unwrap()),
        };
        store.insert_pull_request(&pr12, ".").unwrap();

        let issue10 = Issue {
            number:     10,
            title:      "Add user authentication".into(),
            state:      "CLOSED".into(),
            body:       None,
            author:     "alice".into(),
            created_at: Some(DateTime::from_timestamp(1_696_150_800, 0).unwrap()),
        };
        store.insert_issue(&issue10, ".").unwrap();
        store.link_pr_to_issue(12, 10, ".").unwrap();

        // auth.ts should have commits A and B
        let auth_commits = store.commits_for_file("auth.ts", ".").unwrap();
        let auth_hashes: Vec<&str> = auth_commits.iter().map(|c| c.short_hash.as_str()).collect();
        assert!(auth_hashes.contains(&"aaaaaaa"), "commit A missing from auth.ts");
        assert!(auth_hashes.contains(&"bbbbbbb"), "commit B missing from auth.ts");
        assert!(!auth_hashes.contains(&"ccccccc"), "commit C wrongly appears in auth.ts");

        // auth.ts should have PR #12
        let auth_prs = store.prs_for_file("auth.ts", ".").unwrap();
        assert_eq!(auth_prs.len(), 1);
        assert_eq!(auth_prs[0].number, 12);

        // auth.ts should have Issue #10
        let auth_issues = store.issues_for_file("auth.ts", ".").unwrap();
        assert_eq!(auth_issues.len(), 1);
        assert_eq!(auth_issues[0].number, 10);

        // user.ts should have commits B and C
        let user_commits = store.commits_for_file("user.ts", ".").unwrap();
        let user_hashes: Vec<&str> = user_commits.iter().map(|c| c.short_hash.as_str()).collect();
        assert!(user_hashes.contains(&"bbbbbbb"), "commit B missing from user.ts");
        assert!(user_hashes.contains(&"ccccccc"), "commit C missing from user.ts");
        assert!(!user_hashes.contains(&"aaaaaaa"), "commit A wrongly appears in user.ts");

        // double-ingest must not duplicate anything
        store.insert_commit(&commit_a, ".").unwrap();
        store.insert_commit(&commit_b, ".").unwrap();
        store.insert_commit(&commit_c, ".").unwrap();
        store.insert_pull_request(&pr12, ".").unwrap();
        store.insert_issue(&issue10, ".").unwrap();
        store.link_pr_to_issue(12, 10, ".").unwrap();

        assert_eq!(store.commit_count(".").unwrap(), 3);
        let count_prs: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM pull_requests", [], |r| r.get(0)
        ).unwrap();
        assert_eq!(count_prs, 1);
        let count_issues: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM issues", [], |r| r.get(0)
        ).unwrap();
        assert_eq!(count_issues, 1);
        let count_links: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM pr_issues", [], |r| r.get(0)
        ).unwrap();
        assert_eq!(count_links, 1);

        // PR and issue timestamps survive the roundtrip
        let prs = store.prs_for_file("auth.ts", ".").unwrap();
        assert_eq!(prs[0].created_at, Some(1_696_154_400_i64));
        assert_eq!(prs[0].merged_at,  Some(1_696_351_500_i64));
        let issues = store.issues_for_file("auth.ts", ".").unwrap();
        assert_eq!(issues[0].created_at, Some(1_696_150_800_i64));
    }

    #[test]
    fn co_changes_identifies_coupled_files() {
        let store = Store::open(":memory:").unwrap();

        store.insert_commit(&test_commit("aaaaaaa1111", &["auth.ts"]), ".").unwrap();
        store.insert_commit(&test_commit("bbbbbbb1111", &["auth.ts", "user.ts"]), ".").unwrap();
        store.insert_commit(&test_commit("ccccccc1111", &["user.ts"]), ".").unwrap();

        let co = store.co_changes_for_file("auth.ts", ".", 1).unwrap();
        assert_eq!(co.len(), 1);
        assert_eq!(co[0].file_path, "user.ts");
        assert_eq!(co[0].change_count, 1);

        let co2 = store.co_changes_for_file("user.ts", ".", 1).unwrap();
        assert_eq!(co2.len(), 1);
        assert_eq!(co2[0].file_path, "auth.ts");
    }

    #[test]
    fn co_changes_ranks_by_frequency() {
        let store = Store::open(":memory:").unwrap();

        for i in 0..2_u8 {
            store.insert_commit(
                &test_commit(&format!("commit_ab_{i}0000"), &["a.rs", "b.rs"]),
                ".",
            ).unwrap();
        }
        store.insert_commit(&test_commit("commit_ad_0000", &["a.rs", "d.rs"]), ".").unwrap();

        let co = store.co_changes_for_file("a.rs", ".", 1).unwrap();
        assert_eq!(co[0].file_path, "b.rs");
        assert_eq!(co[0].change_count, 2);
        assert_eq!(co[1].file_path, "d.rs");
        assert_eq!(co[1].change_count, 1);
    }

    #[test]
    fn co_changes_min_count_filters_infrequent_pairs() {
        let store = Store::open(":memory:").unwrap();

        // b.rs co-changes 2×; d.rs co-changes 1×
        for i in 0..2_u8 {
            store.insert_commit(
                &test_commit(&format!("commit_ab_min{i}000"), &["a.rs", "b.rs"]),
                ".",
            ).unwrap();
        }
        store.insert_commit(&test_commit("commit_ad_min0000", &["a.rs", "d.rs"]), ".").unwrap();

        let co = store.co_changes_for_file("a.rs", ".", 2).unwrap();
        assert_eq!(co.len(), 1, "only b.rs meets min_count=2");
        assert_eq!(co[0].file_path, "b.rs");
    }

    #[test]
    fn first_seen_returns_oldest_commit() {
        let store = Store::open(":memory:").unwrap();

        let mut old = test_commit("old_hash_1111", &["f.rs"]);
        old.timestamp = DateTime::from_timestamp(1_000, 0).unwrap();
        let mut new = test_commit("new_hash_1111", &["f.rs"]);
        new.timestamp = DateTime::from_timestamp(2_000, 0).unwrap();

        store.insert_commit(&new, ".").unwrap();
        store.insert_commit(&old, ".").unwrap();

        let row = store.first_seen("f.rs", ".").unwrap().expect("should find a commit");
        assert_eq!(row.short_hash, "old_has");
        assert_eq!(row.timestamp, 1_000);
    }

    #[test]
    fn last_seen_returns_newest_commit() {
        let store = Store::open(":memory:").unwrap();

        let mut old = test_commit("old2_hash_111", &["g.rs"]);
        old.timestamp = DateTime::from_timestamp(1_000, 0).unwrap();
        let mut new = test_commit("new2_hash_111", &["g.rs"]);
        new.timestamp = DateTime::from_timestamp(2_000, 0).unwrap();

        store.insert_commit(&old, ".").unwrap();
        store.insert_commit(&new, ".").unwrap();

        let row = store.last_seen("g.rs", ".").unwrap().expect("should find a commit");
        assert_eq!(row.short_hash, "new2_ha");
        assert_eq!(row.timestamp, 2_000);
    }

    #[test]
    fn first_seen_returns_none_for_unknown_file() {
        let store = Store::open(":memory:").unwrap();
        let result = store.first_seen("nonexistent.rs", ".").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn touch_count_correct() {
        let store = Store::open(":memory:").unwrap();
        store.insert_commit(&test_commit("tc_hash_1111", &["counted.rs"]), ".").unwrap();
        store.insert_commit(&test_commit("tc_hash_2222", &["counted.rs"]), ".").unwrap();
        store.insert_commit(&test_commit("tc_hash_3333", &["other.rs"]),   ".").unwrap();
        assert_eq!(store.touch_count("counted.rs", ".").unwrap(), 2);
        assert_eq!(store.touch_count("other.rs",   ".").unwrap(), 1);
        assert_eq!(store.touch_count("missing.rs", ".").unwrap(), 0);
    }

    #[test]
    fn hot_files_ordered_by_frequency() {
        let store = Store::open(":memory:").unwrap();

        // hot.rs: 3 commits, warm.rs: 2, cold.rs: 1
        for i in 0..3_u8 {
            store.insert_commit(&test_commit(&format!("hot_c{i}_0000"), &["hot.rs"]),  ".").unwrap();
        }
        for i in 0..2_u8 {
            store.insert_commit(&test_commit(&format!("warm_c{i}_000"), &["warm.rs"]), ".").unwrap();
        }
        store.insert_commit(&test_commit("cold_c0_00000", &["cold.rs"]), ".").unwrap();

        let hot = store.hot_files(".", 10).unwrap();
        assert_eq!(hot[0].file_path, "hot.rs");
        assert_eq!(hot[0].touch_count, 3);
        assert_eq!(hot[1].file_path, "warm.rs");
        assert_eq!(hot[1].touch_count, 2);
        assert_eq!(hot[2].file_path, "cold.rs");
        assert_eq!(hot[2].touch_count, 1);
    }

    #[test]
    fn hot_files_respects_limit() {
        let store = Store::open(":memory:").unwrap();
        for i in 0..5_u8 {
            store.insert_commit(&test_commit(&format!("lim_hash_{i}0000"), &[&format!("file{i}.rs")]), ".").unwrap();
        }
        let hot = store.hot_files(".", 3).unwrap();
        assert_eq!(hot.len(), 3);
    }

    #[test]
    fn commits_for_file_asc_is_oldest_first() {
        let store = Store::open(":memory:").unwrap();

        let mut c1 = test_commit("aaa1111", &["f.rs"]);
        c1.timestamp = DateTime::from_timestamp(1_000, 0).unwrap();
        let mut c2 = test_commit("bbb1111", &["f.rs"]);
        c2.timestamp = DateTime::from_timestamp(2_000, 0).unwrap();

        store.insert_commit(&c1, ".").unwrap();
        store.insert_commit(&c2, ".").unwrap();

        let rows = store.commits_for_file_asc("f.rs", ".").unwrap();
        assert_eq!(rows[0].timestamp, 1_000);
        assert_eq!(rows[1].timestamp, 2_000);
    }

    // ── Rename evidence ─────────────────────────────────────────────────────

    fn test_rename_ev(commit: &str, old: &str, new: &str, score: u8) -> atlas_ir::RenameEvidence {
        atlas_ir::RenameEvidence {
            commit_hash:      commit.into(),
            old_path:         old.into(),
            new_path:         new.into(),
            similarity_score: score,
            detection_source: "git-rename".into(),
        }
    }

    #[test]
    fn rename_evidence_insert_and_query_all() {
        let store = Store::open(":memory:").unwrap();
        let ev    = test_rename_ev("abc1234567", "old/auth.rs", "new/auth.rs", 100);
        store.insert_rename_evidence(&ev, ".").unwrap();

        let rows = store.all_rename_evidence(".").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].commit_hash,      "abc1234567");
        assert_eq!(rows[0].old_path,         "old/auth.rs");
        assert_eq!(rows[0].new_path,         "new/auth.rs");
        assert_eq!(rows[0].similarity_score, 100);
        assert_eq!(rows[0].detection_source, "git-rename");
    }

    #[test]
    fn rename_evidence_insert_is_idempotent() {
        let store = Store::open(":memory:").unwrap();
        let ev    = test_rename_ev("abc1234567", "a.rs", "b.rs", 100);
        store.insert_rename_evidence(&ev, ".").unwrap();
        store.insert_rename_evidence(&ev, ".").unwrap();

        let rows = store.all_rename_evidence(".").unwrap();
        assert_eq!(rows.len(), 1, "duplicate insert must produce exactly one row");
    }

    #[test]
    fn rename_evidence_for_path_returns_both_source_and_dest() {
        let store = Store::open(":memory:").unwrap();
        // ev1: auth.rs → security/auth.rs (auth.rs is old_path)
        // ev2: security/auth.rs → crates/auth/lib.rs (security/auth.rs is old_path)
        store.insert_rename_evidence(&test_rename_ev("aaa111", "auth.rs",          "security/auth.rs",      100), ".").unwrap();
        store.insert_rename_evidence(&test_rename_ev("bbb222", "security/auth.rs", "crates/auth/src/lib.rs", 75), ".").unwrap();

        // Query for "security/auth.rs" — it is both new_path of ev1 and old_path of ev2.
        let rows = store.rename_evidence_for_path("security/auth.rs", ".").unwrap();
        assert_eq!(rows.len(), 2, "security/auth.rs appears in both rename records");

        // Query for "auth.rs" — only ev1 references it.
        let rows_auth = store.rename_evidence_for_path("auth.rs", ".").unwrap();
        assert_eq!(rows_auth.len(), 1);
        assert_eq!(rows_auth[0].old_path, "auth.rs");
    }

    #[test]
    fn rename_evidence_repo_path_isolation() {
        let store = Store::open(":memory:").unwrap();
        let ev    = test_rename_ev("abc1234567", "a.rs", "b.rs", 100);
        store.insert_rename_evidence(&ev, "/repo/a").unwrap();

        let rows_a = store.all_rename_evidence("/repo/a").unwrap();
        let rows_b = store.all_rename_evidence("/repo/b").unwrap();
        assert_eq!(rows_a.len(), 1);
        assert!(rows_b.is_empty(), "rename evidence must be isolated by repo_path");
    }

    #[test]
    fn rename_evidence_multiple_commits_ordered_by_old_path() {
        let store = Store::open(":memory:").unwrap();
        store.insert_rename_evidence(&test_rename_ev("ccc333", "z.rs",    "za.rs",  90), ".").unwrap();
        store.insert_rename_evidence(&test_rename_ev("aaa111", "a.rs",    "ab.rs", 100), ".").unwrap();
        store.insert_rename_evidence(&test_rename_ev("bbb222", "middle.rs", "m2.rs", 80), ".").unwrap();

        let rows = store.all_rename_evidence(".").unwrap();
        assert_eq!(rows[0].old_path, "a.rs",      "first row: a.rs");
        assert_eq!(rows[1].old_path, "middle.rs",  "second row: middle.rs");
        assert_eq!(rows[2].old_path, "z.rs",       "third row: z.rs");
    }

    // ── File identity storage ───────────────────────────────────────────────

    fn setup_rename_chain(store: &Store) -> (String, String, String) {
        // Three commits forming a rename chain: p1 → p2 → p3
        // ts: 1000 < 2000 < 3000 (causal order)
        let h1 = "aaaaaaaaaaaaaaaa"; // introduces p1
        let h2 = "bbbbbbbbbbbbbbbb"; // renames p1 → p2
        let h3 = "cccccccccccccccc"; // renames p2 → p3
        store.insert_commit(&atlas_ir::Commit {
            hash: h1.into(), short_hash: "aaaaaaa".into(), message: "create p1".into(),
            author_name: "A".into(), author_email: "a@x.com".into(),
            timestamp: DateTime::from_timestamp(1_000, 0).unwrap(),
            files_changed: vec!["p1.rs".into()],
        }, ".").unwrap();
        store.insert_commit(&atlas_ir::Commit {
            hash: h2.into(), short_hash: "bbbbbbb".into(), message: "rename p1→p2".into(),
            author_name: "A".into(), author_email: "a@x.com".into(),
            timestamp: DateTime::from_timestamp(2_000, 0).unwrap(),
            files_changed: vec!["p2.rs".into()],
        }, ".").unwrap();
        store.insert_commit(&atlas_ir::Commit {
            hash: h3.into(), short_hash: "ccccccc".into(), message: "rename p2→p3".into(),
            author_name: "A".into(), author_email: "a@x.com".into(),
            timestamp: DateTime::from_timestamp(3_000, 0).unwrap(),
            files_changed: vec!["p3.rs".into()],
        }, ".").unwrap();
        // Insert rename evidence manually (bypasses git connector for unit tests)
        store.insert_rename_evidence(&atlas_ir::RenameEvidence {
            commit_hash: h2.into(), old_path: "p1.rs".into(), new_path: "p2.rs".into(),
            similarity_score: 100, detection_source: "git-rename".into(),
        }, ".").unwrap();
        store.insert_rename_evidence(&atlas_ir::RenameEvidence {
            commit_hash: h3.into(), old_path: "p2.rs".into(), new_path: "p3.rs".into(),
            similarity_score: 80, detection_source: "git-rename".into(),
        }, ".").unwrap();
        (h1.into(), h2.into(), h3.into())
    }

    #[test]
    fn insert_and_query_file_identity() {
        let store = Store::open(":memory:").unwrap();
        let id = store.insert_file_identity(".").unwrap();
        assert!(id > 0, "insert must return a positive id");
    }

    #[test]
    fn path_observation_insert_and_query() {
        let store = Store::open(":memory:").unwrap();
        let id = store.insert_file_identity(".").unwrap();
        store.insert_path_observation(id, "src/auth.rs", "hash_abc", None, ".").unwrap();

        let obs = store.path_history_for_identity(id, ".").unwrap();
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].path,                 "src/auth.rs");
        assert_eq!(obs[0].introduced_by_commit, "hash_abc");
        assert!(obs[0].superseded_by_commit.is_none());
    }

    #[test]
    fn supersede_path_observation_sets_superseded_commit() {
        let store = Store::open(":memory:").unwrap();
        let id = store.insert_file_identity(".").unwrap();
        store.insert_path_observation(id, "p.rs", "hash_intro", None, ".").unwrap();
        store.supersede_path_observation(id, "p.rs", "hash_super", ".").unwrap();

        let obs = store.path_history_for_identity(id, ".").unwrap();
        assert_eq!(obs[0].superseded_by_commit.as_deref(), Some("hash_super"));
    }

    #[test]
    fn identities_for_path_returns_distinct_ids() {
        let store = Store::open(":memory:").unwrap();
        let i1 = store.insert_file_identity(".").unwrap();
        let i2 = store.insert_file_identity(".").unwrap();
        // Both identities have an observation for "shared.rs" (path-reuse scenario)
        store.insert_path_observation(i1, "shared.rs", "hash_first",  None, ".").unwrap();
        store.insert_path_observation(i2, "shared.rs", "hash_second", None, ".").unwrap();

        let ids = store.identities_for_path("shared.rs", ".").unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&i1));
        assert!(ids.contains(&i2));
    }

    #[test]
    fn identities_for_path_unknown_returns_empty() {
        let store = Store::open(":memory:").unwrap();
        let ids = store.identities_for_path("ghost.rs", ".").unwrap();
        assert!(ids.is_empty());
    }

    #[test]
    fn resolve_current_path_returns_non_superseded_identity() {
        let store = Store::open(":memory:").unwrap();
        let i1 = store.insert_file_identity(".").unwrap();
        let i2 = store.insert_file_identity(".").unwrap();
        // i1's "auth.rs" is superseded (historical); i2's "auth.rs" is current
        store.insert_path_observation(i1, "auth.rs", "hash_old", Some("hash_renamed"), ".").unwrap();
        store.insert_path_observation(i2, "auth.rs", "hash_new", None, ".").unwrap();

        let current = store.resolve_current_path("auth.rs", ".").unwrap();
        assert_eq!(current, Some(i2), "current occupant must be i2, not superseded i1");
    }

    #[test]
    fn resolve_path_to_identity_returns_none_for_ambiguous_path() {
        let store = Store::open(":memory:").unwrap();
        let i1 = store.insert_file_identity(".").unwrap();
        let i2 = store.insert_file_identity(".").unwrap();
        store.insert_path_observation(i1, "p.rs", "hash_a", None, ".").unwrap();
        store.insert_path_observation(i2, "p.rs", "hash_b", None, ".").unwrap();

        // Two identities for the same path — must return None (caller should use identities_for_path)
        let result = store.resolve_path_to_identity("p.rs", ".").unwrap();
        assert!(result.is_none(), "ambiguous path must return None");
    }

    #[test]
    fn clear_file_identities_removes_all_materialized_state() {
        let store = Store::open(":memory:").unwrap();
        let id = store.insert_file_identity(".").unwrap();
        store.insert_path_observation(id, "f.rs", "hash_x", None, ".").unwrap();

        store.clear_file_identities(".").unwrap();

        let ids = store.identities_for_path("f.rs", ".").unwrap();
        assert!(ids.is_empty(), "after clear, no identities must remain");
    }

    #[test]
    fn rename_evidence_with_timestamps_ordered_oldest_first() {
        let store = Store::open(":memory:").unwrap();
        let (h1, h2, h3) = setup_rename_chain(&store);
        let _ = h1;

        let edges = store.rename_evidence_with_timestamps(".").unwrap();
        assert_eq!(edges.len(), 2);
        // h2 (ts=2000) comes before h3 (ts=3000)
        assert_eq!(edges[0].commit_hash, h2);
        assert_eq!(edges[1].commit_hash, h3);
        assert!(edges[0].timestamp < edges[1].timestamp);
    }

    #[test]
    fn commits_for_file_after_ts_returns_correct_subset() {
        let store = Store::open(":memory:").unwrap();
        let (h1, h2, h3) = setup_rename_chain(&store);
        let _ = (h1, h3);

        // h2 is at ts=2000 for p2.rs.  Adding a second commit for p2.rs at ts=4000.
        store.insert_commit(&atlas_ir::Commit {
            hash: "dddddddddddddddd".into(), short_hash: "ddddddd".into(),
            message: "later".into(), author_name: "A".into(), author_email: "a@x.com".into(),
            timestamp: DateTime::from_timestamp(4_000, 0).unwrap(),
            files_changed: vec!["p2.rs".into()],
        }, ".").unwrap();

        // Commits for p2.rs after ts=2000 (strictly): only the ts=4000 commit
        let after = store.commits_for_file_after_ts("p2.rs", 2_000, ".").unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].hash, "dddddddddddddddd");
        assert_eq!(after[0].timestamp, 4_000);
    }

    // ── file_identity_commits ───────────────────────────────────────────────

    fn setup_path_reuse_identities(store: &Store) -> (i64, i64) {
        // service.rs: S1 occupies it from ts=100 to ts=200 (rename to legacy),
        // S2 occupies it from ts=300 onward (fresh file).
        // Also creates commits so populate_identity_commits can join.
        let hx = "hash_x_111111111";
        let hy = "hash_y_222222222";
        let hz = "hash_z_333333333";

        store.insert_commit(&atlas_ir::Commit {
            hash: hx.into(), short_hash: "hash_x_".into(), message: "create S1".into(),
            author_name: "A".into(), author_email: "a@x.com".into(),
            timestamp: DateTime::from_timestamp(100, 0).unwrap(),
            files_changed: vec!["service.rs".into()],
        }, ".").unwrap();
        store.insert_commit(&atlas_ir::Commit {
            hash: hy.into(), short_hash: "hash_y_".into(), message: "rename S1".into(),
            author_name: "A".into(), author_email: "a@x.com".into(),
            timestamp: DateTime::from_timestamp(200, 0).unwrap(),
            files_changed: vec!["legacy/service.rs".into()],
        }, ".").unwrap();
        store.insert_commit(&atlas_ir::Commit {
            hash: hz.into(), short_hash: "hash_z_".into(), message: "create S2".into(),
            author_name: "A".into(), author_email: "a@x.com".into(),
            timestamp: DateTime::from_timestamp(300, 0).unwrap(),
            files_changed: vec!["service.rs".into()],
        }, ".").unwrap();

        let s1 = store.insert_file_identity(".").unwrap();
        let s2 = store.insert_file_identity(".").unwrap();

        // S1: service.rs from hx (ts=100) to hy (ts=200, the rename commit)
        store.insert_path_observation(s1, "service.rs",        hx, Some(hy), ".").unwrap();
        store.insert_path_observation(s1, "legacy/service.rs", hy, None,     ".").unwrap();
        // S2: service.rs from hz (ts=300), no superseding
        store.insert_path_observation(s2, "service.rs", hz, None, ".").unwrap();

        (s1, s2)
    }

    #[test]
    fn commits_for_identity_returns_materialized_commits() {
        let store = Store::open(":memory:").unwrap();
        let id = store.insert_file_identity(".").unwrap();
        store.insert_file_identity_commit(id, "commit_aaa", ".").unwrap();
        store.insert_file_identity_commit(id, "commit_bbb", ".").unwrap();

        // Without a real commits row, commits_for_identity returns empty (inner JOIN).
        // Verify idempotency: inserting the same commit twice is safe.
        store.insert_file_identity_commit(id, "commit_aaa", ".").unwrap();
        let count: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM file_identity_commits WHERE file_identity_id = ?1",
            params![id],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 2, "duplicate insert must not create extra rows");
    }

    #[test]
    fn populate_identity_commits_respects_temporal_boundary() {
        // Proves the path-reuse invariant at the storage layer:
        // S1 must only get commit X (ts=100, within window [100, 200)),
        // S2 must only get commit Z (ts=300, within window [300, ∞)).
        // The naive path-join bug would assign Z to S1 and X to S2.
        let store = Store::open(":memory:").unwrap();
        let (s1, s2) = setup_path_reuse_identities(&store);

        store.populate_identity_commits(".").unwrap();

        let s1_commits = store.commits_for_identity(s1, ".").unwrap();
        let s2_commits = store.commits_for_identity(s2, ".").unwrap();

        let s1_hashes: Vec<&str> = s1_commits.iter().map(|c| c.hash.as_str()).collect();
        let s2_hashes: Vec<&str> = s2_commits.iter().map(|c| c.hash.as_str()).collect();

        assert!(s1_hashes.contains(&"hash_x_111111111"), "S1 must contain commit X");
        assert!(!s1_hashes.contains(&"hash_z_333333333"),
            "S1 must NOT contain commit Z — temporal boundary violation");
        assert!(s2_hashes.contains(&"hash_z_333333333"), "S2 must contain commit Z");
        assert!(!s2_hashes.contains(&"hash_x_111111111"),
            "S2 must NOT contain commit X — temporal boundary violation");
    }

    #[test]
    fn populate_identity_commits_includes_rename_commit_in_new_path() {
        // The commit that performs a rename (e.g. commit C: auth.rs → security/auth.rs)
        // touches the NEW path (security/auth.rs) in commit_files, so it belongs
        // to the observation for security/auth.rs, not auth.rs.
        let store = Store::open(":memory:").unwrap();
        let (h1, h2, h3) = setup_rename_chain(&store);
        // setup_rename_chain: p1→p2 at h2 (ts=2000), p2→p3 at h3 (ts=3000)
        // h1 creates p1 at ts=1000

        let id = store.insert_file_identity(".").unwrap();
        // p1 observation: introduced=h1 (ts=1000), superseded=h2 (ts=2000)
        store.insert_path_observation(id, "p1.rs", &h1, Some(&h2), ".").unwrap();
        // p2 observation: introduced=h2 (ts=2000), superseded=h3 (ts=3000)
        store.insert_path_observation(id, "p2.rs", &h2, Some(&h3), ".").unwrap();
        // p3 observation: introduced=h3 (ts=3000), no superseding
        store.insert_path_observation(id, "p3.rs", &h3, None, ".").unwrap();

        store.populate_identity_commits(".").unwrap();

        let commits = store.commits_for_identity(id, ".").unwrap();
        let hashes: Vec<&str> = commits.iter().map(|c| c.hash.as_str()).collect();

        assert!(hashes.contains(&h1.as_str()), "h1 must be assigned (creates p1)");
        assert!(hashes.contains(&h2.as_str()), "h2 must be assigned (touches p2, the rename destination)");
        assert!(hashes.contains(&h3.as_str()), "h3 must be assigned (touches p3, the rename destination)");
        assert_eq!(commits.len(), 3, "exactly 3 commits in the chain");
    }

    #[test]
    fn clear_file_identities_also_clears_identity_commits() {
        let store = Store::open(":memory:").unwrap();
        let id = store.insert_file_identity(".").unwrap();
        store.insert_file_identity_commit(id, "some_commit", ".").unwrap();

        store.clear_file_identities(".").unwrap();

        let count: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM file_identity_commits WHERE repo_path = '.'",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 0, "clear must remove file_identity_commits rows");
    }
}
