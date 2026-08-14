use anyhow::{Context, Result};
use atlas_core::investigate;
use atlas_storage::Store;
use serde::Deserialize;
use std::path::Path;

// ── Benchmark schema ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct BenchmarkFile {
    #[serde(rename = "case")]
    cases: Vec<BenchmarkCase>,
}

#[derive(Deserialize)]
struct BenchmarkCase {
    id:                   String,
    description:          String,
    repo:                 String,
    db:                   String,
    anchors:              Vec<String>,
    expected_center:      Vec<String>,
    #[serde(default)]
    supporting_evidence:  Vec<String>,
    #[serde(default)]
    false_positive_alert: Vec<String>,
}

// ── Score for a single case ───────────────────────────────────────────────────

struct CaseScore {
    id:                   String,
    description:          String,
    center_found:         usize,
    center_total:         usize,
    center_first_rank:    Option<usize>,
    supporting_found:     usize,
    supporting_total:     usize,
    noise_found:          usize,
    noise_total:          usize,
    error:                Option<String>,
}

impl CaseScore {
    fn center_rate(&self) -> f32 {
        if self.center_total == 0 { return 1.0; }
        self.center_found as f32 / self.center_total as f32
    }

    fn supporting_rate(&self) -> f32 {
        if self.supporting_total == 0 { return 1.0; }
        self.supporting_found as f32 / self.supporting_total as f32
    }

    fn noise_rate(&self) -> f32 {
        if self.noise_total == 0 { return 0.0; }
        self.noise_found as f32 / self.noise_total as f32
    }

    fn pass(&self) -> bool {
        self.error.is_none() && self.center_rate() >= 0.75 && self.noise_rate() <= 0.25
    }
}

// ── Main entry point ──────────────────────────────────────────────────────────

pub fn run(corpus_dir: &str, verbose: bool) -> Result<()> {
    let dir = Path::new(corpus_dir);
    if !dir.exists() {
        anyhow::bail!("corpus directory not found: {}", corpus_dir);
    }

    let mut all_cases: Vec<BenchmarkCase> = Vec::new();

    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "toml").unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.path());

    for entry in entries {
        let path = entry.path();
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let file: BenchmarkFile = toml::from_str(&content)
            .with_context(|| format!("parsing {}", path.display()))?;
        all_cases.extend(file.cases);
    }

    if all_cases.is_empty() {
        println!("No benchmark cases found in {}", corpus_dir);
        return Ok(());
    }

    println!("Atlas Investigation Benchmark");
    println!("corpus: {}  ({} cases)", corpus_dir, all_cases.len());
    println!();

    let mut scores: Vec<CaseScore> = Vec::new();

    for case in &all_cases {
        let score = run_case(case, verbose);
        print_case_result(&score, verbose);
        scores.push(score);
    }

    print_aggregate(&scores);
    Ok(())
}

fn run_case(case: &BenchmarkCase, verbose: bool) -> CaseScore {
    if verbose {
        println!("  running {} ({}) …", case.id, case.anchors.join(" · "));
    }

    let store = match Store::open(&case.db) {
        Ok(s) => s,
        Err(e) => return error_score(case, format!("cannot open DB {}: {}", case.db, e)),
    };

    let anchor_strs: Vec<&str> = case.anchors.iter().map(String::as_str).collect();
    let doc = match investigate(&anchor_strs, &case.repo, &store) {
        Ok(d) => d,
        Err(e) => return error_score(case, format!("investigate failed: {}", e)),
    };

    // Score center: expected_center must be in core_candidates
    let core_paths: Vec<&str> = doc.core_candidates.iter().map(|c| c.file.as_str()).collect();

    let mut center_found = 0;
    let mut center_first_rank: Option<usize> = None;

    for expected in &case.expected_center {
        // Find first candidate that contains the expected substring
        if let Some((rank, _)) = core_paths.iter().enumerate()
            .find(|(_, p)| p.contains(expected.as_str()))
        {
            center_found += 1;
            let rank1 = rank + 1;
            center_first_rank = Some(match center_first_rank {
                None => rank1,
                Some(prev) => prev.min(rank1),
            });
        }
    }

    // Score supporting: expected in supporting_artifacts
    let support_paths: Vec<&str> = doc.supporting_artifacts.iter().map(|c| c.file.as_str()).collect();
    let supporting_found = case.supporting_evidence.iter()
        .filter(|exp| support_paths.iter().any(|p| p.contains(exp.as_str())))
        .count();

    // Score noise: false_positive_alert should NOT be in core_candidates
    let noise_found = case.false_positive_alert.iter()
        .filter(|fp| core_paths.iter().any(|p| p.contains(fp.as_str())))
        .count();

    CaseScore {
        id:                   case.id.clone(),
        description:          case.description.clone(),
        center_found,
        center_total:         case.expected_center.len(),
        center_first_rank,
        supporting_found,
        supporting_total:     case.supporting_evidence.len(),
        noise_found,
        noise_total:          case.false_positive_alert.len(),
        error:                None,
    }
}

fn error_score(case: &BenchmarkCase, msg: String) -> CaseScore {
    CaseScore {
        id:                case.id.clone(),
        description:       case.description.clone(),
        center_found:      0,
        center_total:      case.expected_center.len(),
        center_first_rank: None,
        supporting_found:  0,
        supporting_total:  case.supporting_evidence.len(),
        noise_found:       0,
        noise_total:       case.false_positive_alert.len(),
        error:             Some(msg),
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn print_case_result(s: &CaseScore, _verbose: bool) {
    let status = if let Some(ref e) = s.error {
        format!("ERROR: {}", e)
    } else {
        let pass = if s.pass() { "PASS" } else { "FAIL" };
        let rank_str = s.center_first_rank
            .map(|r| format!(" rank={}", r))
            .unwrap_or_default();
        format!(
            "{} center={}/{} ({:.0}%){} support={}/{} noise={}/{}",
            pass,
            s.center_found, s.center_total,
            s.center_rate() * 100.0,
            rank_str,
            s.supporting_found, s.supporting_total,
            s.noise_found, s.noise_total,
        )
    };
    println!("  {:30}  {}", s.id, status);
    if s.error.is_none() && !s.pass() {
        println!("    description: {}", s.description);
    }
}

fn print_aggregate(scores: &[CaseScore]) {
    println!();
    println!("AGGREGATE");

    let total   = scores.len();
    let errors  = scores.iter().filter(|s| s.error.is_some()).count();
    let passed  = scores.iter().filter(|s| s.pass()).count();
    let failed  = total - passed - errors;

    let valid: Vec<_> = scores.iter().filter(|s| s.error.is_none()).collect();

    let avg_center = if valid.is_empty() { 0.0 } else {
        valid.iter().map(|s| s.center_rate()).sum::<f32>() / valid.len() as f32
    };
    let avg_support = if valid.is_empty() { 0.0 } else {
        valid.iter().map(|s| s.supporting_rate()).sum::<f32>() / valid.len() as f32
    };
    let avg_noise = if valid.is_empty() { 0.0 } else {
        valid.iter().map(|s| s.noise_rate()).sum::<f32>() / valid.len() as f32
    };

    println!("  cases:             {}", total);
    println!("  passed:            {}  ({:.0}%)", passed, passed as f32 / total as f32 * 100.0);
    println!("  failed:            {}", failed);
    if errors > 0 {
        println!("  errors:            {}", errors);
    }
    println!("  avg center rate:   {:.0}%", avg_center * 100.0);
    println!("  avg support rate:  {:.0}%", avg_support * 100.0);
    println!("  avg noise rate:    {:.0}%", avg_noise * 100.0);
}
