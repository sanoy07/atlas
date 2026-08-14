use anyhow::Result;
use atlas_ir::Commit;
use chrono::{TimeZone, Utc};

/// Parse `git log` output produced by GitRepo::log_raw_scoped.
///
/// Format per commit:
///   \x1e<hash>\x1f<short>\x1f<parents>\x1f<author_name>\x1f<author_email>\x1f<ts>\x1f<subject>
///   [blank line]
///   <changed_file_1>
///   <changed_file_2>
///
/// `<parents>` is a space-separated list, possibly empty (root commit) or
/// containing multiple hashes (merge commits).
///
/// Backward compatible: if the header record only contains 6 fields (old
/// format without %P), parsing still succeeds with an empty parents list.
pub fn parse(raw: &str) -> Result<Vec<Commit>> {
    let mut commits: Vec<Commit> = Vec::new();
    let mut current: Option<CommitMeta> = None;
    let mut files: Vec<String> = Vec::new();

    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix('\x1e') {
            if let Some(meta) = current.take() {
                commits.push(meta.into_commit(std::mem::take(&mut files)));
            }
            let parts: Vec<&str> = rest.splitn(7, '\x1f').collect();
            current = match parts.len() {
                7 => Some(CommitMeta {
                    hash:         parts[0].to_string(),
                    short_hash:   parts[1].to_string(),
                    parents:      split_parents(parts[2]),
                    author_name:  parts[3].to_string(),
                    author_email: parts[4].to_string(),
                    timestamp:    parts[5].parse().unwrap_or(0),
                    message:      parts[6].to_string(),
                }),
                6 => Some(CommitMeta { // legacy format for backward compat
                    hash:         parts[0].to_string(),
                    short_hash:   parts[1].to_string(),
                    parents:      Vec::new(),
                    author_name:  parts[2].to_string(),
                    author_email: parts[3].to_string(),
                    timestamp:    parts[4].parse().unwrap_or(0),
                    message:      parts[5].to_string(),
                }),
                _ => None,
            };
        } else if !line.is_empty() && current.is_some() {
            files.push(line.to_string());
        }
    }

    if let Some(meta) = current {
        commits.push(meta.into_commit(files));
    }

    Ok(commits)
}

fn split_parents(field: &str) -> Vec<String> {
    field.split_whitespace().map(|s| s.to_string()).collect()
}

struct CommitMeta {
    hash:         String,
    short_hash:   String,
    parents:      Vec<String>,
    author_name:  String,
    author_email: String,
    timestamp:    i64,
    message:      String,
}

impl CommitMeta {
    fn into_commit(self, files: Vec<String>) -> Commit {
        Commit {
            hash:          self.hash,
            short_hash:    self.short_hash,
            message:       self.message,
            author_name:   self.author_name,
            author_email:  self.author_email,
            timestamp:     Utc.timestamp_opt(self.timestamp, 0).single().unwrap_or_default(),
            files_changed: files,
            parents:       self.parents,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_raw(entries: &[(&str, &str, &[&str])]) -> String {
        // Legacy 6-field format (no %P).  Verifies backward-compat path.
        entries.iter().map(|(hash, msg, files)| {
            let header = format!("\x1e{hash}\x1f{h}\x1fAlice\x1fa@x.com\x1f1720000000\x1f{msg}",
                hash = hash, h = &hash[..7.min(hash.len())], msg = msg);
            let file_lines: String = files.iter().map(|f| format!("\n{f}")).collect();
            format!("{header}{file_lines}")
        }).collect::<Vec<_>>().join("\n")
    }

    fn make_raw_with_parents(entries: &[(&str, &str, &str, &[&str])]) -> String {
        // Current 7-field format: hash, short, PARENTS, author, email, ts, subject.
        entries.iter().map(|(hash, parents, msg, files)| {
            let header = format!("\x1e{hash}\x1f{h}\x1f{p}\x1fAlice\x1fa@x.com\x1f1720000000\x1f{msg}",
                hash = hash, h = &hash[..7.min(hash.len())], p = parents, msg = msg);
            let file_lines: String = files.iter().map(|f| format!("\n{f}")).collect();
            format!("{header}{file_lines}")
        }).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn parses_commit_with_single_parent() {
        let raw = make_raw_with_parents(&[
            ("abc1234567", "def000", "second", &["src/x.rs"]),
        ]);
        let c = &parse(&raw).unwrap()[0];
        assert_eq!(c.parents, vec!["def000"]);
    }

    #[test]
    fn parses_merge_commit_with_two_parents() {
        let raw = make_raw_with_parents(&[
            ("mmmm1111", "aaa1 bbb2", "merge feature", &[]),
        ]);
        let c = &parse(&raw).unwrap()[0];
        assert_eq!(c.parents, vec!["aaa1", "bbb2"]);
    }

    #[test]
    fn parses_root_commit_with_no_parents() {
        let raw = make_raw_with_parents(&[
            ("rrrr0000", "", "root commit", &["README.md"]),
        ]);
        let c = &parse(&raw).unwrap()[0];
        assert!(c.parents.is_empty(), "root commit must have empty parents; got {:?}", c.parents);
    }

    #[test]
    fn legacy_six_field_format_still_parses_with_empty_parents() {
        // Backward-compat guard: an old-format record (no %P) still parses,
        // producing an empty parents vec rather than crashing.
        let raw = make_raw(&[("abc1234567", "legacy", &["src/x.rs"])]);
        let c = &parse(&raw).unwrap()[0];
        assert!(c.parents.is_empty());
    }

    #[test]
    fn empty_input_returns_empty() {
        assert!(parse("").unwrap().is_empty());
    }

    #[test]
    fn parses_single_commit_no_files() {
        let raw = make_raw(&[("abc1234567", "First commit", &[])]);
        let commits = parse(&raw).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].message, "First commit");
        assert!(commits[0].files_changed.is_empty());
    }

    #[test]
    fn parses_two_commits_with_files() {
        let raw = make_raw(&[
            ("abc1234567", "First commit",  &["src/main.rs"]),
            ("def7654321", "Second commit", &["src/lib.rs", "src/other.rs"]),
        ]);
        let commits = parse(&raw).unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].files_changed, vec!["src/main.rs"]);
        assert_eq!(commits[1].files_changed.len(), 2);
    }

    #[test]
    fn message_with_pipe_does_not_confuse_parser() {
        let raw = make_raw(&[("abc1234567", "feat: add foo | bar", &["a.rs"])]);
        let commits = parse(&raw).unwrap();
        assert_eq!(commits[0].message, "feat: add foo | bar");
    }
}
