//! Deterministic code-intelligence queries over structural_edges.
//!
//! These replace agent host "domain drills" with general operations:
//! find callers, find implementations, capability surfaces, definition-ranked search.
//!
//! Epistemic contract:
//! - Edges are OBSERVED at last structural ingest (working-tree snapshot).
//! - Path/name heuristics for "implementation" and "capability" are DERIVED.
//! - Never claim runtime dispatch completeness (dynamic import / DI not covered).

use anyhow::Result;
use atlas_storage::{Store, StructuralEdgeRow};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

// ── Reports ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct CallSite {
    pub caller_file: String,
    pub caller_symbol: Option<String>,
    pub callee_file: String,
    pub callee_symbol: Option<String>,
    pub kind: String,
    pub evidence_line: Option<u32>,
    pub evidence_snippet: String,
    pub is_test: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CallersReport {
    pub subject: String,
    pub resolved_as: String,
    pub production_callers: Vec<CallSite>,
    pub test_callers: Vec<CallSite>,
    pub callees: Vec<CallSite>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImplementationHit {
    pub file: String,
    pub reason: String,
    pub imports_interface: bool,
    pub is_test: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImplementationsReport {
    pub subject: String,
    pub interface_files: Vec<String>,
    pub implementations: Vec<ImplementationHit>,
    pub importers: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CapabilitySurface {
    pub name: String,
    pub layer: String,
    pub infrastructure: Vec<String>,
    pub product_surfaces: Vec<String>,
    pub evidence_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CapabilitiesReport {
    pub repo_path: String,
    pub capabilities: Vec<CapabilitySurface>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodeSearchHit {
    pub path: String,
    pub symbol: Option<String>,
    pub kind: String,
    pub rank_bucket: String,
    pub evidence_line: Option<u32>,
    pub snippet: String,
    pub is_test: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodeSearchReport {
    pub query: String,
    pub hits: Vec<CodeSearchHit>,
    pub limitations: Vec<String>,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Render an optional source line as user-facing text.  A missing line is
/// simply absent — `Some(2)` leaking through `{:?}` is debug output, not an
/// answer.
fn fmt_line(line: Option<u32>) -> String {
    line.map(|l| format!(" (line {l})")).unwrap_or_default()
}

fn is_test_path(p: &str) -> bool {
    let lower = p.to_ascii_lowercase();
    lower.starts_with("tests/")
        || lower.contains("/tests/")
        || lower.contains("__tests__")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".test.js")
        || lower.ends_with(".spec.ts")
        || lower.ends_with(".spec.js")
}

fn edge_to_callsite(e: &StructuralEdgeRow, _reverse: bool) -> CallSite {
    CallSite {
        caller_file: e.source_file.clone(),
        caller_symbol: e.source_symbol.clone(),
        callee_file: e.target_file.clone(),
        callee_symbol: e.target_symbol.clone(),
        kind: e.kind.clone(),
        evidence_line: e.evidence_line,
        evidence_snippet: e.evidence_snippet.clone(),
        is_test: is_test_path(&e.source_file),
    }
}

fn basename_stem(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .split('.')
        .next()
        .unwrap_or(path)
        .to_string()
}

fn alnum_norm(s: &str) -> String {
    s.to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

// ── Callers / callees ────────────────────────────────────────────────────────

/// Find OBSERVED callers of a symbol (or Class.method) and optional callees
/// when the subject resolves to a defining file via symbol search.
pub fn find_callers(subject: &str, repo: &str, store: &Store, limit: usize) -> Result<CallersReport> {
    let subject = subject.trim();
    let mut resolved = "symbol".to_string();

    let mut call_edges = store.structural_edges_by_target_symbol(subject, repo, limit)?;

    // Also accept a file path: reverse edges to that file.
    if call_edges.is_empty()
        && (subject.contains('/') || subject.ends_with(".ts") || subject.ends_with(".js"))
    {
        resolved = "file".to_string();
        let targeting = store.structural_edges_targeting(subject, repo)?;
        call_edges = targeting
            .into_iter()
            .filter(|e| {
                e.kind == "calls_static" || e.kind == "calls_instance" || e.kind == "imports"
            })
            .take(limit)
            .collect();
    }

    let mut prod = Vec::new();
    let mut tests = Vec::new();
    let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
    for e in &call_edges {
        if e.kind != "calls_static"
            && e.kind != "calls_instance"
            && e.kind != "references_model"
            && !(resolved == "file" && e.kind == "imports")
        {
            continue;
        }
        let key = (
            e.source_file.clone(),
            e.target_symbol.clone().unwrap_or_default(),
            e.kind.clone(),
        );
        if !seen.insert(key) {
            continue;
        }
        let site = edge_to_callsite(e, true);
        if site.is_test {
            tests.push(site);
        } else {
            prod.push(site);
        }
    }

    let mut callees = Vec::new();
    let from_src = store.structural_edges_by_source_symbol(subject, repo, limit.min(80))?;
    for e in from_src {
        if e.kind.starts_with("calls") || e.kind == "references_model" {
            callees.push(edge_to_callsite(&e, false));
        }
    }
    if callees.is_empty() && resolved == "file" {
        for e in store.structural_edges_for_file(subject, repo)? {
            if e.kind.starts_with("calls") || e.kind == "references_model" {
                callees.push(edge_to_callsite(&e, false));
            }
        }
    }

    Ok(CallersReport {
        subject: subject.to_string(),
        resolved_as: resolved,
        production_callers: prod,
        test_callers: tests,
        callees,
        limitations: vec![
            "Call edges are static snapshot from last typescript ingest (CALLS_STATIC/INSTANCE)."
                .into(),
            "Dynamic import, DI, and string-based dispatch are not observed.".into(),
            "Symbol match is string equality / Class.method suffix — not type-checked.".into(),
        ],
    })
}

// ── Implementations ──────────────────────────────────────────────────────────

/// Implementations of an interface / abstract type.
///
/// Prefers OBSERVED `implements` edges; falls back to DERIVED factory/import heuristics.
pub fn find_implementations(
    subject: &str,
    repo: &str,
    store: &Store,
    limit: usize,
) -> Result<ImplementationsReport> {
    let subject = subject.trim();
    let mut interface_files: BTreeSet<String> = BTreeSet::new();
    let mut importers: BTreeSet<String> = BTreeSet::new();
    let mut hits: Vec<ImplementationHit> = Vec::new();

    let subj_l = subject.to_ascii_lowercase();
    let stripped = subject.trim_start_matches('I').trim_start_matches('i');
    let stem_guess = stripped
        .trim_end_matches("Provider")
        .trim_end_matches("Service")
        .trim_end_matches("Interface");
    let stem_l = stem_guess.to_ascii_lowercase();

    // ── OBSERVED implements edges (preferred) ───────────────────────────────
    // Match target_symbol == InterfaceName or path subject as target_file.
    let impl_edges = store.structural_edges_by_target_symbol(subject, repo, limit.max(80))?;
    for e in &impl_edges {
        if e.kind != "implements" {
            continue;
        }
        // target_symbol match for interface name
        let sym_ok = e
            .target_symbol
            .as_ref()
            .map(|s| s == subject || s.ends_with(subject) || s == stripped)
            .unwrap_or(false);
        let file_ok = e.target_file == subject
            || e.target_file.ends_with(subject)
            || (!stem_l.is_empty() && e.target_file.to_ascii_lowercase().contains(&stem_l));
        if !sym_ok && !file_ok && e.target_symbol.as_deref() != Some(subject) {
            // still accept exact symbol search hits that are implements
            if e.target_symbol.as_ref().map(|s| alnum_norm(s) == alnum_norm(subject)).unwrap_or(false)
            {
                // ok
            } else {
                continue;
            }
        }
        if !e.target_file.starts_with("UNRESOLVED:") {
            interface_files.insert(e.target_file.clone());
        }
        hits.push(ImplementationHit {
            file: e.source_file.clone(),
            reason: format!(
                "OBSERVED implements {}{}",
                e.target_symbol.as_deref().unwrap_or("?"),
                fmt_line(e.evidence_line)
            ),
            imports_interface: true,
            is_test: is_test_path(&e.source_file),
        });
    }
    // Also: any implements edge whose target_file is the subject path
    if subject.contains('/') {
        for e in store.structural_edges_targeting(subject, repo)? {
            if e.kind == "implements" {
                interface_files.insert(subject.to_string());
                if !hits.iter().any(|h| h.file == e.source_file) {
                    hits.push(ImplementationHit {
                        file: e.source_file.clone(),
                        reason: format!(
                            "OBSERVED implements → {}{}",
                            subject,
                            fmt_line(e.evidence_line)
                        ),
                        imports_interface: true,
                        is_test: is_test_path(&e.source_file),
                    });
                }
            }
        }
    }

    // Broader: symbol search for implements kind only
    if hits.is_empty() {
        for e in store.structural_edges_symbol_search(subject, repo, 120)? {
            if e.kind != "implements" {
                continue;
            }
            if !e.target_file.starts_with("UNRESOLVED:") {
                interface_files.insert(e.target_file.clone());
            }
            if !hits.iter().any(|h| h.file == e.source_file) {
                hits.push(ImplementationHit {
                    file: e.source_file.clone(),
                    reason: format!(
                        "OBSERVED implements {}",
                        e.target_symbol.as_deref().unwrap_or("?")
                    ),
                    imports_interface: true,
                    is_test: is_test_path(&e.source_file),
                });
            }
        }
    }

    if subject.contains('/') {
        interface_files.insert(subject.to_string());
    }

    // If OBSERVED hits found, still collect importers for context, then return early-ish
    let observed_count = hits.len();

    let mut search = store.structural_edges_symbol_search(subject, repo, 120)?;
    if stem_l.len() >= 3 {
        search.extend(store.structural_edges_symbol_search(stem_guess, repo, 120)?);
    }
    for frag in [
        format!("{}.interface", stem_l),
        format!("{}interface", stem_l),
        format!("{}/", stem_l),
    ] {
        if frag.len() >= 4 {
            search.extend(store.structural_edges_symbol_search(&frag, repo, 80)?);
        }
    }

    for e in &search {
        for path in [&e.target_file, &e.source_file] {
            let pl = path.to_ascii_lowercase();
            let is_iface_path = pl.contains(".interface.")
                || pl.contains("/interface")
                || pl.ends_with("interface.ts")
                || pl.ends_with("interface.js");
            let name_aligned = !stem_l.is_empty() && pl.contains(&stem_l);
            if is_iface_path && (name_aligned || pl.contains(&subj_l)) {
                interface_files.insert(path.clone());
            }
        }
    }

    let factory_hits =
        store.structural_edges_symbol_search(&format!("{}.factory", stem_l), repo, 40)?;
    let mut factory_files: BTreeSet<String> = BTreeSet::new();
    for e in &factory_hits {
        for path in [&e.source_file, &e.target_file] {
            if path.to_ascii_lowercase().contains("factory")
                && path.to_ascii_lowercase().contains(&stem_l)
            {
                factory_files.insert(path.clone());
                for out in store.structural_edges_for_file(path, repo)? {
                    if out.kind == "imports"
                        && out.target_file.to_ascii_lowercase().contains("interface")
                    {
                        interface_files.insert(out.target_file.clone());
                    }
                    if out.kind == "imports"
                        && (out.target_file.contains("adapter")
                            || out.target_file.contains("Adapter")
                            || out.target_file.contains("implementations/"))
                    {
                        hits.push(ImplementationHit {
                            file: out.target_file.clone(),
                            reason: format!("imported by factory {}", path),
                            imports_interface: true,
                            is_test: is_test_path(&out.target_file),
                        });
                    }
                }
            }
        }
    }

    for iface in interface_files.iter().cloned().collect::<Vec<_>>() {
        for e in store.structural_importers_of(&iface, repo, limit.max(80))? {
            importers.insert(e.source_file.clone());
            let sf = e.source_file.as_str();
            let reason = if sf.contains("adapter") || sf.contains("Adapter") {
                "imports interface; path suggests adapter".to_string()
            } else if sf.contains("factory") {
                "imports interface; factory/wiring".to_string()
            } else if !stem_l.is_empty()
                && basename_stem(sf).to_ascii_lowercase().contains(&stem_l)
            {
                "imports interface; name aligns with subject".to_string()
            } else {
                "imports interface".to_string()
            };
            let is_implish = sf.contains("adapter")
                || sf.contains("Adapter")
                || sf.contains("/implementations/")
                || sf.contains("provider")
                || (sf.contains("Provider") && !sf.contains("interface"))
                || reason.contains("name aligns");
            if is_implish {
                hits.push(ImplementationHit {
                    file: sf.to_string(),
                    reason,
                    imports_interface: true,
                    is_test: is_test_path(sf),
                });
            }
        }
    }

    for e in &search {
        for path in [&e.source_file, &e.target_file] {
            let pl = path.to_ascii_lowercase();
            let family = stem_l.is_empty()
                || pl.contains(&stem_l)
                || (pl.contains("storage") && stem_l.contains("storage"));
            if (pl.contains("adapter") || pl.contains("/implementations/")) && family {
                if !hits.iter().any(|h| h.file == *path) {
                    hits.push(ImplementationHit {
                        file: path.clone(),
                        reason: "adapter/implementation path in symbol neighborhood".into(),
                        imports_interface: interface_files.iter().any(|i| {
                            let dir = i.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                            path.starts_with(dir)
                        }),
                        is_test: is_test_path(path),
                    });
                }
            }
        }
    }

    hits.sort_by(|a, b| a.is_test.cmp(&b.is_test).then_with(|| a.file.cmp(&b.file)));
    hits.dedup_by(|a, b| a.file == b.file);
    hits.truncate(limit);

    for fac in &factory_files {
        for e in store.structural_importers_of(fac, repo, 40)? {
            importers.insert(e.source_file.clone());
        }
    }

    let mut limitations = vec![
        "Plain importers of the interface file are listed under importers, not as implementors."
            .into(),
    ];
    if observed_count > 0 {
        limitations.insert(
            0,
            format!(
                "{} OBSERVED implements edge(s); remaining candidates may be DERIVED factory/import heuristics.",
                observed_count
            ),
        );
    } else {
        limitations.insert(
            0,
            "No OBSERVED implements edges for this subject — results are DERIVED from factory/import heuristics. Re-ingest with a parser that extracts `implements`.".into(),
        );
    }

    Ok(ImplementationsReport {
        subject: subject.to_string(),
        interface_files: interface_files.into_iter().collect(),
        implementations: hits,
        importers: importers.into_iter().take(limit).collect(),
        limitations,
    })
}

// ── Capabilities ─────────────────────────────────────────────────────────────

/// Infer product capability surfaces from infrastructure import fan-in.
pub fn compute_capabilities(repo: &str, store: &Store) -> Result<CapabilitiesReport> {
    let infra_seeds = [
        ("storage", "src/infrastructure/storage"),
        ("storage", "infrastructure/storage"),
        ("messaging", "src/infrastructure/messaging"),
        ("messaging", "src/infrastructure/queue"),
        ("caching", "src/infrastructure/caching"),
        ("caching", "src/infrastructure/cache"),
        ("rate-limiting", "src/infrastructure/rate-limiting"),
        ("email", "src/infrastructure/email"),
        ("logger", "src/infrastructure/logger"),
        ("auth", "src/infrastructure/auth"),
        ("database", "src/infrastructure/database"),
        ("database", "src/infrastructure/db"),
    ];

    let mut by_cap: BTreeMap<String, CapabilitySurface> = BTreeMap::new();

    for (cap_name, prefix) in infra_seeds {
        let importers = store.structural_importers_of(prefix, repo, 200)?;
        if importers.is_empty() {
            continue;
        }
        let entry = by_cap
            .entry(cap_name.to_string())
            .or_insert_with(|| CapabilitySurface {
                name: cap_name.to_string(),
                layer: "infrastructure".into(),
                infrastructure: Vec::new(),
                product_surfaces: Vec::new(),
                evidence_notes: Vec::new(),
            });

        let mut infra_files: BTreeSet<String> = BTreeSet::new();
        let mut products: BTreeSet<String> = BTreeSet::new();
        for e in &importers {
            infra_files.insert(e.target_file.clone());
            if !is_test_path(&e.source_file)
                && !e.source_file.starts_with(prefix)
                && !e.source_file.contains("/infrastructure/")
            {
                products.insert(e.source_file.clone());
            }
        }
        for e in
            store.structural_edges_from_prefix(&format!("{}/", prefix.trim_end_matches('/')), repo)?
        {
            if e.source_file.starts_with(prefix) {
                infra_files.insert(e.source_file.clone());
            }
        }

        for f in infra_files {
            if !entry.infrastructure.contains(&f) {
                entry.infrastructure.push(f);
            }
        }
        for p in products {
            if !entry.product_surfaces.contains(&p) {
                entry.product_surfaces.push(p);
            }
        }
        if entry.evidence_notes.is_empty() {
            entry.evidence_notes.push(format!(
                "Product surfaces = non-test files with OBSERVED imports into `{}`",
                prefix
            ));
        }
    }

    if let Some(storage) = by_cap.get_mut("storage") {
        storage.product_surfaces.sort();
        let mut notes = Vec::new();
        for p in &storage.product_surfaces {
            if p.contains("listing-asset") {
                notes.push(format!("data-room / listing assets: {}", p));
            } else if p.contains("kyc") || p.contains("compliance") {
                notes.push(format!("KYC/compliance storage consumer: {}", p));
            } else if p.contains("support") {
                notes.push(format!("support attachments: {}", p));
            } else if p.contains("sign") {
                notes.push(format!("signing documents: {}", p));
            } else if p.contains("payment") {
                notes.push(format!("payment proofs/media: {}", p));
            }
        }
        storage.evidence_notes.extend(notes);
        storage.infrastructure.sort();
    }

    let mut capabilities: Vec<_> = by_cap.into_values().collect();
    capabilities.sort_by(|a, b| a.name.cmp(&b.name));
    for c in &mut capabilities {
        c.product_surfaces.sort();
        c.infrastructure.sort();
    }

    Ok(CapabilitiesReport {
        repo_path: repo.to_string(),
        capabilities,
        limitations: vec![
            "Capabilities are DERIVED from import fan-in to infrastructure path prefixes.".into(),
            "Missing edges ⇒ missing surfaces. Re-run `atlas ingest . --typescript` after large refactors.".into(),
            "Not a business-domain ontology — path-derived only.".into(),
        ],
    })
}

// ── Definition-ranked code search ────────────────────────────────────────────

pub fn definition_ranked_search(
    query: &str,
    repo: &str,
    store: &Store,
    limit: usize,
) -> Result<CodeSearchReport> {
    let query = query.trim();
    let edges =
        store.structural_edges_symbol_search(query, repo, limit.saturating_mul(3).max(40))?;

    let mut hits: Vec<CodeSearchHit> = Vec::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let q_n = alnum_norm(query);

    for e in edges {
        let (path, sym) = if e
            .target_symbol
            .as_ref()
            .map(|s| s.to_ascii_lowercase().contains(&query.to_ascii_lowercase()))
            .unwrap_or(false)
            || e
                .target_file
                .to_ascii_lowercase()
                .contains(&query.to_ascii_lowercase())
        {
            (e.target_file.clone(), e.target_symbol.clone())
        } else {
            (e.source_file.clone(), e.source_symbol.clone())
        };

        let key = (path.clone(), sym.clone().unwrap_or_default());
        if !seen.insert(key) {
            continue;
        }

        let is_test = is_test_path(&path);
        let stem_n = alnum_norm(&basename_stem(&path));
        let path_n = alnum_norm(&path);
        let rank_bucket = if !q_n.is_empty()
            && (stem_n == q_n
                || stem_n == format!("{}service", q_n)
                || stem_n.starts_with(&q_n)
                || path_n.contains(&format!("{}service", q_n))
                || path_n.contains(&format!("{}model", q_n))
                || path_n.contains(&format!("{}resolver", q_n)))
        {
            "DEFINITION"
        } else if path.contains("adapter") || path.contains("factory") || path.contains("interface")
        {
            "WIRING"
        } else if is_test {
            "TEST"
        } else if e.kind.starts_with("calls") {
            "CALL_SITE"
        } else {
            "REFERENCE"
        };

        hits.push(CodeSearchHit {
            path,
            symbol: sym,
            kind: e.kind,
            rank_bucket: rank_bucket.into(),
            evidence_line: e.evidence_line,
            snippet: e.evidence_snippet.chars().take(160).collect(),
            is_test,
        });
    }

    fn bucket_ord(b: &str) -> u8 {
        match b {
            "DEFINITION" => 0,
            "WIRING" => 1,
            "CALL_SITE" => 2,
            "REFERENCE" => 3,
            "TEST" => 4,
            _ => 5,
        }
    }
    hits.sort_by(|a, b| {
        bucket_ord(&a.rank_bucket)
            .cmp(&bucket_ord(&b.rank_bucket))
            .then_with(|| a.path.cmp(&b.path))
    });
    hits.truncate(limit);

    Ok(CodeSearchReport {
        query: query.to_string(),
        hits,
        limitations: vec![
            "Ranked over structural_edges only — not a full-text code search.".into(),
            "DEFINITION bucket is path/symbol heuristic, not a language-server definition.".into(),
            "Use atlas callers / implementations for relationship questions.".into(),
        ],
    })
}

/// Emphasize callees of a subject (outgoing structural calls).
pub fn find_callees(subject: &str, repo: &str, store: &Store, limit: usize) -> Result<CallersReport> {
    let mut report = find_callers(subject, repo, store, limit)?;
    if report.callees.is_empty() {
        let search = definition_ranked_search(subject, repo, store, 5)?;
        for h in search.hits {
            if h.rank_bucket == "DEFINITION" || h.rank_bucket == "WIRING" {
                let edges = store.structural_edges_for_file(&h.path, repo)?;
                for e in edges {
                    if e.kind.starts_with("calls") || e.kind == "references_model" {
                        report.callees.push(edge_to_callsite(&e, false));
                    }
                }
                if !report.callees.is_empty() {
                    report.resolved_as = format!("callees-via:{}", h.path);
                    break;
                }
            }
        }
    }
    report.callees.truncate(limit);
    Ok(report)
}
