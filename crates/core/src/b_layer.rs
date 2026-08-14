//! B5–B10 aggregation layer.
//!
//! Pure projections over existing Atlas evidence.  No new extractors, no
//! schema changes, no LLM.  Every derived classification exposes its rule.

use anyhow::Result;
use atlas_ir::{
    AnomaliesReport, AnomalyEntry, AnomalyKind, CohortsReport, ConfigArtifactReport,
    ConfigArtifactSummary, ConfigHistoryCommit, ConfigInventoryReport, DependencyLinkageReport,
    DependencySection, DirectoryCohort, DirectoryPairCochange, EvidenceClass, HistoricalRedirect,
    ModuleEntry, ModulesReport, PackageDependencyEntry, PackageSourceObservation, TestLinkageKind,
    TestModuleLink, TestModuleReport,
};
use atlas_storage::Store;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

// ─── shared helpers ─────────────────────────────────────────────────────────

fn normalize_subject(subject: &str) -> String {
    subject
        .trim()
        .trim_start_matches('/')
        .trim_end_matches('/')
        .to_string()
}

/// Immediate child directory names under `prefix` from the files table.
fn immediate_child_dirs(all_paths: &[String], prefix: &str) -> Vec<String> {
    let prefix_with_slash = if prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", prefix)
    };
    let mut set: BTreeSet<String> = BTreeSet::new();
    for p in all_paths {
        if !p.starts_with(&prefix_with_slash) {
            continue;
        }
        let rest = &p[prefix_with_slash.len()..];
        if let Some(slash) = rest.find('/') {
            let seg = &rest[..slash];
            if !seg.is_empty() {
                set.insert(seg.to_string());
            }
        }
    }
    set.into_iter().collect()
}

/// Path-only test classification (mirrors `classify_artifact_role` Test arm).
pub fn path_looks_like_test(path: &str) -> bool {
    let p = path.to_lowercase();
    p.starts_with("tests/")
        || p.starts_with("test/")
        || p.starts_with("spec/")
        || p.starts_with("__tests__/")
        || p.contains("/tests/")
        || p.contains("/test/")
        || p.contains("/spec/")
        || p.contains("/__tests__/")
        || p.ends_with(".spec.ts")
        || p.ends_with(".spec.js")
        || p.ends_with(".test.ts")
        || p.ends_with(".test.js")
}

/// Packages that are declared but almost never appear as static import targets.
/// Surfacing them as anomalies floods the report without signaling product risk.
fn is_toolchain_noise_package(name: &str, section: &atlas_ir::DependencySection) -> bool {
    use atlas_ir::DependencySection;
    if name.starts_with("@types/") {
        return true;
    }
    // Codegen / compiler / test runners — typically CLI-only.
    const TOOLS: &[&str] = &[
        "typescript",
        "tsx",
        "ts-node",
        "ts-node-dev",
        "eslint",
        "prettier",
        "vitest",
        "jest",
        "mocha",
        "webpack",
        "vite",
        "esbuild",
        "rollup",
        "parcel",
        "nodemon",
        "husky",
        "lint-staged",
        "rimraf",
        "npm-run-all",
        "concurrently",
        "cross-env",
        "dotenv-cli",
        "@swc/core",
        "@swc/cli",
        "babel-loader",
        "typescript-eslint",
    ];
    if TOOLS.iter().any(|t| name == *t || name.starts_with(&format!("{t}/"))) {
        return true;
    }
    if name.starts_with("@graphql-codegen/")
        || name.starts_with("@typescript-eslint/")
        || name.starts_with("@vitest/")
        || name.starts_with("@jest/")
        || name.starts_with("@babel/")
        || name.starts_with("eslint-")
        || name.starts_with("@eslint/")
    {
        return true;
    }
    // Entirely-dev tooling sections: keep only packages that look like runtime
    // libs mistakenly in devDependencies (heuristic: not covered above).
    if matches!(section, DependencySection::DevDependencies) {
        // Already filtered common tools; remaining @types* handled.
        // Filter bare type helpers still common in devDeps.
        if name.ends_with("-types") || name.contains("typedoc") {
            return true;
        }
    }
    false
}

