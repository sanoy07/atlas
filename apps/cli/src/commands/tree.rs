use anyhow::Result;
use atlas_core::build_repository_tree;
use atlas_ir::{RepositoryTree, TreeNode, TreeNodeKind};

pub fn run(depth: Option<u32>, json: bool) -> Result<()> {
    let repo = super::discover_repo_root()?;
    let tree = build_repository_tree(&repo, depth)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&tree)?);
    } else {
        render(&tree);
    }
    Ok(())
}

fn render(tree: &RepositoryTree) {
    // Root line — directory basename with trailing '/'.
    let root_label = format!("{}/", tree.root.name);
    println!("{}", root_label);

    let last_index = tree.root.children.len().saturating_sub(1);
    for (i, child) in tree.root.children.iter().enumerate() {
        render_child(child, "", i == last_index);
    }

    if !tree.excluded.is_empty() {
        println!();
        println!("excluded: {}", tree.excluded.join(", "));
    }
}

fn render_child(node: &TreeNode, prefix: &str, is_last: bool) {
    let branch = if is_last { "└── " } else { "├── " };
    let label = match node.kind {
        TreeNodeKind::Directory => format!("{}/", node.name),
        TreeNodeKind::File      => node.name.clone(),
    };
    println!("{}{}{}", prefix, branch, label);

    if matches!(node.kind, TreeNodeKind::Directory) && !node.children.is_empty() {
        let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
        let last_index = node.children.len().saturating_sub(1);
        for (i, child) in node.children.iter().enumerate() {
            render_child(child, &child_prefix, i == last_index);
        }
    }
}
