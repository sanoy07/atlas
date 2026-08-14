//! Step 2 acceptance tests for `build_repository_tree`.
//!
//! Covers: depth semantics (0, 1, unlimited), RepoAwareness exclusion of
//! defaults (dist/, node_modules/, target/, etc.), .git skip, deterministic
//! ordering, .gitignore prefix honouring, and relative-path normalisation.

use atlas_core::build_repository_tree;
use atlas_ir::{TreeNode, TreeNodeKind};
use std::path::Path;
use tempfile::TempDir;

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn mkdir(root: &Path, rel: &str) {
    std::fs::create_dir_all(root.join(rel)).unwrap();
}

/// Flatten every node in the tree into a Vec of `(relative_path, kind)` pairs
/// for easy assertion.  Root has `relative_path == ""`.
fn flatten(node: &TreeNode) -> Vec<(String, TreeNodeKind)> {
    let mut out = Vec::new();
    out.push((node.relative_path.clone(), node.kind));
    for child in &node.children {
        out.extend(flatten(child));
    }
    out
}

fn child_names(node: &TreeNode) -> Vec<String> {
    node.children.iter().map(|c| c.name.clone()).collect()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn tree_depth_zero_returns_root_with_no_children() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "README.md", "hi");
    write(dir.path(), "src/index.rs", "");
    write(dir.path(), "docs/guide.md", "");

    let tree = build_repository_tree(dir.path().to_str().unwrap(), Some(0)).unwrap();

    assert_eq!(tree.root.kind, TreeNodeKind::Directory);
    assert_eq!(tree.root.relative_path, "");
    assert!(
        tree.root.children.is_empty(),
        "depth 0 must return root without any children"
    );
    // Root name is the basename of the tempdir path.
    let expected_name = dir.path().file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(tree.root.name, expected_name);
    assert_eq!(tree.depth_limit, Some(0));
    assert_eq!(tree.schema_version, 1);
}

#[test]
fn tree_depth_one_lists_immediate_children_only() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "README.md", "hi");
    write(dir.path(), "src/index.rs", "");
    write(dir.path(), "docs/guide.md", "");

    let tree = build_repository_tree(dir.path().to_str().unwrap(), Some(1)).unwrap();

    // Alphabetical order: README.md, docs, src.
    assert_eq!(child_names(&tree.root), vec!["README.md", "docs", "src"]);

    for child in &tree.root.children {
        match child.kind {
            TreeNodeKind::Directory => assert!(
                child.children.is_empty(),
                "directory {} at depth 1 must have empty children", child.name
            ),
            TreeNodeKind::File => assert!(child.children.is_empty()),
        }
    }
}

#[test]
fn tree_unlimited_depth_walks_all_files() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "src/a/b/c/leaf.rs", "");

    let tree = build_repository_tree(dir.path().to_str().unwrap(), None).unwrap();

    let flat = flatten(&tree.root);
    let paths: Vec<&str> = flat.iter().map(|(p, _)| p.as_str()).collect();
    assert!(paths.contains(&"src/a/b/c/leaf.rs"),
        "unlimited depth must reach leaf; got {:?}", paths);
    assert!(paths.contains(&"src/a/b/c"), "intermediate directory must exist");
    assert_eq!(tree.depth_limit, None);
}

#[test]
fn tree_excludes_repo_awareness_defaults() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "src/main.rs", "");
    write(dir.path(), "node_modules/pkg/index.js", "");
    write(dir.path(), "dist/bundle.js", "");
    write(dir.path(), "target/debug/foo",  "");

    let tree = build_repository_tree(dir.path().to_str().unwrap(), None).unwrap();

    let flat = flatten(&tree.root);
    let paths: Vec<&str> = flat.iter().map(|(p, _)| p.as_str()).collect();

    assert!(paths.contains(&"src/main.rs"), "src/main.rs must be present");
    for excluded in ["node_modules", "dist", "target"] {
        assert!(
            !paths.iter().any(|p| p == &excluded || p.starts_with(&format!("{}/", excluded))),
            "{} must not appear anywhere in tree; got {:?}", excluded, paths
        );
        assert!(
            tree.excluded.iter().any(|p| p == excluded),
            "{} must be reported in tree.excluded; got {:?}", excluded, tree.excluded
        );
    }
}

#[test]
fn tree_excludes_dot_git_at_root() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), ".git/HEAD", "ref: refs/heads/main\n");
    write(dir.path(), "src/main.rs", "");

    let tree = build_repository_tree(dir.path().to_str().unwrap(), None).unwrap();

    let flat = flatten(&tree.root);
    let paths: Vec<&str> = flat.iter().map(|(p, _)| p.as_str()).collect();

    assert!(
        !paths.iter().any(|p| p == &".git" || p.starts_with(".git/")),
        ".git must be pruned; got {:?}", paths
    );
    assert!(tree.excluded.contains(&".git".to_string()),
        "expected .git in tree.excluded; got {:?}", tree.excluded);
}

#[test]
fn tree_deterministic_alphabetical_order() {
    let dir = TempDir::new().unwrap();
    // Create children out of alphabetical order to prove sorting isn't just
    // the OS returning them in creation order by accident.
    write(dir.path(), "z_last.rs",  "");
    write(dir.path(), "a_first.rs", "");
    write(dir.path(), "m_mid.rs",   "");
    mkdir(dir.path(), "zeta_dir");
    mkdir(dir.path(), "alpha_dir");

    let tree = build_repository_tree(dir.path().to_str().unwrap(), Some(1)).unwrap();

    assert_eq!(
        child_names(&tree.root),
        vec!["a_first.rs", "alpha_dir", "m_mid.rs", "z_last.rs", "zeta_dir"],
        "children must be case-sensitive alphabetical"
    );
}

#[test]
fn tree_gitignore_prefixes_honoured() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), ".gitignore", "custom_build/\n");
    write(dir.path(), "custom_build/artifact", "");
    write(dir.path(), "src/main.rs", "");

    let tree = build_repository_tree(dir.path().to_str().unwrap(), None).unwrap();

    let flat = flatten(&tree.root);
    let paths: Vec<&str> = flat.iter().map(|(p, _)| p.as_str()).collect();
    assert!(
        !paths.iter().any(|p| p == &"custom_build" || p.starts_with("custom_build/")),
        "custom_build/ from .gitignore must be pruned; got {:?}", paths
    );
    assert!(tree.excluded.contains(&"custom_build".to_string()));
}

#[test]
fn tree_relative_path_normalised() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "src/lib/mod.rs", "");

    let tree = build_repository_tree(dir.path().to_str().unwrap(), None).unwrap();

    assert_eq!(tree.root.relative_path, "");
    for (path, _) in flatten(&tree.root) {
        assert!(!path.contains('\\'), "no backslashes in relative_path: {}", path);
        assert!(!path.starts_with('/'), "no leading slash: {}", path);
        assert!(!path.ends_with('/'),   "no trailing slash: {}", path);
    }

    // The nested file's relative path is normalised.
    let flat = flatten(&tree.root);
    assert!(flat.iter().any(|(p, k)| p == "src/lib/mod.rs" && *k == TreeNodeKind::File));
    assert!(flat.iter().any(|(p, k)| p == "src/lib"        && *k == TreeNodeKind::Directory));
    assert!(flat.iter().any(|(p, k)| p == "src"            && *k == TreeNodeKind::Directory));
}