/// Extract the npm package root from an `UNRESOLVED:external:…` target.
///
/// Rules (deterministic):
/// - strip `UNRESOLVED:external:`
/// - scoped: `@scope/name` → first two slash-separated segments
/// - unscoped: first slash-separated segment
pub fn external_package_name(target: &str) -> Option<String> {
    let rest = target.strip_prefix("UNRESOLVED:external:")?;
    if rest.is_empty() {
        return None;
    }
    if let Some(scoped) = rest.strip_prefix('@') {
        let mut parts = scoped.split('/');
        let scope = parts.next()?;
        let name = parts.next()?;
        if scope.is_empty() || name.is_empty() {
            return None;
        }
        Some(format!("@{}/{}", scope, name))
    } else {
        Some(rest.split('/').next()?.to_string())
    }
}

// ─── B5 — atlas modules ─────────────────────────────────────────────────────

const MODULES_SCHEMA_VERSION: u32 = 1;

/// Inventory of immediate child directories under `subject` (default
/// `src/modules`) with deterministic evidence counts.
pub fn compute_modules(
    subject: &str,
    repo_path: &str,
    store: &Store,
) -> Result<ModulesReport> {
    let subject = if subject.trim().is_empty() {
        "src/modules".to_string()
    } else {
        normalize_subject(subject)
    };

    let all_paths = store.all_file_paths(repo_path)?;
    let names = immediate_child_dirs(&all_paths, &subject);
    let all_edges = store.structural_edges_from_prefix("", repo_path)?;

    let mut modules: Vec<ModuleEntry> = Vec::new();
    for name in &names {
        let path = format!("{}/{}", subject, name);
        let prefix = format!("{}/", path);

        let file_paths: Vec<&String> = all_paths
            .iter()
            .filter(|p| p.starts_with(&prefix) || *p == &path)
            .collect();
        let file_count = file_paths.len();

        let mut subdirs: BTreeSet<String> = BTreeSet::new();
        for p in &file_paths {
            if let Some(rest) = p.strip_prefix(&prefix) {
                if let Some(slash) = rest.find('/') {
                    let seg = &rest[..slash];
                    if !seg.is_empty() {
                        subdirs.insert(seg.to_string());
                    }
                }
            }
        }

        let commits = store.commits_under_prefix(&prefix, repo_path)?;
        let observed_commit_count = commits.len();

        let mut outgoing = 0usize;
        let mut incoming = 0usize;
        for e in &all_edges {
            if e.source_file.starts_with(&prefix) {
                outgoing += 1;
            }
            if e.target_file.starts_with(&prefix) {
                incoming += 1;
            }
        }

        let in_module_test_file_count = file_paths
            .iter()
            .filter(|p| path_looks_like_test(p))
            .count();

        let tests_dir_prefix = format!("tests/{}/", name);
        let has_tests_dir = all_paths.iter().any(|p| p.starts_with(&tests_dir_prefix));
        let has_associated_tests = in_module_test_file_count > 0 || has_tests_dir;
        let test_association_rule = format!(
            "DERIVED: true if any file under `{}/` matches the path-test heuristic, \
             OR any file path starts with `tests/{}/`",
            path, name
        );

        modules.push(ModuleEntry {
            name: name.clone(),
            path,
            file_count,
            subdirectories: subdirs.into_iter().collect(),
            observed_commit_count,
            outgoing_edge_count: outgoing,
            incoming_edge_count: incoming,
            in_module_test_file_count,
            has_associated_tests,
            test_association_rule,
        });
    }

    // Drop ghost modules that only exist in historical `files` rows when the
    // working tree is available (repo root is a real directory). In-memory /
    // missing roots keep evidence-only modules so fixtures stay valid.
    let root = std::path::Path::new(repo_path);
    let on_disk_filter = root.is_dir();
    let modules: Vec<ModuleEntry> = if on_disk_filter {
        modules
            .into_iter()
            .filter(|m| root.join(&m.path).is_dir())
            .collect()
    } else {
        modules
    };

    // names already alphabetical via BTreeSet; keep modules in that order.
    let total_modules = modules.len();

    let discovery_rule = if on_disk_filter {
        format!(
            "DETERMINISTIC: immediate child directories of `{}` that appear in the \
             `files` table AND exist as directories on disk (ghost/historical paths dropped)",
            subject
        )
    } else {
        format!(
            "DETERMINISTIC: immediate child directories of `{}` that appear in the \
             `files` table (a path segment after the subject prefix with a further `/`)",
            subject
        )
    };

    Ok(ModulesReport {
        schema_version: MODULES_SCHEMA_VERSION,
        subject: subject.clone(),
        modules,
        discovery_rule,
        total_modules,
    })
}

