use anyhow::Result;
use atlas_ir::Commit;
use chrono::{TimeZone, Utc};

pub fn parse(raw: &str) -> Result<Vec<Commit>> {
    let mut commits: Vec<Commit> = Vec::new();
    let mut current: Option<CommitMeta> = None;
    let mut files: Vec<String> = Vec::new();

    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix('\x1e') {
            if let Some(meta) = current.take() {
                commits.push(meta.into_commit(std::mem::take(&mut files)));
            }
            // format: hash\x1fshort\x1fname\x1femail\x1ftimestamp\x1fsubject
            let parts: Vec<&str> = rest.splitn(6, '\x1f').collect();
            if parts.len() == 6 {
                current = Some(CommitMeta {
                    hash:         parts[0].to_string(),
                    short_hash:   parts[1].to_string(),
                    author_name:  parts[2].to_string(),
                    author_email: parts[3].to_string(),
                    timestamp:    parts[4].parse().unwrap_or(0),
                    message:      parts[5].to_string(),
                });
            }
        } else if !line.is_empty() && current.is_some() {
            files.push(line.to_string());
        }
    }

    if let Some(meta) = current {
        commits.push(meta.into_commit(files));
    }

    Ok(commits)
}

struct CommitMeta {
    hash:         String,
    short_hash:   String,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_raw(entries: &[(&str, &str, &[&str])]) -> String {
        entries.iter().map(|(hash, msg, files)| {
            let header = format!("\x1e{hash}\x1f{h}\x1fAlice\x1fa@x.com\x1f1720000000\x1f{msg}",
                hash = hash, h = &hash[..7.min(hash.len())], msg = msg);
            let file_lines: String = files.iter().map(|f| format!("\n{f}")).collect();
            format!("{header}{file_lines}")
        }).collect::<Vec<_>>().join("\n")
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
