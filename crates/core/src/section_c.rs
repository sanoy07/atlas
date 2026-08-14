//! Section C — Map / Focus / Impact.
//!
//! Composes existing B-layer and inspect/structural/history APIs into
//! claim-oriented orientation reports.  No new extractors.  No LLM.
//! Epistemic layers: observed | derived | inferred | unknown.

use crate::{
    compute_config_inventory, compute_dependency_linkage, compute_module_coupling,
    compute_modules, compute_test_module_links, detect_peer_structure, inspect,
};
use anyhow::Result;
use atlas_ir::{
    EpistemicLayer, EvidenceDimensions, EvidenceRef, FocusReport, HistoricalRedirect,
    ImpactNeighbor, ImpactReport, MapReport, OrientationClaim,
};
use atlas_storage::Store;

const MAP_SCHEMA: u32 = 1;
const FOCUS_SCHEMA: u32 = 1;
const IMPACT_SCHEMA: u32 = 1;

fn git_head_short(repo_path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["-C", repo_path, "rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn claim(
    id: &str,
    subject: &str,
    statement: &str,
    layer: EpistemicLayer,
    method: &str,
    evidence: Vec<EvidenceRef>,
    limitations: Vec<String>,
) -> OrientationClaim {
    OrientationClaim {
        id: id.into(),
        subject: subject.into(),
        statement: statement.into(),
        layer,
        evidence,
        method: method.into(),
        limitations,
    }
}

fn eref(kind: &str, id: &str, summary: &str) -> EvidenceRef {
    EvidenceRef {
        kind: kind.into(),
        id: id.into(),
        summary: summary.into(),
        timestamp: None,
    }
}

/// Choose modules subject: prefer non-empty domain roots.
///
/// Order: `src/modules` (Nest-style) → discovered code roots (`src`, `lib/src`,
/// `cli/src`, `crates/*/src`, …) by child density → fallback `src`.
pub fn resolve_modules_subject(repo_path: &str, store: &Store) -> Result<String> {
    let m = compute_modules("src/modules", repo_path, store)?;
    if m.total_modules > 0 {
        return Ok("src/modules".into());
    }
    // Prefer roots that actually have child modules (layout-aware, not src-only).
    if let Ok(roots) = crate::subject_resolve::discover_code_roots(repo_path, store) {
        let mut best: Option<(String, usize)> = None;
        for root in roots {
            if let Ok(r) = compute_modules(&root, repo_path, store) {
                if r.total_modules > 0 {
                    let n = r.total_modules as usize;
                    if best.as_ref().map(|(_, c)| n > *c).unwrap_or(true) {
                        best = Some((root, n));
                    }
                }
            }
        }
        if let Some((root, _)) = best {
            return Ok(root);
        }
    }
    let s = compute_modules("src", repo_path, store)?;
    if s.total_modules > 0 {
        return Ok("src".into());
    }
    // Monorepo library root even if "modules" count is flat files only
    let lib = compute_modules("lib/src", repo_path, store)?;
    if lib.total_modules > 0 {
        return Ok("lib/src".into());
    }
    Ok("src".into())
}

// ─── C1 Map ─────────────────────────────────────────────────────────────────

/// Repository orientation map from existing aggregations.
pub fn build_map(repo_path: &str, store: &Store) -> Result<MapReport> {
    let modules_subject = resolve_modules_subject(repo_path, store)?;
    let modules_report = compute_modules(&modules_subject, repo_path, store)?;
    let modules: Vec<String> = modules_report.modules.iter().map(|m| m.name.clone()).collect();

    let coupling = compute_module_coupling(&modules_subject, repo_path, store)?;
    let top_coupling: Vec<(String, String, usize)> = coupling
        .cells
        .iter()
        .take(12)
        .map(|c| {
            (
                c.source_module.clone(),
                c.target_module.clone(),
                c.edge_count,
            )
        })
        .collect();

    let hot = store.hot_files(repo_path, 15)?;
    let hot_files: Vec<(String, i64)> = hot
        .iter()
        .map(|h| (h.file_path.clone(), h.touch_count))
        .collect();

    let config = compute_config_inventory(repo_path, store)?;
    let config_artifacts: Vec<String> = config
        .artifacts
        .iter()
        .map(|a| a.file_path.clone())
        .collect();

    let peers = detect_peer_structure(&modules_subject, repo_path, store)?;
    let deps = compute_dependency_linkage(repo_path, store)?;
    let tests = compute_test_module_links(&modules_subject, None, repo_path, store)?;

    let edge_count = store.structural_edge_count(repo_path).unwrap_or(0);
    let commit_count = store.commit_count(repo_path).unwrap_or(0);

    let mut claims = Vec::new();
    let mut cid = 0u32;
    let mut next_id = || {
        cid += 1;
        format!("map-{cid:03}")
    };

    claims.push(claim(
        &next_id(),
        &modules_subject,
        &format!(
            "Module/layer inventory under `{}` has {} child director{}.",
            modules_subject,
            modules.len(),
            if modules.len() == 1 { "y" } else { "ies" }
        ),
        EpistemicLayer::Observed,
        "compute_modules immediate children of subject from files table",
        vec![eref(
            "method",
            "compute_modules",
            &format!("total_modules={}", modules.len()),
        )],
        vec![
            "Names are path segments, not business-domain labels.".into(),
            "Historical paths in files table may appear as modules.".into(),
        ],
    ));

    if let Some(top) = modules_report
        .modules
        .iter()
        .max_by_key(|m| m.observed_commit_count)
    {
        if top.observed_commit_count > 0 {
            claims.push(claim(
                &next_id(),
                &top.path,
                &format!(
                    "`{}` has the highest observed commit touch count under the inventory ({} commits).",
                    top.name, top.observed_commit_count
                ),
                EpistemicLayer::Derived,
                "max(observed_commit_count) over ModuleEntry",
                vec![eref(
                    "module",
                    &top.path,
                    &format!("commits={}", top.observed_commit_count),
                )],
                vec!["Commit count is not ownership or importance.".into()],
            ));
        }
    }

    if let Some(cell) = coupling.cells.first() {
        claims.push(claim(
            &next_id(),
            &format!("{}→{}", cell.source_module, cell.target_module),
            &format!(
                "Strongest observed cross-module coupling cell: {} → {} ({} structural edges).",
                cell.source_module, cell.target_module, cell.edge_count
            ),
            EpistemicLayer::Observed,
            "compute_module_coupling sparse cells sorted by edge_count",
            vec![eref(
                "structural",
                &format!("{}→{}", cell.source_module, cell.target_module),
                &format!("{} edges", cell.edge_count),
            )],
            vec!["Static edges only; snapshot of working tree at last structural ingest.".into()],
        ));
    }

    if let Some((path, n)) = hot_files.first() {
        claims.push(claim(
            &next_id(),
            path,
            &format!("Hottest file by commit touches: `{path}` ({n} commits)."),
            EpistemicLayer::Observed,
            "hot_files(repo, 15)",
            vec![eref("file", path, &format!("touches={n}"))],
            vec!["Path-scoped hot files; identity-aware hot files not used in map v1.".into()],
        ));
    }

    if !peers.patterns.is_empty() {
        let p = &peers.patterns[0];
        claims.push(claim(
            &next_id(),
            &peers.peer_parent,
            &format!(
                "Most prevalent peer structural pattern: `{}` in {}/{} peers.",
                p.element, p.prevalence_num, p.prevalence_den
            ),
            EpistemicLayer::Derived,
            "detect_peer_structure patterns prevalence",
            vec![eref(
                "pattern",
                &p.element,
                &format!("{}/{}", p.prevalence_num, p.prevalence_den),
            )],
            vec!["Prevalence is not a style judgment.".into()],
        ));
    }

    claims.push(claim(
        &next_id(),
        "package.json",
        &format!(
            "Dependency linkage: {} declared, {} observed via structural edges, {} both.",
            deps.total_declared, deps.total_observed, deps.declared_and_observed
        ),
        EpistemicLayer::Derived,
        "compute_dependency_linkage",
        vec![eref(
            "config",
            "package.json",
            &deps.declaration_provenance,
        )],
        vec!["OBSERVED ≠ runtime usage.".into()],
    ));

    claims.push(claim(
        &next_id(),
        "tests",
        &format!(
            "Test↔module links: {} links from {} test files under path rules ({} unlinked).",
            tests.total_links,
            tests.total_test_files,
            tests.unlinked_tests.len()
        ),
        EpistemicLayer::Derived,
        "compute_test_module_links",
        vec![eref(
            "method",
            "test_linkage",
            &format!("links={}", tests.total_links),
        )],
        vec!["Path rules only; unlinked is not 'orphan ownership'.".into()],
    ));

    if !config_artifacts.is_empty() {
        claims.push(claim(
            &next_id(),
            "configuration",
            &format!(
                "{} configuration artifact(s) in configuration_artifacts.",
                config_artifacts.len()
            ),
            EpistemicLayer::Observed,
            "compute_config_inventory",
            config_artifacts
                .iter()
                .take(5)
                .map(|p| eref("config", p, "ingested artifact"))
                .collect(),
            vec!["Current content only; no historical config bodies.".into()],
        ));
    }

    let mut coverage_notes = vec![
        format!("commits_in_db≈{commit_count}"),
        format!("structural_edges={edge_count}"),
        format!("modules_subject={modules_subject}"),
    ];
    if edge_count == 0 {
        coverage_notes.push("No structural edges — map coupling cells empty.".into());
    }

    let limitations = vec![
        "Map is orientation, not architectural meaning or quality judgment.".into(),
        "Structural data is a working-tree snapshot from last structural ingest.".into(),
        "Git history scope depends on last ingest (often HEAD-only).".into(),
        "No runtime DI, dynamic dispatch, or production traffic.".into(),
    ];

    // Unknown layer claim when structure missing
    if edge_count == 0 {
        claims.push(claim(
            &next_id(),
            "structure",
            "Structural graph not available for this repository in the current DB.",
            EpistemicLayer::Unknown,
            "structural_edge_count == 0",
            vec![],
            vec!["Run atlas ingest . --typescript (or language-specific stage).".into()],
        ));
    }

    Ok(MapReport {
        schema_version: MAP_SCHEMA,
        repo_path: repo_path.to_string(),
        git_head: git_head_short(repo_path),
        modules_subject,
        modules,
        claims,
        hot_files,
        top_coupling,
        config_artifacts,
        coverage_notes,
        limitations,
    })
}

// ─── C2 Focus ───────────────────────────────────────────────────────────────

/// Local neighborhood for a path, module name, or file.
pub fn build_focus(subject: &str, repo_path: &str, store: &Store) -> Result<FocusReport> {
    let raw = subject.trim().trim_start_matches('/');
    let modules_subject = resolve_modules_subject(repo_path, store)?;

    // Resolve module bare name → path
    let (focus_path, subject_kind) = if raw.contains('/') {
        let kind = if store
            .all_file_paths(repo_path)?
            .iter()
            .any(|p| p.starts_with(&format!("{raw}/")) || p == raw)
        {
            if raw.contains('.') {
                "file"
            } else {
                "directory"
            }
        } else {
            "path"
        };
        (raw.to_string(), kind.to_string())
    } else {
        // bare module name
        let path = format!("{modules_subject}/{raw}");
        (path, "module".to_string())
    };

    let mut redirect_note: Option<HistoricalRedirect> = None;
    let mut working = focus_path.clone();
    if subject_kind == "file" || focus_path.contains('.') {
        if let Some(current) = store.current_path_if_historical(&focus_path, repo_path)? {
            let id = store
                .resolve_path_to_identity(&current, repo_path)?
                .unwrap_or(-1);
            redirect_note = Some(HistoricalRedirect {
                original_subject: focus_path.clone(),
                current_path: current.clone(),
                identity_id: id,
            });
            working = current;
        }
    }

    let insp = inspect(&working, repo_path, store)?;

    let mut incoming: Vec<String> = Vec::new();
    let mut outgoing: Vec<String> = Vec::new();
    // InspectionDocument fields - check structure
    // Use structural edges directly for reliability
    let out_edges = store.structural_edges_for_file(&working, repo_path)?;
    for e in &out_edges {
        outgoing.push(format!("{} → {} [{}]", e.source_file, e.target_file, e.kind));
    }
    let in_edges = store.structural_edges_targeting(&working, repo_path)?;
    for e in &in_edges {
        incoming.push(format!("{} → {} [{}]", e.source_file, e.target_file, e.kind));
    }
    // directory: prefix
    if subject_kind == "directory" || subject_kind == "module" {
        let prefix = format!("{working}/");
        let from = store.structural_edges_from_prefix(&prefix, repo_path)?;
        for e in from.iter().take(40) {
            if !e.target_file.starts_with(&prefix) {
                outgoing.push(format!(
                    "{} → {} [{}]",
                    e.source_file, e.target_file, e.kind
                ));
            }
        }
        let to = store.structural_edges_to_prefix(&prefix, repo_path)?;
        for e in to.iter().take(40) {
            if !e.source_file.starts_with(&prefix) {
                incoming.push(format!(
                    "{} → {} [{}]",
                    e.source_file, e.target_file, e.kind
                ));
            }
        }
        outgoing.sort();
        outgoing.dedup();
        incoming.sort();
        incoming.dedup();
    }

    let tests = compute_test_module_links(&modules_subject, None, repo_path, store)?;
    let related_tests: Vec<String> = tests
        .links
        .iter()
        .filter(|l| {
            working.starts_with(&l.module_path)
                || l.module_path.starts_with(&working)
                || l.test_path.contains(&working)
                || working.contains(&l.module_name)
        })
        .map(|l| format!("{} → {} ({:?})", l.test_path, l.module_name, l.linkage_kind))
        .take(20)
        .collect();

    let deps = compute_dependency_linkage(repo_path, store)?;
    let packages_observed: Vec<String> = deps
        .packages
        .iter()
        .filter(|p| {
            p.is_observed
                && p.observations
                    .iter()
                    .any(|o| o.source_file.starts_with(&working) || o.source_file == working)
        })
        .map(|p| p.package_name.clone())
        .take(20)
        .collect();

    let commits = if subject_kind == "file" {
        store.commits_for_file(&working, repo_path)?
    } else {
        store.commits_under_prefix(&format!("{working}/"), repo_path)?
    };
    let recent_commits: Vec<String> = commits
        .iter()
        .take(12)
        .map(|c| format!("{} {} — {}", c.short_hash, c.timestamp, truncate(&c.message, 70)))
        .collect();

    let author_lines: Vec<String> = if subject_kind == "file" {
        store
            .authors_for_file(&working, repo_path)?
            .into_iter()
            .take(10)
            .map(|a| {
                format!(
                    "{} <{}> commits={}",
                    a.author_name, a.author_email, a.commit_count
                )
            })
            .collect()
    } else {
        let prefix = format!("{working}/");
        store
            .authors_for_prefix(&prefix, repo_path)?
            .into_iter()
            .take(10)
            .map(|a| {
                format!(
                    "{} <{}> commits={}",
                    a.author_name, a.author_email, a.commit_count
                )
            })
            .collect()
    };

    let related_docs: Vec<String> = store
        .documents_under_prefix(&format!("{working}/"), repo_path)
        .unwrap_or_default()
        .into_iter()
        .take(15)
        .map(|(path, dtype, title)| format!("{path} [{dtype}] {title}"))
        .collect();

    let mut claims = Vec::new();
    let mut cid = 0u32;
    let mut next_id = || {
        cid += 1;
        format!("focus-{cid:03}")
    };

    claims.push(claim(
        &next_id(),
        &working,
        &format!(
            "Focus subject resolved as {subject_kind} path `{working}` ({} outgoing, {} incoming edge rows listed).",
            outgoing.len(),
            incoming.len()
        ),
        EpistemicLayer::Observed,
        "structural_edges_for_file / from_prefix / to_prefix",
        vec![eref("path", &working, "focus subject")],
        vec!["Edge list may be truncated for large subtrees.".into()],
    ));

    if !related_tests.is_empty() {
        claims.push(claim(
            &next_id(),
            &working,
            &format!(
                "{} test linkage row(s) associated under documented path rules.",
                related_tests.len()
            ),
            EpistemicLayer::Derived,
            "compute_test_module_links filter",
            related_tests
                .iter()
                .take(3)
                .map(|t| eref("file", t, "test link"))
                .collect(),
            vec!["Path rules only.".into()],
        ));
    }

    if !author_lines.is_empty() {
        claims.push(claim(
            &next_id(),
            &working,
            &format!(
                "{} author tuple(s) with observed commits on this subject (not ownership).",
                author_lines.len()
            ),
            EpistemicLayer::Observed,
            "authors_for_file / authors_for_prefix",
            vec![eref("method", "authors", "observed commits")],
            vec!["Exact (name,email) tuples; no alias merge.".into()],
        ));
    }

    let limitations = vec![
        "Focus is a neighborhood pack, not a complete call graph.".into(),
        "Authors are observed commit tuples, not owners or experts.".into(),
        "Structural edges are snapshot-only.".into(),
        "InspectionDocument fields may partially overlap edge lists.".into(),
    ];
    let _ = insp; // ensure inspect ran for side-effect free validation of path

    Ok(FocusReport {
        schema_version: FOCUS_SCHEMA,
        subject: working,
        subject_kind,
        redirect_note,
        claims,
        incoming: incoming.into_iter().take(50).collect(),
        outgoing: outgoing.into_iter().take(50).collect(),
        related_tests,
        packages_observed: packages_observed,
        recent_commits,
        authors: author_lines,
        related_docs,
        limitations,
    })
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>())
    }
}