// ─── B6 — test ↔ module linkage ─────────────────────────────────────────────

const TEST_MODULE_SCHEMA_VERSION: u32 = 1;

/// Associate test files with modules using explicit path rules only.
pub fn compute_test_module_links(
    modules_subject: &str,
    path_filter: Option<&str>,
    repo_path: &str,
    store: &Store,
) -> Result<TestModuleReport> {
    let modules_subject = if modules_subject.trim().is_empty() {
        "src/modules".to_string()
    } else {
        normalize_subject(modules_subject)
    };

    let all_paths = store.all_file_paths(repo_path)?;
    let module_names = immediate_child_dirs(&all_paths, &modules_subject);
    let module_set: HashSet<&str> = module_names.iter().map(|s| s.as_str()).collect();

    let filter = path_filter.map(|s| normalize_subject(s));

    let test_files: Vec<String> = all_paths
        .iter()
        .filter(|p| path_looks_like_test(p))
        .filter(|p| match &filter {
            Some(f) => {
                let fp = format!("{}/", f);
                *p == f || p.starts_with(&fp)
            }
            None => true,
        })
        .cloned()
        .collect();

    let rules = vec![
        "DIRECT (DirectPathPrefix): test path starts with `{modules_subject}/{module}/`".into(),
        "DERIVED (ConventionalTestsDir): test path starts with `tests/{module}/` AND \
         `{modules_subject}/{module}` exists as a discovered module"
            .into(),
    ];

    let mut links: Vec<TestModuleLink> = Vec::new();
    let mut linked_tests: HashSet<String> = HashSet::new();

    for test_path in &test_files {
        // Rule 1: direct under module.
        let mod_prefix = format!("{}/", modules_subject);
        if let Some(rest) = test_path.strip_prefix(&mod_prefix) {
            if let Some(slash) = rest.find('/') {
                let name = &rest[..slash];
                if module_set.contains(name) {
                    links.push(TestModuleLink {
                        test_path: test_path.clone(),
                        module_name: name.to_string(),
                        module_path: format!("{}/{}", modules_subject, name),
                        linkage_kind: TestLinkageKind::DirectPathPrefix,
                        evidence_class: EvidenceClass::Deterministic,
                        rule: format!(
                            "test path is under module directory `{}/{}/`",
                            modules_subject, name
                        ),
                    });
                    linked_tests.insert(test_path.clone());
                    continue;
                }
            }
        }

        // Rule 2: tests/<module>/…
        if let Some(rest) = test_path.strip_prefix("tests/") {
            if let Some(slash) = rest.find('/') {
                let name = &rest[..slash];
                if module_set.contains(name) {
                    links.push(TestModuleLink {
                        test_path: test_path.clone(),
                        module_name: name.to_string(),
                        module_path: format!("{}/{}", modules_subject, name),
                        linkage_kind: TestLinkageKind::ConventionalTestsDir,
                        evidence_class: EvidenceClass::Derived,
                        rule: format!(
                            "test path starts with `tests/{}/` and module `{}` exists under `{}`",
                            name, name, modules_subject
                        ),
                    });
                    linked_tests.insert(test_path.clone());
                }
            }
        }
    }

    links.sort_by(|a, b| {
        a.module_name
            .cmp(&b.module_name)
            .then(a.test_path.cmp(&b.test_path))
            .then(format!("{:?}", a.linkage_kind).cmp(&format!("{:?}", b.linkage_kind)))
    });

    let mut unlinked_tests: Vec<String> = test_files
        .into_iter()
        .filter(|t| !linked_tests.contains(t))
        .collect();
    unlinked_tests.sort();

    let total_links = links.len();
    let total_test_files = linked_tests.len() + unlinked_tests.len();

    Ok(TestModuleReport {
        schema_version: TEST_MODULE_SCHEMA_VERSION,
        modules_subject,
        path_filter: filter,
        modules: module_names,
        links,
        unlinked_tests,
        total_test_files,
        total_links,
        linkage_rules: rules,
    })
}

