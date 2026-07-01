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
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA)?;
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
               (number, title, state, body, author, merge_commit_sha, repo_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                pr.number,
                pr.title,
                pr.state,
                pr.body,
                pr.author,
                pr.merge_commit_sha,
                repo_path,
            ],
        )?;
        Ok(())
    }

    pub fn insert_issue(&self, issue: &Issue, repo_path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO issues
               (number, title, state, body, author, repo_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                issue.number,
                issue.title,
                issue.state,
                issue.body,
                issue.author,
                repo_path,
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
            "SELECT DISTINCT pr.number, pr.title, pr.state, pr.author
             FROM pull_requests pr
             JOIN commits c ON c.hash = pr.merge_commit_sha AND c.repo_path = pr.repo_path
             JOIN commit_files cf ON cf.commit_hash = c.hash
             WHERE cf.file_path = ?1 AND pr.repo_path = ?2",
        )?;

        let rows = stmt.query_map(params![file_path, repo_path], |row| {
            Ok(PrRow {
                number: row.get(0)?,
                title:  row.get(1)?,
                state:  row.get(2)?,
                author: row.get(3)?,
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
    pub number: i64,
    pub title:  String,
    pub state:  String,
    pub author: String,
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
    PRIMARY KEY (number, repo_path)
);

CREATE TABLE IF NOT EXISTS issues (
    number    INTEGER NOT NULL,
    title     TEXT NOT NULL,
    state     TEXT NOT NULL,
    body      TEXT,
    author    TEXT,
    repo_path TEXT NOT NULL,
    PRIMARY KEY (number, repo_path)
);
";

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_ir::Commit;
    use chrono::DateTime;

    fn test_commit(hash: &str, file: &str) -> Commit {
        Commit {
            hash:          hash.into(),
            short_hash:    hash[..7.min(hash.len())].into(),
            message:       "test commit".into(),
            author_name:   "Alice".into(),
            author_email:  "a@x.com".into(),
            timestamp:     DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            files_changed: vec![file.into()],
        }
    }

    #[test]
    fn insert_and_query_commit() {
        let store = Store::open(":memory:").unwrap();
        let c = test_commit("abc1234", "src/main.rs");
        store.insert_commit(&c, ".").unwrap();

        let rows = store.commits_for_file("src/main.rs", ".").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].message, "test commit");
    }

    #[test]
    fn insert_is_idempotent() {
        let store = Store::open(":memory:").unwrap();
        let c = test_commit("abc1234", "src/main.rs");
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
}
