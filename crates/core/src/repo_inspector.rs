//! Repository inspector — deterministic observation surface for a single repository.
//!
//! Produces `Vec<ProfileClaim>` from what is directly observable on disk.  Every
//! claim carries specific `ClaimEvidence` (a file path, a package.json field, a
//! lockfile filename) — no inference, no interpretation.  The inspector never
//! decides what a repository *is*; it only reports what it *observes*.
//!
//! This is an observation surface, not a new concept.  All produced types
//! (`ProfileClaim`, `ProfileClaimKind`, `ClaimEvidence`) already exist in the IR.
//! Adding this module extends what Atlas can see without committing to a new
//! ontology.

use anyhow::Result;
use atlas_ir::{ClaimEvidence, ProfileClaim, ProfileClaimKind};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Deterministic inspection of `repo_path`.  Returns an empty vec on any I/O
/// failure of a specific probe — a missing file is a valid observation
/// (evidence of absence), not an error.
pub fn inspect_repository(repo_path: &str) -> Result<Vec<ProfileClaim>> {
    let root = Path::new(repo_path);
    let mut claims: Vec<ProfileClaim> = Vec::new();

    inspect_package_json(root, &mut claims);
    inspect_lockfiles(root, &mut claims);
    inspect_modules(root, &mut claims);
    inspect_source_extensions(root, &mut claims);
    inspect_cargo_toml(root, &mut claims);

    Ok(claims)
}

// ── Probes ───────────────────────────────────────────────────────────────────

fn inspect_package_json(root: &Path, claims: &mut Vec<ProfileClaim>) {
    let pkg_path = root.join("package.json");
    let Ok(content) = std::fs::read_to_string(&pkg_path) else { return };
    let Ok(json): std::result::Result<Value, _> = serde_json::from_str(&content) else { return };

    // main entry point
    if let Some(main) = json.get("main").and_then(|v| v.as_str()) {
        claims.push(ProfileClaim {
            kind:     ProfileClaimKind::EntryPoint,
            value:    main.to_string(),
            evidence: ClaimEvidence::PackageJsonMain { value: main.to_string() },
        });
    }

    // scripts — only surface as evidence when present; no interpretation
    if let Some(scripts) = json.get("scripts").and_then(|v| v.as_object()) {
        for (name, cmd) in scripts {
            if let Some(cmd_str) = cmd.as_str() {
                claims.push(ProfileClaim {
                    kind:     ProfileClaimKind::EntryPoint,
                    value:    format!("script:{}", name),
                    evidence: ClaimEvidence::PackageJsonScript {
                        script_name: name.clone(),
                        command:     cmd_str.to_string(),
                    },
                });
            }
        }
    }

    // dependencies — one Framework claim per package name.  We do NOT classify
    // packages by role; the claim is "this dependency was declared", the caller
    // decides what to do with the name.
    for section in ["dependencies", "devDependencies"] {
        if let Some(deps) = json.get(section).and_then(|v| v.as_object()) {
            for (pkg, version) in deps {
                let version_str = version.as_str().unwrap_or("").to_string();
                claims.push(ProfileClaim {
                    kind:     dependency_kind(pkg),
                    value:    pkg.clone(),
                    evidence: ClaimEvidence::PackageJsonDependency {
                        package: pkg.clone(),
                        version: version_str,
                    },
                });
            }
        }
    }

    // Runtime marker: presence of a package.json means Node.js runtime is claimed.
    claims.push(ProfileClaim {
        kind:     ProfileClaimKind::Runtime,
        value:    "Node.js".to_string(),
        evidence: ClaimEvidence::FileExists { relative_path: "package.json".to_string() },
    });
}