// ─── B7 — NPM dependency → source linkage ───────────────────────────────────

const DEP_LINKAGE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug)]
struct DeclaredPkg {
    version: String,
    section: DependencySection,
}

fn parse_package_json_deps(content: &str) -> Result<BTreeMap<String, DeclaredPkg>> {
    let v: serde_json::Value = serde_json::from_str(content)?;
    let mut out: BTreeMap<String, DeclaredPkg> = BTreeMap::new();
    let sections = [
        ("dependencies", DependencySection::Dependencies),
        ("devDependencies", DependencySection::DevDependencies),
        ("optionalDependencies", DependencySection::OptionalDependencies),
        ("peerDependencies", DependencySection::PeerDependencies),
    ];
    for (key, section) in sections {
        if let Some(obj) = v.get(key).and_then(|x| x.as_object()) {
            for (name, ver) in obj {
                let version = ver.as_str().unwrap_or("").to_string();
                // First section wins if duplicated (unusual).
                out.entry(name.clone()).or_insert(DeclaredPkg {
                    version,
                    section: section.clone(),
                });
            }
        }
    }
    Ok(out)
}

/// Link package.json declarations to UNRESOLVED:external structural edges.
pub fn compute_dependency_linkage(
    repo_path: &str,
    store: &Store,
) -> Result<DependencyLinkageReport> {
    // Prefer ingested configuration artifact; fall back to on-disk package.json.
    let (content, declaration_provenance) =
        if let Some(art) = store.configuration_artifact("package.json", repo_path)? {
            (art.raw_content, "configuration_artifacts (package.json)".to_string())
        } else {
            let disk = Path::new(repo_path).join("package.json");
            match std::fs::read_to_string(&disk) {
                Ok(c) => (
                    c,
                    "working-tree package.json (not present in configuration_artifacts)".to_string(),
                ),
                Err(_) => (
                    String::new(),
                    "no package.json in configuration_artifacts or working tree".to_string(),
                ),
            }
        };

    let declared = if content.is_empty() {
        BTreeMap::new()
    } else {
        parse_package_json_deps(&content).unwrap_or_default()
    };

    let all_edges = store.structural_edges_from_prefix("", repo_path)?;
    // package -> observations
    let mut observed: BTreeMap<String, Vec<PackageSourceObservation>> = BTreeMap::new();
    for e in &all_edges {
        if let Some(pkg) = external_package_name(&e.target_file) {
            observed.entry(pkg).or_default().push(PackageSourceObservation {
                source_file: e.source_file.clone(),
                target_spec: e.target_file.clone(),
                edge_kind: e.kind.clone(),
                evidence_snippet: e.evidence_snippet.clone(),
            });
        }
    }
    for obs in observed.values_mut() {
        obs.sort_by(|a, b| {
            a.source_file
                .cmp(&b.source_file)
                .then(a.target_spec.cmp(&b.target_spec))
        });
    }

    let mut package_names: BTreeSet<String> = BTreeSet::new();
    package_names.extend(declared.keys().cloned());
    package_names.extend(observed.keys().cloned());

    let mut packages: Vec<PackageDependencyEntry> = Vec::new();
    for name in package_names {
        let decl = declared.get(&name);
        let obs = observed.get(&name).cloned().unwrap_or_default();
        let is_declared = decl.is_some();
        let is_observed = !obs.is_empty();
        let distinct_source_files = obs
            .iter()
            .map(|o| o.source_file.as_str())
            .collect::<HashSet<_>>()
            .len();
        let observation_count = obs.len();
        packages.push(PackageDependencyEntry {
            package_name: name,
            section: decl
                .map(|d| d.section.clone())
                .unwrap_or(DependencySection::Undeclared),
            declared_version: decl.map(|d| d.version.clone()).unwrap_or_default(),
            is_declared,
            is_observed,
            observations: obs,
            observation_count,
            distinct_source_files,
        });
    }
    // Sort: observed+declared first by observation_count desc, then name.
    packages.sort_by(|a, b| {
        b.observation_count
            .cmp(&a.observation_count)
            .then(a.package_name.cmp(&b.package_name))
    });

    let total_declared = packages.iter().filter(|p| p.is_declared).count();
    let total_observed = packages.iter().filter(|p| p.is_observed).count();
    let declared_and_observed = packages
        .iter()
        .filter(|p| p.is_declared && p.is_observed)
        .count();
    let declared_unobserved = packages
        .iter()
        .filter(|p| p.is_declared && !p.is_observed)
        .count();
    let observed_undeclared = packages
        .iter()
        .filter(|p| !p.is_declared && p.is_observed)
        .count();

    Ok(DependencyLinkageReport {
        schema_version: DEP_LINKAGE_SCHEMA_VERSION,
        declaration_source: "package.json".to_string(),
        declaration_provenance,
        packages,
        total_declared,
        total_observed,
        declared_and_observed,
        declared_unobserved,
        observed_undeclared,
        methodology: vec![
            "DECLARED: package name appears under dependencies / devDependencies / \
             optionalDependencies / peerDependencies in package.json"
                .into(),
            "OBSERVED: structural_edges.target_file matches UNRESOLVED:external:<package>… \
             (static import evidence only — not runtime usage)"
                .into(),
            "Package root extraction: unscoped = first path segment; scoped = @scope/name".into(),
            "No claim of runtime execution or bundle inclusion is made".into(),
        ],
    })
}

