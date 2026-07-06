use anyhow::Result;
use atlas_ir::{Commit, Issue, PullRequest};
use rusqlite::{params, Connection};
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
            "SELECT c.short_hash, c.message, c.author_name, c.timestamp
             FROM commits c
             JOIN commit_files cf ON c.hash = cf.commit_hash
             WHERE cf.file_path = ?1 AND c.repo_path = ?2
             ORDER BY c.timestamp DESC",
        )?;

        let rows = stmt.query_map(params![file_path, repo_path], |row| {
            Ok(CommitRow {
                short_hash:  row.get(0)?,
                message:     row.get(1)?,
                author_name: row.get(2)?,
                timestamp:   row.get(3)?,
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
            "SELECT c.short_hash, c.message, c.author_name, c.timestamp
             FROM commits c
             JOIN commit_files cf ON c.hash = cf.commit_hash
             WHERE cf.file_path = ?1 AND c.repo_path = ?2
             ORDER BY c.timestamp ASC
             LIMIT 1",
        )?;

        let mut rows = stmt.query_map(params![file_path, repo_path], |row| {
            Ok(CommitRow {
                short_hash:  row.get(0)?,
                message:     row.get(1)?,
                author_name: row.get(2)?,
                timestamp:   row.get(3)?,
            })
        })?;

        Ok(rows.next().transpose()?)
    }

    /// The most recent commit that touched `file_path`.
    pub fn last_seen(&self, file_path: &str, repo_path: &str) -> Result<Option<CommitRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.short_hash, c.message, c.author_name, c.timestamp
             FROM commits c
             JOIN commit_files cf ON c.hash = cf.commit_hash
             WHERE cf.file_path = ?1 AND c.repo_path = ?2
             ORDER BY c.timestamp DESC
             LIMIT 1",
        )?;

        let mut rows = stmt.query_map(params![file_path, repo_path], |row| {
            Ok(CommitRow {
                short_hash:  row.get(0)?,
                message:     row.get(1)?,
                author_name: row.get(2)?,
                timestamp:   row.get(3)?,
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
            "SELECT c.short_hash, c.message, c.author_name, c.timestamp
             FROM commits c
             JOIN commit_files cf ON c.hash = cf.commit_hash
             WHERE cf.file_path = ?1 AND c.repo_path = ?2
             ORDER BY c.timestamp ASC",
        )?;

        let rows = stmt.query_map(params![file_path, repo_path], |row| {
            Ok(CommitRow {
                short_hash:  row.get(0)?,
                message:     row.get(1)?,
                author_name: row.get(2)?,
                timestamp:   row.get(3)?,
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

    pub fn link_pr_to_issue(&self, pr_number: i64, issue_number: i64, repo_path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO pr_issues (pr_number, issue_number, repo_path)
             VALUES (?1, ?2, ?3)",
            params![pr_number, issue_number, repo_path],
        )?;
        Ok(())
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
}

#[derive(Debug)]
pub struct CommitRow {
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
}
