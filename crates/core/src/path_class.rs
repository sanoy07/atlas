//! Repository path/file class for retrieval ranking (soft signals).
//!
//! Distinct from C5.1-E `InferredRole` (entrypoint/service/satellite). This
//! classifies **where a path lives in the repository** so demos, assets, CI,
//! and notebooks do not outrank production implementation on lexical ties.
//!
//! Soft signal only — demos can still win when they are the best match.

use atlas_ir::ArtifactRole;

/// Coarse path class used as a ranking prior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathClass {
    Production,
    Library,
    Cli,
    Test,
    Example,
    Demo,
    Benchmark,
    Notebook,
    Documentation,
    Config,
    Generated,
    Vendor,
    Asset,
    Migration,
    Other,
}

/// Classify a repo-relative path. Deterministic path heuristics only.
pub fn classify_path(path: &str) -> PathClass {
    let p = path.replace('\\', "/").to_lowercase();
    let base = p.rsplit('/').next().unwrap_or(&p);

    // Assets / binary-ish first (even under docs)
    if is_asset_name(base) {
        return PathClass::Asset;
    }

    if p.contains("node_modules/")
        || p.contains("/vendor/")
        || p.starts_with("vendor/")
        || p.contains("/third_party/")
    {
        return PathClass::Vendor;
    }

    if p.contains("/generated/")
        || p.contains("/.generated/")
        || p.contains("/dist/")
        || p.contains("/build/")
        || p.contains("/target/")
        || base.ends_with(".min.js")
        || base.ends_with(".pb.rs")
        || p.contains("/protos/")
        || p.contains(".gen.")
    {
        // protos can be source-of-truth in some repos — keep Generated so soft demotion applies
        if p.contains("/protos/") || base.ends_with(".proto") || base.ends_with(".thrift") {
            return PathClass::Generated;
        }
        if p.contains("/generated/") || p.contains("/dist/") || p.contains("/build/") {
            return PathClass::Generated;
        }
    }

    if p.contains("/migrations/") || p.starts_with("migrations/") {
        return PathClass::Migration;
    }

    if p.starts_with("demos/")
        || p.contains("/demos/")
        || p.starts_with("demo/")
        || p.contains("/demo/")
    {
        return PathClass::Demo;
    }

    if p.starts_with("examples/")
        || p.starts_with("example/")
        || p.contains("/examples/")
        || p.contains("/example/")
    {
        return PathClass::Example;
    }

    if p.starts_with("benches/")
        || p.starts_with("benchmarks/")
        || p.contains("/benches/")
        || p.contains("/benchmarks/")
        || base.starts_with("bench_")
    {
        return PathClass::Benchmark;
    }

    if p.starts_with("notebooks/")
        || p.contains("/notebooks/")
        || base.ends_with(".ipynb")
    {
        return PathClass::Notebook;
    }

    if p.starts_with("docs/")
        || p.starts_with("doc/")
        || p.starts_with("web/docs/")
        || base.ends_with(".md")
        || base.ends_with(".rst")
        || base.ends_with(".adoc")
        || base.ends_with(".mdx")
    {
        return PathClass::Documentation;
    }

    if p.starts_with(".github/")
        || p.starts_with(".cargo/")
        || p.starts_with(".config/")
        || base.ends_with(".yml")
        || base.ends_with(".yaml")
        || base.ends_with(".toml") && !base.starts_with("cargo")
        || base == "dockerfile"
        || base.starts_with("dockerfile.")
    {
        // Cargo.toml at crate roots is often production layout signal
        if base == "cargo.toml" || base == "cargo.lock" {
            return PathClass::Config;
        }
        if p.starts_with(".github/") || base.ends_with(".yml") || base.ends_with(".yaml") {
            return PathClass::Config;
        }
    }

    if is_test_path(&p) {
        return PathClass::Test;
    }

    // CLI vs library layout cues (soft)
    if p.starts_with("cli/")
        || p.contains("/cli/src/")
        || p.starts_with("apps/")
        || p.contains("/bin/")
        || base == "main.rs" && (p.contains("/cli/") || p.contains("/bin/"))
    {
        return PathClass::Cli;
    }

    if p.starts_with("lib/")
        || p.contains("/lib/src/")
        || p.starts_with("crates/")
        || (p.starts_with("src/") && is_source_ext(base))
    {
        return if p.starts_with("lib/") || p.contains("/lib/src/") {
            PathClass::Library
        } else {
            PathClass::Production
        };
    }

    if is_source_ext(base) {
        return PathClass::Production;
    }

    PathClass::Other
}

fn is_asset_name(base: &str) -> bool {
    base.ends_with(".png")
        || base.ends_with(".jpg")
        || base.ends_with(".jpeg")
        || base.ends_with(".gif")
        || base.ends_with(".svg")
        || base.ends_with(".webp")
        || base.ends_with(".ico")
        || base.ends_with(".pdf")
        || base.ends_with(".mp4")
        || base.ends_with(".wav")
        || base.ends_with(".woff")
        || base.ends_with(".woff2")
        || base.ends_with(".ttf")
}

fn is_test_path(p: &str) -> bool {
    p.starts_with("tests/")
        || p.starts_with("test/")
        || p.contains("/tests/")
        || p.contains("/test/")
        || p.contains("/__tests__/")
        || p.contains(".test.")
        || p.contains(".spec.")
        || p.ends_with("_test.rs")
        || p.ends_with("_tests.rs")
        || p.contains("/testing/")
}