// ─── B8 — cross-directory co-change cohorts ─────────────────────────────────

const COHORTS_SCHEMA_VERSION: u32 = 1;
const DEFAULT_COCHANGE_THRESHOLD: usize = 2;

/// Identify directory pairs under `subject` that repeatedly co-change.
pub fn compute_directory_cohorts(
    subject: &str,
    cochange_threshold: Option<usize>,
    repo_path: &str,
    store: &Store,
) -> Result<CohortsReport> {
    let subject = if subject.trim().is_empty() {
        "src/modules".to_string()
    } else {
        normalize_subject(subject)
    };
    let threshold = cochange_threshold.unwrap_or(DEFAULT_COCHANGE_THRESHOLD);

    let all_paths = store.all_file_paths(repo_path)?;
    let directories = immediate_child_dirs(&all_paths, &subject);

    // pair (a,b) with a < b -> count
    let mut pair_counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    let commits = store.all_commits_with_files(repo_path)?;
    let subject_prefix = format!("{}/", subject);

    for c in &commits {
        let mut touched: BTreeSet<String> = BTreeSet::new();
        for f in &c.files {
            if let Some(rest) = f.strip_prefix(&subject_prefix) {
                if let Some(slash) = rest.find('/') {
                    let seg = &rest[..slash];
                    if !seg.is_empty() {
                        touched.insert(seg.to_string());
                    }
                }
            }
        }
        let dirs: Vec<String> = touched.into_iter().collect();
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                let a = &dirs[i];
                let b = &dirs[j];
                let key = if a < b {
                    (a.clone(), b.clone())
                } else {
                    (b.clone(), a.clone())
                };
                *pair_counts.entry(key).or_insert(0) += 1;
            }
        }
    }

    let mut pairs: Vec<DirectoryPairCochange> = pair_counts
        .into_iter()
        .map(|((a, b), n)| DirectoryPairCochange {
            directory_a: a,
            directory_b: b,
            cochange_commit_count: n,
            evidence_class: EvidenceClass::Deterministic,
        })
        .collect();
    pairs.sort_by(|x, y| {
        y.cochange_commit_count
            .cmp(&x.cochange_commit_count)
            .then(x.directory_a.cmp(&y.directory_a))
            .then(x.directory_b.cmp(&y.directory_b))
    });

    // Build undirected graph of edges at/above threshold → connected components.
    let mut adj: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut edge_weight: HashMap<(String, String), usize> = HashMap::new();
    for p in &pairs {
        if p.cochange_commit_count >= threshold {
            adj.entry(p.directory_a.clone())
                .or_default()
                .insert(p.directory_b.clone());
            adj.entry(p.directory_b.clone())
                .or_default()
                .insert(p.directory_a.clone());
            edge_weight.insert(
                (p.directory_a.clone(), p.directory_b.clone()),
                p.cochange_commit_count,
            );
        }
    }

    let mut remaining: BTreeSet<String> = directories.iter().cloned().collect();
    let mut cohorts: Vec<DirectoryCohort> = Vec::new();
    let mut singletons: Vec<String> = Vec::new();

    while let Some(start) = remaining.iter().next().cloned() {
        remaining.remove(&start);
        // BFS
        let mut component: BTreeSet<String> = BTreeSet::new();
        let mut stack = vec![start.clone()];
        component.insert(start);
        while let Some(n) = stack.pop() {
            if let Some(neis) = adj.get(&n) {
                for nei in neis {
                    if component.insert(nei.clone()) {
                        remaining.remove(nei);
                        stack.push(nei.clone());
                    }
                }
            }
        }
        if component.len() < 2 {
            if let Some(only) = component.into_iter().next() {
                // Only singleton if it never appears in a threshold edge.
                if !adj.contains_key(&only) {
                    singletons.push(only);
                }
            }
            continue;
        }
        let members: Vec<String> = component.into_iter().collect();
        let mut min_edge = usize::MAX;
        let mut total_edge = 0usize;
        for i in 0..members.len() {
            for j in (i + 1)..members.len() {
                let a = &members[i];
                let b = &members[j];
                let key = if a < b {
                    (a.clone(), b.clone())
                } else {
                    (b.clone(), a.clone())
                };
                if let Some(&w) = edge_weight.get(&key) {
                    min_edge = min_edge.min(w);
                    total_edge += w;
                }
            }
        }
        if min_edge == usize::MAX {
            min_edge = 0;
        }
        cohorts.push(DirectoryCohort {
            members,
            min_edge_cochange: min_edge,
            total_edge_cochange: total_edge,
            evidence_class: EvidenceClass::Derived,
        });
    }
    cohorts.sort_by(|a, b| {
        b.total_edge_cochange
            .cmp(&a.total_edge_cochange)
            .then(a.members.cmp(&b.members))
    });
    singletons.sort();

    Ok(CohortsReport {
        schema_version: COHORTS_SCHEMA_VERSION,
        subject: subject.clone(),
        directories,
        cochange_threshold: threshold,
        pairs,
        cohorts,
        singletons,
        methodology: vec![
            format!(
                "DETERMINISTIC pairs: for each commit, take the set of immediate child \
                 directories of `{}` touched by that commit; every unordered pair in the \
                 set increments cochange_commit_count by 1",
                subject
            ),
            format!(
                "DERIVED cohorts: connected components of the undirected graph of pairs \
                 with cochange_commit_count ≥ {} (threshold explicit; not a domain claim)",
                threshold
            ),
            "Singletons (directories with no pair above threshold) are listed, not discarded"
                .into(),
            "Language: these are co-change cohorts, NOT business-domain clusters".into(),
        ],
    })
}