/// Map a dependency name to the most specific ProfileClaimKind we can support
/// with existing IR variants.  This is a **naming projection**, not a
/// classification: the evidence remains the raw package name.  When multiple
/// variants could apply, we prefer the more specific one.
fn dependency_kind(pkg: &str) -> ProfileClaimKind {
    match pkg {
        // Persistence
        "mongoose" | "mongodb" | "typeorm" | "prisma" | "sequelize" | "knex" | "redis" | "ioredis"
            => ProfileClaimKind::Persistence,
        // Messaging
        "@google-cloud/pubsub" | "amqplib" | "kafkajs" | "bull" | "bullmq" | "@nestjs/microservices"
            => ProfileClaimKind::Messaging,
        // Auth
        "firebase-admin" | "passport" | "jsonwebtoken" | "@nestjs/jwt" | "@nestjs/passport"
            => ProfileClaimKind::Auth,
        // Email
        "@sendgrid/mail" | "nodemailer" | "resend" | "postmark"
            => ProfileClaimKind::EmailProvider,
        // Template
        "handlebars" | "pug" | "ejs" | "mjml"
            => ProfileClaimKind::TemplateEngine,
        // Blockchain
        "@polymeshassociation/polymesh-sdk" | "ethers" | "web3" | "@solana/web3.js"
            => ProfileClaimKind::BlockchainClient,
        // Default: Framework — a declared dependency, role unclassified
        _ => ProfileClaimKind::Framework,
    }
}

fn inspect_lockfiles(root: &Path, claims: &mut Vec<ProfileClaim>) {
    for lockfile in ["package-lock.json", "yarn.lock", "pnpm-lock.yaml", "bun.lockb", "Cargo.lock"] {
        if root.join(lockfile).exists() {
            claims.push(ProfileClaim {
                kind:     ProfileClaimKind::PackageManager,
                value:    package_manager_for_lockfile(lockfile).to_string(),
                evidence: ClaimEvidence::LockfilePresent { filename: lockfile.to_string() },
            });
        }
    }
}

fn package_manager_for_lockfile(filename: &str) -> &'static str {
    match filename {
        "package-lock.json" => "npm",
        "yarn.lock"         => "yarn",
        "pnpm-lock.yaml"    => "pnpm",
        "bun.lockb"         => "bun",
        "Cargo.lock"        => "cargo",
        _                   => "unknown",
    }
}

fn inspect_modules(root: &Path, claims: &mut Vec<ProfileClaim>) {
    // Top-level src/ subdirectories are surfaced as observed modules.
    // Only direct children of src/ — no recursion.  Presence of the directory
    // is the evidence.
    let src = root.join("src");
    let Ok(entries) = std::fs::read_dir(&src) else { return };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() { continue }
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else { continue };
        // Skip hidden directories.
        if name.starts_with('.') { continue }
        claims.push(ProfileClaim {
            kind:     ProfileClaimKind::Module,
            value:    name.clone(),
            evidence: ClaimEvidence::DirectoryExists {
                relative_path: format!("src/{}", name),
            },
        });
    }
}

fn inspect_source_extensions(root: &Path, claims: &mut Vec<ProfileClaim>) {
    // Report each source language we observe by scanning file extensions,
    // capping the walk at a small depth to stay deterministic and cheap.
    // For each language we produce at most one claim, carrying an example
    // path as evidence.
    let mut observed: BTreeSet<&'static str> = BTreeSet::new();
    let mut examples: std::collections::HashMap<&'static str, PathBuf> = Default::default();

    walk_shallow(root, 3, &mut |path| {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else { return };
        let lang = match ext {
            "ts" | "tsx"           => Some(("TypeScript", ext)),
            "js" | "jsx" | "mjs" | "cjs" => Some(("JavaScript", ext)),
            "rs"                   => Some(("Rust", ext)),
            "py"                   => Some(("Python", ext)),
            "go"                   => Some(("Go", ext)),
            "c" | "h"              => Some(("C", ext)),
            "cpp" | "cc" | "hpp"   => Some(("C++", ext)),
            _                      => None,
        };
        if let Some((lang, ext)) = lang {
            if observed.insert(lang) {
                examples.insert(lang, path.strip_prefix(root).unwrap_or(path).to_path_buf());
                let _ = ext; // ext retained for potential future use in evidence
            }
        }
    });

    for lang in observed {
        let example = examples
            .get(lang)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let extension = example
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_string();
        claims.push(ProfileClaim {
            kind:     ProfileClaimKind::Language,
            value:    lang.to_string(),
            evidence: ClaimEvidence::SourceExtension {
                extension,
                example_path: example,
            },
        });
    }
}

fn inspect_cargo_toml(root: &Path, claims: &mut Vec<ProfileClaim>) {
    let cargo = root.join("Cargo.toml");
    if cargo.exists() {
        claims.push(ProfileClaim {
            kind:     ProfileClaimKind::Runtime,
            value:    "Rust".to_string(),
            evidence: ClaimEvidence::FileExists { relative_path: "Cargo.toml".to_string() },
        });
    }
}