fn is_source_ext(base: &str) -> bool {
    base.ends_with(".rs")
        || base.ends_with(".ts")
        || base.ends_with(".tsx")
        || base.ends_with(".js")
        || base.ends_with(".jsx")
        || base.ends_with(".py")
        || base.ends_with(".go")
        || base.ends_with(".java")
        || base.ends_with(".kt")
        || base.ends_with(".c")
        || base.ends_with(".h")
        || base.ends_with(".cpp")
        || base.ends_with(".cc")
}

/// Map path class → soft multiplicative prior (1.0 = neutral).
/// Question can opt into test/demo when it names them.
pub fn class_rank_multiplier(class: PathClass, question: &str) -> f32 {
    let q = question.to_lowercase();
    let wants_test = q.contains("test") || q.contains("spec") || q.contains("fixture");
    let wants_demo = q.contains("demo") || q.contains("tutorial") || q.contains("walkthrough");
    let wants_bench = q.contains("bench") || q.contains("performance") || q.contains("throughput");
    let wants_docs = q.contains("document") || q.contains("readme") || q.contains("docs ");
    let wants_cli = q.contains("cli") || q.contains("command line") || q.contains("subcommand");
    let wants_example = q.contains("example") || q.contains("sample");
    let wants_ci = q.contains("ci") || q.contains("workflow") || q.contains("github action");

    match class {
        PathClass::Production | PathClass::Library => 1.35,
        PathClass::Cli => {
            if wants_cli {
                1.25
            } else {
                // CLI is real code; slight demotion vs library for architecture questions
                0.95
            }
        }
        PathClass::Test => {
            if wants_test {
                1.1
            } else {
                0.45
            }
        }
        PathClass::Example => {
            if wants_example {
                1.15
            } else {
                0.4
            }
        }
        PathClass::Demo => {
            if wants_demo {
                1.2
            } else {
                0.22
            }
        }
        PathClass::Benchmark => {
            if wants_bench {
                1.15
            } else {
                0.35
            }
        }
        PathClass::Notebook => {
            if q.contains("notebook") {
                1.0
            } else {
                0.2
            }
        }
        PathClass::Documentation => {
            if wants_docs || q.contains("what is") || q.contains("overview") {
                0.85
            } else {
                0.35
            }
        }
        PathClass::Config => {
            if wants_ci || q.contains("config") {
                0.9
            } else {
                0.25
            }
        }
        PathClass::Generated => 0.3,
        PathClass::Vendor => 0.1,
        PathClass::Asset => 0.08,
        PathClass::Migration => 0.5,
        PathClass::Other => 0.7,
    }
}

/// Additive boost for exact subject stem hits on production/library paths.
pub fn class_subject_boost(class: PathClass) -> f32 {
    match class {
        PathClass::Production | PathClass::Library => 4.0,
        PathClass::Cli => 2.0,
        PathClass::Test => 0.5,
        PathClass::Example | PathClass::Demo => 0.0,
        PathClass::Documentation => 0.5,
        PathClass::Asset | PathClass::Vendor => -5.0,
        _ => 0.0,
    }
}

/// Bridge to existing ArtifactRole where useful for IR.
pub fn to_artifact_role(class: PathClass) -> ArtifactRole {
    match class {
        PathClass::Test => ArtifactRole::Test,
        PathClass::Example | PathClass::Demo => ArtifactRole::Example,
        PathClass::Documentation => ArtifactRole::Documentation,
        PathClass::Migration => ArtifactRole::Migration,
        PathClass::Generated => ArtifactRole::Generated,
        PathClass::Config => ArtifactRole::Unknown,
        _ => ArtifactRole::ProductionSource,
    }
}

/// Apply path-class soft ranking to an already-scored candidate list.
/// `base_score` is the current combined score; returns adjusted score.
pub fn apply_class_to_score(path: &str, question: &str, base_score: f32) -> f32 {
    let class = classify_path(path);
    let mult = class_rank_multiplier(class, question);
    let mut s = base_score * mult;
    // Extra demotion when lexical match is only on non-production class
    if matches!(
        class,
        PathClass::Asset | PathClass::Demo | PathClass::Notebook | PathClass::Config
    ) && base_score > 0.0
    {
        s *= 0.85;
    }
    s.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_beats_demo_and_asset() {
        let q = "How are first-class conflicts represented and merged?";
        let prod = apply_class_to_score("lib/src/conflicts.rs", q, 10.0);
        let demo = apply_class_to_score("demos/demo_juggle_conflicts.sh", q, 10.0);
        let png = apply_class_to_score("demos/juggle_conflicts.png", q, 10.0);
        assert!(prod > demo, "prod {prod} vs demo {demo}");
        assert!(prod > png, "prod {prod} vs png {png}");
        assert!(demo > png);
    }

    #[test]
    fn ci_and_notebook_demoted() {
        let q = "How does batch encoding work?";
        let src = apply_class_to_score("src/batch.rs", q, 8.0);
        let ci = apply_class_to_score(".github/workflows/CI.yml", q, 8.0);
        let nb = apply_class_to_score("notebooks/inspect_tokenizers.py", q, 8.0);
        assert!(src > ci);
        assert!(src > nb);
    }

    #[test]
    fn library_vs_cli_layout() {
        assert_eq!(classify_path("lib/src/op_store.rs"), PathClass::Library);
        assert_eq!(
            classify_path("cli/src/commands/operation/log.rs"),
            PathClass::Cli
        );
        assert_eq!(classify_path("src/bpe/mod.rs"), PathClass::Production);
    }
}