// ─── B9 — anomalies ─────────────────────────────────────────────────────────

const ANOMALIES_SCHEMA_VERSION: u32 = 1;

/// Deviation-from-observed-pattern view, reusing B1/B5/B6/B7 evidence.
pub fn compute_anomalies(
    subject: &str,
    repo_path: &str,
    store: &Store,
) -> Result<AnomaliesReport> {
    let subject = if subject.trim().is_empty() {
        "src/modules".to_string()
    } else {
        normalize_subject(subject)
    };

    let mut anomalies: Vec<AnomalyEntry> = Vec::new();

    // From B1 peer structure.
    let peer = super::detect_peer_structure(&subject, repo_path, store)?;
    for d in &peer.deviations {
        anomalies.push(AnomalyEntry {
            kind: AnomalyKind::PeerStructureDeviation,
            subject: format!("{}/{}", peer.peer_parent, d.peer),
            observation: format!("missing structural element `{}`", d.element),
            expected: format!(
                "element present in {}/{} peers (strict majority threshold {}/{})",
                d.peer_prevalence_num,
                d.peer_prevalence_den,
                peer.deviation_threshold_num,
                peer.deviation_threshold_den
            ),
            evidence_class: EvidenceClass::Derived,
            evidence: vec![
                format!("peer={}", d.peer),
                format!("element={}", d.element),
                format!(
                    "peer_prevalence={}/{}",
                    d.peer_prevalence_num, d.peer_prevalence_den
                ),
            ],
            threshold_note: format!(
                "deviation when present_count * {} > {} * peer_count",
                peer.deviation_threshold_den, peer.deviation_threshold_num
            ),
        });
    }

    // From B5/B6: modules without associated tests.
    let mods = compute_modules(&subject, repo_path, store)?;
    for m in &mods.modules {
        if !m.has_associated_tests {
            anomalies.push(AnomalyEntry {
                kind: AnomalyKind::MissingAssociatedTests,
                subject: m.path.clone(),
                observation: "no associated test files under documented linkage rules".into(),
                expected: format!(
                    "in-module test path OR tests/{}/ tree (see test_association_rule)",
                    m.name
                ),
                evidence_class: EvidenceClass::Derived,
                evidence: vec![
                    format!("file_count={}", m.file_count),
                    format!("in_module_test_file_count={}", m.in_module_test_file_count),
                    m.test_association_rule.clone(),
                ],
                threshold_note: "binary rule: has_associated_tests == false".into(),
            });
        }
    }

    // From B7: declared but unobserved packages.
    // Skip toolchain / type-only noise — those are expected never to appear as
    // runtime UNRESOLVED:external imports.
    let deps = compute_dependency_linkage(repo_path, store)?;
    for p in deps
        .packages
        .iter()
        .filter(|p| p.is_declared && !p.is_observed)
        .filter(|p| !is_toolchain_noise_package(&p.package_name, &p.section))
    {
        anomalies.push(AnomalyEntry {
            kind: AnomalyKind::DeclaredDependencyUnobserved,
            subject: p.package_name.clone(),
            observation: "package declared in package.json with zero structural import edges"
                .into(),
            expected: "≥1 UNRESOLVED:external structural edge targeting this package root".into(),
            evidence_class: EvidenceClass::Derived,
            evidence: vec![
                format!("section={:?}", p.section),
                format!("declared_version={}", p.declared_version),
                format!("declaration_source={}", deps.declaration_provenance),
            ],
            threshold_note: "observation_count == 0; toolchain/@types packages excluded".into(),
        });
    }

    anomalies.sort_by(|a, b| {
        format!("{:?}", a.kind)
            .cmp(&format!("{:?}", b.kind))
            .then(a.subject.cmp(&b.subject))
            .then(a.observation.cmp(&b.observation))
    });

    let total_anomalies = anomalies.len();
    Ok(AnomaliesReport {
        schema_version: ANOMALIES_SCHEMA_VERSION,
        subject,
        anomalies,
        total_anomalies,
        methodology: vec![
            "Anomalies are deviations from patterns Atlas observed elsewhere — not quality judgments"
                .into(),
            "PeerStructureDeviation: reuses B1 detect_peer_structure deviations".into(),
            "MissingAssociatedTests: reuses B5 has_associated_tests (B6 rules)".into(),
            "DeclaredDependencyUnobserved: reuses B7 declared∩¬observed (excludes @types/*, typescript, eslint, codegen, test runners)"
                .into(),
            "Language uses 'deviates from observed peer pattern'; quality adjectives are out of scope"
                .into(),
            "Modules inventory drops paths not present on disk when repo root exists"
                .into(),
        ],
    })
}