// ── Small shallow walker (bounded, deterministic) ────────────────────────────

fn walk_shallow(root: &Path, max_depth: usize, cb: &mut dyn FnMut(&Path)) {
    walk_inner(root, root, max_depth, cb);
}

fn walk_inner(root: &Path, dir: &Path, remaining_depth: usize, cb: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    // Sort for determinism.
    let mut sorted: Vec<_> = entries.flatten().collect();
    sorted.sort_by_key(|e| e.file_name());

    for entry in sorted {
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip common noise directories and hidden entries.
        if ft.is_dir() {
            if name_str.starts_with('.')
                || matches!(name_str.as_ref(),
                    "node_modules" | "target" | "dist" | "build" | ".next" | "coverage"
                    | "__pycache__" | ".cache" | "out" | ".nuxt")
            {
                continue;
            }
            if remaining_depth > 0 {
                walk_inner(root, &path, remaining_depth - 1, cb);
            }
        } else if ft.is_file() {
            cb(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn inspects_typescript_node_project() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        write(root, "package.json", r#"{
            "name": "app",
            "main": "src/server.ts",
            "scripts": { "start": "node dist/server.js" },
            "dependencies": {
                "mongoose": "^7.0.0",
                "@google-cloud/pubsub": "^4.0.0",
                "express": "^4.18.0"
            }
        }"#);
        write(root, "package-lock.json", "{}");
        write(root, "src/services/user.service.ts", "export {}\n");
        write(root, "src/models/user.model.ts", "export {}\n");

        let claims = inspect_repository(root.to_str().unwrap()).unwrap();

        // Runtime observed
        assert!(claims.iter().any(|c|
            matches!(c.kind, ProfileClaimKind::Runtime) && c.value == "Node.js"));
        // Entry point captured
        assert!(claims.iter().any(|c|
            matches!(c.kind, ProfileClaimKind::EntryPoint) && c.value == "src/server.ts"));
        // Mongoose classified as Persistence
        assert!(claims.iter().any(|c|
            matches!(c.kind, ProfileClaimKind::Persistence) && c.value == "mongoose"));
        // Pub/Sub classified as Messaging
        assert!(claims.iter().any(|c|
            matches!(c.kind, ProfileClaimKind::Messaging) && c.value == "@google-cloud/pubsub"));
        // Express falls through to Framework — not misclassified
        assert!(claims.iter().any(|c|
            matches!(c.kind, ProfileClaimKind::Framework) && c.value == "express"));
        // npm inferred from lockfile
        assert!(claims.iter().any(|c|
            matches!(c.kind, ProfileClaimKind::PackageManager) && c.value == "npm"));
        // src/ subdirectories surfaced as modules
        assert!(claims.iter().any(|c|
            matches!(c.kind, ProfileClaimKind::Module) && c.value == "services"));
        assert!(claims.iter().any(|c|
            matches!(c.kind, ProfileClaimKind::Module) && c.value == "models"));
        // TypeScript language observed
        assert!(claims.iter().any(|c|
            matches!(c.kind, ProfileClaimKind::Language) && c.value == "TypeScript"));
    }

    #[test]
    fn empty_directory_produces_no_claims() {
        let dir = TempDir::new().unwrap();
        let claims = inspect_repository(dir.path().to_str().unwrap()).unwrap();
        assert!(claims.is_empty(), "empty repo should produce zero observations");
    }

    #[test]
    fn cargo_project_observes_rust_runtime() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(root, "Cargo.toml", "[package]\nname = \"x\"\nversion = \"0.1.0\"\n");
        write(root, "Cargo.lock", "");
        write(root, "src/lib.rs", "pub fn hello() {}\n");

        let claims = inspect_repository(root.to_str().unwrap()).unwrap();
        assert!(claims.iter().any(|c|
            matches!(c.kind, ProfileClaimKind::Runtime) && c.value == "Rust"));
        assert!(claims.iter().any(|c|
            matches!(c.kind, ProfileClaimKind::PackageManager) && c.value == "cargo"));
        assert!(claims.iter().any(|c|
            matches!(c.kind, ProfileClaimKind::Language) && c.value == "Rust"));
    }
}
