use anyhow::Result;
use atlas_ir::RenameEvidence;

/// Parse the output of `git log --format=\x1e%H --name-status -M`.
///
/// Each commit starts with a line `\x1e<full_hash>`.  Lines beginning with `R`
/// are rename records: `R{score}\t{old_path}\t{new_path}`.  All other lines
/// (M, A, D, C, blank) are ignored.
pub fn parse(raw: &str) -> Result<Vec<RenameEvidence>> {
    let mut result      = Vec::new();
    let mut current     = "";

    for line in raw.lines() {
        if let Some(hash) = line.strip_prefix('\x1e') {
            current = hash.trim();
        } else if !current.is_empty() && line.starts_with('R') {
            if let Some(ev) = parse_rename_line(current, line) {
                result.push(ev);
            }
        }
    }

    Ok(result)
}

fn parse_rename_line(commit_hash: &str, line: &str) -> Option<RenameEvidence> {
    // Format: R{score}\t{old_path}\t{new_path}
    // score is 0–100 (e.g. R100, R75, R52).
    let mut parts = line.splitn(3, '\t');
    let status    = parts.next()?;
    let old_path  = parts.next()?.trim();
    let new_path  = parts.next()?.trim();

    if old_path.is_empty() || new_path.is_empty() { return None; }

    let score_str = status.get(1..)?;
    let similarity_score: u8 = score_str.parse().ok()?;

    Some(RenameEvidence {
        commit_hash:      commit_hash.to_string(),
        old_path:         old_path.to_string(),
        new_path:         new_path.to_string(),
        similarity_score,
        detection_source: "git-rename".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_raw(entries: &[(&str, &[(&str, &str, u8)])]) -> String {
        entries.iter().map(|(hash, renames)| {
            let rename_lines: String = renames.iter()
                .map(|(old, new, score)| format!("R{score}\t{old}\t{new}\n"))
                .collect();
            format!("\x1e{hash}\n\n{rename_lines}")
        }).collect()
    }

    #[test]
    fn empty_input_returns_empty() {
        assert!(parse("").unwrap().is_empty());
    }

    #[test]
    fn parses_single_exact_rename() {
        let raw = make_raw(&[("abc1234567890", &[("old/auth.rs", "new/auth.rs", 100)])]);
        let evs = parse(&raw).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].commit_hash,      "abc1234567890");
        assert_eq!(evs[0].old_path,         "old/auth.rs");
        assert_eq!(evs[0].new_path,         "new/auth.rs");
        assert_eq!(evs[0].similarity_score, 100);
        assert_eq!(evs[0].detection_source, "git-rename");
    }

    #[test]
    fn parses_rename_with_modification() {
        let raw = make_raw(&[("def9876543210", &[("src/auth.rs", "crates/auth/src/lib.rs", 75)])]);
        let evs = parse(&raw).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].similarity_score, 75);
    }

    #[test]
    fn ignores_non_rename_status_lines() {
        // M (modified), A (added), D (deleted) lines must be ignored.
        let raw = format!(
            "\x1eabc123\n\nM\tsrc/lib.rs\nA\tsrc/new.rs\nD\tsrc/old.rs\nR100\tsrc/a.rs\tsrc/b.rs\n"
        );
        let evs = parse(&raw).unwrap();
        assert_eq!(evs.len(), 1, "only the R line should produce evidence");
        assert_eq!(evs[0].old_path, "src/a.rs");
    }

    #[test]
    fn parses_multiple_renames_in_one_commit() {
        let raw = make_raw(&[(
            "multi0000000000",
            &[
                ("a/old.rs", "a/new.rs",   100),
                ("b/old.rs", "b/new.rs",   80),
            ],
        )]);
        let evs = parse(&raw).unwrap();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].old_path, "a/old.rs");
        assert_eq!(evs[1].old_path, "b/old.rs");
    }

    #[test]
    fn parses_renames_across_multiple_commits() {
        let raw = make_raw(&[
            ("commit_aaaaaaa", &[("x.rs", "y.rs", 100)]),
            ("commit_bbbbbbb", &[("y.rs", "z.rs", 90)]),
        ]);
        let evs = parse(&raw).unwrap();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].commit_hash, "commit_aaaaaaa");
        assert_eq!(evs[1].commit_hash, "commit_bbbbbbb");
    }

    #[test]
    fn commit_with_no_renames_produces_no_evidence() {
        let raw = "\x1eabc1230000\n\nM\tsome.rs\nA\tother.rs\n";
        let evs = parse(raw).unwrap();
        assert!(evs.is_empty());
    }

    #[test]
    fn malformed_rename_line_is_skipped() {
        // Line has R but not enough tab-separated fields.
        let raw = "\x1eabc1230001\n\nRXXX\tbadonly\n";
        let evs = parse(raw).unwrap();
        assert!(evs.is_empty(), "malformed R line must not panic — just skip");
    }

    #[test]
    fn paths_with_spaces_are_parsed_correctly() {
        let raw = "\x1eabc1230002\n\nR100\told path/a.rs\tnew path/b.rs\n";
        let evs = parse(raw).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].old_path, "old path/a.rs");
        assert_eq!(evs[0].new_path, "new path/b.rs");
    }
}