// ─── B10 — configuration → history / provenance ─────────────────────────────

const CONFIG_SCHEMA_VERSION: u32 = 1;

/// Inventory of ingested configuration artifacts with touch counts.
pub fn compute_config_inventory(
    repo_path: &str,
    store: &Store,
) -> Result<ConfigInventoryReport> {
    let arts = store.list_configuration_artifacts(repo_path)?;
    let mut artifacts: Vec<ConfigArtifactSummary> = Vec::new();
    for (file_path, artifact_kind, sha256) in arts {
        let commits = store.commits_for_file(&file_path, repo_path)?;
        let ingested_at = store
            .configuration_artifact(&file_path, repo_path)?
            .map(|a| a.ingested_at)
            .unwrap_or(0);
        artifacts.push(ConfigArtifactSummary {
            file_path,
            artifact_kind,
            sha256,
            ingested_at,
            touching_commit_count: commits.len(),
        });
    }
    artifacts.sort_by(|a, b| a.file_path.cmp(&b.file_path));
    let total_artifacts = artifacts.len();
    Ok(ConfigInventoryReport {
        schema_version: CONFIG_SCHEMA_VERSION,
        artifacts,
        total_artifacts,
        provenance: vec![
            "Source table: configuration_artifacts (repo-scoped)".into(),
            "touching_commit_count from commits ⨝ commit_files on exact file_path".into(),
        ],
    })
}