// ─── C3 Impact ──────────────────────────────────────────────────────────────

/// Blast-radius style neighbors for a file path (module path uses prefix).
pub fn build_impact(subject: &str, repo_path: &str, store: &Store) -> Result<ImpactReport> {
    let raw = subject.trim().trim_start_matches('/');
    let mut working = raw.to_string();
    if let Some(current) = store.current_path_if_historical(raw, repo_path)? {
        working = current;
    }

    let is_file = working.contains('.')
        && store
            .all_file_paths(repo_path)?
            .iter()
            .any(|p| p == &working);

    // Collect neighbor scores
    use std::collections::HashMap;
    #[derive(Default)]
    struct Acc {
        structural: f32,
        cochange: f32,
        reasons: Vec<String>,
        evidence: Vec<EvidenceRef>,
        is_test: bool,
    }
    let mut map: HashMap<String, Acc> = HashMap::new();

    if is_file {
        for e in store.structural_edges_for_file(&working, repo_path)? {
            let t = e.target_file.clone();
            if t.starts_with("UNRESOLVED:") {
                continue;
            }
            let a = map.entry(t.clone()).or_default();
            a.structural += 1.0;
            a.reasons.push(format!("outgoing {}", e.kind));
            a.evidence.push(eref(
                "structural",
                &format!("{working}→{t}"),
                &e.kind,
            ));
        }
        for e in store.structural_edges_targeting(&working, repo_path)? {
            let s = e.source_file.clone();
            let a = map.entry(s.clone()).or_default();
            a.structural += 1.5; // reverse dependency slightly higher for impact
            a.reasons.push(format!("incoming {}", e.kind));
            a.evidence.push(eref(
                "structural",
                &format!("{s}→{working}"),
                &e.kind,
            ));
        }
        for row in store.co_changes_for_file(&working, repo_path, 1)? {
            let a = map.entry(row.file_path.clone()).or_default();
            a.cochange += row.change_count as f32;
            a.reasons.push(format!("co-change×{}", row.change_count));
            a.evidence.push(eref(
                "cochange",
                &row.file_path,
                &format!("count={}", row.change_count),
            ));
        }
    } else {
        let prefix = format!("{working}/");
        for e in store.structural_edges_from_prefix(&prefix, repo_path)? {
            if e.target_file.starts_with(&prefix) || e.target_file.starts_with("UNRESOLVED:") {
                continue;
            }
            let t = e.target_file.clone();
            let a = map.entry(t.clone()).or_default();
            a.structural += 1.0;
            a.reasons.push(format!("out-of-prefix {}", e.kind));
            a.evidence
                .push(eref("structural", &format!("{}→{}", e.source_file, t), &e.kind));
        }
        for e in store.structural_edges_to_prefix(&prefix, repo_path)? {
            if e.source_file.starts_with(&prefix) {
                continue;
            }
            let s = e.source_file.clone();
            let a = map.entry(s.clone()).or_default();
            a.structural += 1.5;
            a.reasons.push(format!("into-prefix {}", e.kind));
            a.evidence
                .push(eref("structural", &format!("{s}→{}", e.target_file), &e.kind));
        }
    }

    // Mark tests
    for (path, a) in map.iter_mut() {
        let p = path.to_lowercase();
        if p.contains("/test") || p.contains("__tests__") || p.ends_with(".test.ts") || p.ends_with(".spec.ts") {
            a.is_test = true;
            a.reasons.push("test-path heuristic".into());
        }
    }

    let touch_subject = if is_file {
        store.touch_count(&working, repo_path).unwrap_or(0) as f32
    } else {
        store
            .commits_under_prefix(&format!("{working}/"), repo_path)
            .map(|c| c.len() as f32)
            .unwrap_or(0.0)
    };

    let mut neighbors: Vec<ImpactNeighbor> = map
        .into_iter()
        .map(|(path, acc)| {
            let structural_connectivity = (acc.structural / (acc.structural + 3.0)).min(1.0);
            let historical_cochange = (acc.cochange / (acc.cochange + 3.0)).min(1.0);
            let subject_relevance = if path.starts_with(&working) || working.starts_with(&path) {
                1.0
            } else if path.split('/').take(3).collect::<Vec<_>>()
                == working.split('/').take(3).collect::<Vec<_>>()
            {
                0.7
            } else {
                0.4
            };
            let corroboration = if acc.is_test {
                0.6
            } else if acc.structural > 0.0 && acc.cochange > 0.0 {
                0.8
            } else if acc.structural > 0.0 {
                0.5
            } else {
                0.3
            };
            let temporal_recency = 0.0; // v1: no per-neighbor last-touch without extra queries
            let rank_score = 0.35 * structural_connectivity
                + 0.30 * historical_cochange
                + 0.20 * subject_relevance
                + 0.15 * corroboration;

            let layer = if acc.structural > 0.0 && acc.cochange > 0.0 {
                EpistemicLayer::Derived
            } else if acc.structural > 0.0 {
                EpistemicLayer::Observed
            } else if acc.cochange > 0.0 {
                EpistemicLayer::Observed
            } else {
                EpistemicLayer::Inferred
            };

            ImpactNeighbor {
                path,
                reasons: {
                    let mut r = acc.reasons;
                    r.sort();
                    r.dedup();
                    r
                },
                layer,
                dimensions: EvidenceDimensions {
                    subject_relevance,
                    temporal_recency,
                    structural_connectivity,
                    historical_cochange,
                    corroboration,
                    provenance_note: if acc.structural > 0.0 {
                        "structural_edges (+ co-change if present)".into()
                    } else {
                        "commit co-change only".into()
                    },
                },
                rank_score,
                evidence: acc.evidence.into_iter().take(6).collect(),
            }
        })
        .collect();

    neighbors.sort_by(|a, b| {
        b.rank_score
            .partial_cmp(&a.rank_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    neighbors.truncate(40);

    let mut claims = Vec::new();
    claims.push(claim(
        "impact-001",
        &working,
        &format!(
            "Impact neighborhood lists {} related path(s) from structural edges and/or co-change (subject touch_count≈{}).",
            neighbors.len(),
            touch_subject as i64
        ),
        EpistemicLayer::Derived,
        "structural reverse/forward + co_changes_for_file; rank_score weighted sum",
        vec![eref("path", &working, "impact subject")],
        vec![
            "Rank score is a retrieval aid, not risk or ownership.".into(),
            "Temporal recency dimension is reserved (0 in v1) unless per-neighbor timestamps loaded.".into(),
        ],
    ));

    if let Some(n) = neighbors.first() {
        claims.push(claim(
            "impact-002",
            &n.path,
            &format!(
                "Highest-ranked related path: `{}` (score={:.2}; reasons: {}).",
                n.path,
                n.rank_score,
                n.reasons.join(", ")
            ),
            n.layer.clone(),
            "argmax rank_score among ImpactNeighbor",
            n.evidence.clone(),
            vec!["Not a recommendation to change this file.".into()],
        ));
    }

    if neighbors.is_empty() {
        claims.push(claim(
            "impact-000",
            &working,
            "No structural or co-change neighbors found for this subject in the current DB.",
            EpistemicLayer::Unknown,
            "empty neighbor map",
            vec![],
            vec![
                "Subject may lack edges, history, or ingest coverage.".into(),
            ],
        ));
    }

    let dimensions_methodology = vec![
        "subject_relevance: path prefix/segment overlap with subject".into(),
        "structural_connectivity: f(edge_count)/(edge_count+3) from structural_edges".into(),
        "historical_cochange: f(cochange_count)/(count+3) from co_changes_for_file".into(),
        "corroboration: higher when both structure and co-change present, or test path".into(),
        "temporal_recency: 0.0 in v1 (dimension reserved for chronology-aware ranking)".into(),
        "rank_score = 0.35*structural + 0.30*cochange + 0.20*relevance + 0.15*corroboration".into(),
        "Epistemic layers: Observed (raw edges/cochange), Derived (combined rank), Unknown (empty).".into(),
    ];

    let limitations = vec![
        "Impact is investigation guidance, not change-safety certification.".into(),
        "No runtime or DI graph.".into(),
        "Config-time wiring may be invisible.".into(),
        "Does not assert root cause or architectural quality.".into(),
    ];

    Ok(ImpactReport {
        schema_version: IMPACT_SCHEMA,
        subject: working,
        neighbors,
        claims,
        dimensions_methodology,
        limitations,
    })
}