/// Provenance report for one configuration path.
pub fn compute_config_provenance(
    file_path: &str,
    repo_path: &str,
    store: &Store,
) -> Result<ConfigArtifactReport> {
    let raw = file_path.trim().trim_start_matches('/');
    let mut working = raw.to_string();
    let mut redirect_note = None;

    if let Some(current) = store.current_path_if_historical(raw, repo_path)? {
        let identity_id = store
            .resolve_path_to_identity(&current, repo_path)?
            .unwrap_or(-1);
        redirect_note = Some(HistoricalRedirect {
            original_subject: raw.to_string(),
            current_path: current.clone(),
            identity_id,
        });
        working = current;
    }

    let artifact = store.configuration_artifact(&working, repo_path)?;
    let (artifact_present, artifact_kind, sha256, ingested_at, content_byte_len) =
        match &artifact {
            Some(a) => (
                true,
                Some(a.artifact_kind.clone()),
                Some(a.sha256.clone()),
                Some(a.ingested_at),
                Some(a.raw_content.len()),
            ),
            None => (false, None, None, None, None),
        };

    // Prefer identity-scoped commits when available.
    let identity_id = store.resolve_path_to_identity(&working, repo_path)?;
    let (touching_rows, identity_commit_count) = if let Some(id) = identity_id {
        let rows = store.commits_for_identity(id, repo_path)?;
        let n = rows.len();
        (rows, Some(n))
    } else {
        (store.commits_for_file(&working, repo_path)?, None)
    };

    // commits_for_file is newest-first; sort asc for first/last convenience in report
    // but keep display as returned (newest first) matching other atlas commands.
    let mut timestamps: Vec<i64> = touching_rows.iter().map(|c| c.timestamp).collect();
    timestamps.sort();
    let first_touch = timestamps.first().copied();
    let last_touch = timestamps.last().copied();

    let touching_commits: Vec<ConfigHistoryCommit> = touching_rows
        .into_iter()
        .map(|c| ConfigHistoryCommit {
            hash: c.hash,
            short_hash: c.short_hash,
            message: c.message,
            author_name: c.author_name,
            timestamp: c.timestamp,
        })
        .collect();
    let touching_commit_count = touching_commits.len();

    let mut limitations = vec![
        "Atlas stores the CURRENT ingested configuration content only — historical \
         content snapshots per commit are NOT available"
            .into(),
    ];
    if !artifact_present {
        limitations.push(format!(
            "No configuration_artifacts row for `{}` — path may not be a recognised \
             root config, or config ingest has not been run",
            working
        ));
    }

    Ok(ConfigArtifactReport {
        schema_version: CONFIG_SCHEMA_VERSION,
        file_path: working,
        artifact_present,
        artifact_kind,
        sha256,
        ingested_at,
        content_byte_len,
        touching_commits,
        touching_commit_count,
        first_touch,
        last_touch,
        redirect_note,
        identity_id,
        identity_commit_count,
        limitations,
        provenance: vec![
            "configuration_artifacts (repo-scoped) for content/sha when present".into(),
            "commits ⨝ commit_files (repo-scoped) for touch history".into(),
            "file_identity_commits used when a FileIdentity exists for the path".into(),
        ],
    })
}
