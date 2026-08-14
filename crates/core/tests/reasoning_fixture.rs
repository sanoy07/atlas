//! Reasoning loop: evidence packet, claim verification, fake local AI, no-AI fallback.

use atlas_core::{
    anchors_from_question, build_evidence_packet, options_from_question, parse_reasoning_response,
    run_reasoning_investigation, verify_claims, FakeReasoningProvider, PacketOptions,
    ReasoningOptions,
};
use atlas_ir::{
    AiReasoningResponse, ClaimStatus, Commit, EvidenceRef, Hypothesis, ProposedClaim,
    StructuralEdge, StructuralEdgeKind, StructuralEvidence,
};
use atlas_storage::Store;
use chrono::{DateTime, Utc};

fn store() -> Store {
    Store::open(":memory:").unwrap()
}

fn commit(s: &Store, repo: &str, hash: &str, ts: i64, files: &[&str], msg: &str) {
    let c = Commit {
        hash: hash.into(),
        short_hash: hash[..7.min(hash.len())].into(),
        message: msg.into(),
        author_name: "Dev".into(),
        author_email: "d@x.com".into(),
        timestamp: DateTime::<Utc>::from_timestamp(ts, 0).unwrap(),
        files_changed: files.iter().map(|f| f.to_string()).collect(),
        parents: vec![],
    };
    s.insert_commit(&c, repo).unwrap();
}

fn edge(s: &Store, repo: &str, src: &str, tgt: &str) {
    s.insert_structural_edge(
        &StructuralEdge {
            source_file: src.into(),
            source_symbol: None,
            target_file: tgt.into(),
            target_symbol: None,
            kind: StructuralEdgeKind::Imports,
            evidence: StructuralEvidence {
                source_file: src.into(),
                line: Some(1),
                snippet: "import".into(),
                extractor: "test".into(),
            },
        },
        repo,
    )
    .unwrap();
}

fn seed_order_repo(s: &Store, repo: &str) {
    commit(
        s,
        repo,
        "aaa1111",
        1000,
        &["src/modules/core/services/order.service.ts"],
        "feat: order service",
    );
    commit(
        s,
        repo,
        "bbb2222",
        2000,
        &[
            "src/modules/core/services/order.service.ts",
            "src/modules/payment/services/payment.service.ts",
        ],
        "feat: payment for orders",
    );
    commit(
        s,
        repo,
        "ccc3333",
        3000,
        &["src/modules/core/services/order.service.ts"],
        "fix: order timeout retry",
    );
    edge(
        s,
        repo,
        "src/modules/core/services/order.service.ts",
        "src/modules/payment/services/payment.service.ts",
    );
}

#[test]
fn anchors_from_question_strips_stopwords() {
    let a = anchors_from_question("orders sometimes timeout during concurrent processing");
    assert!(a.iter().any(|x| x == "orders"));
    assert!(a.iter().any(|x| x == "timeout"));
    assert!(a.iter().any(|x| x == "concurrent") || a.iter().any(|x| x == "processing"));
    assert!(!a.iter().any(|x| x == "during"));
    assert!(!a.iter().any(|x| x == "sometimes"));
}

#[test]
fn evidence_packet_is_bounded_and_has_limitations() {
    let s = store();
    seed_order_repo(&s, "/repo");
    let packet = build_evidence_packet(
        &PacketOptions {
            question: "orders timeout".into(),
            anchors: vec!["order".into(), "timeout".into()],
            seed_files: vec![],
            max_rounds_hint: 3,
        },
        "/repo",
        &s,
    )
    .unwrap();
    assert_eq!(packet.schema_version, 2);
    assert!(!packet.limitations.is_empty());
    assert!(!packet.bounds.is_empty());
    assert!(packet.investigation.core_candidates.len() <= 12);
    assert!(
        !packet.verification_policy.is_empty(),
        "C4-ER packet must carry verification_policy"
    );
    assert!(
        !packet.ranked_evidence.is_empty() || packet.investigation.core_candidates.is_empty(),
        "C4-ER packet should rank evidence when candidates exist"
    );
}

#[test]
fn verify_claims_marks_invented_refs_contradicted() {
    let s = store();
    seed_order_repo(&s, "/repo");
    let path = "src/modules/core/services/order.service.ts";
    let packet = build_evidence_packet(
        &PacketOptions {
            question: "order".into(),
            anchors: vec!["order".into()],
            seed_files: vec![path.into()],
            max_rounds_hint: 1,
        },
        "/repo",
        &s,
    )
    .unwrap();

    let claims = vec![
        ProposedClaim {
            id: "c1".into(),
            subject: "order".into(),
            statement: "order.service is involved".into(),
            kind: "structural".into(),
            evidence_refs: vec![EvidenceRef {
                kind: "file".into(),
                id: path.into(),
                summary: "candidate".into(),
                timestamp: None,
            }],
            method: "test".into(),
            temporal_scope: "".into(),
            limitations: vec![],
            status: ClaimStatus::Unresolved,
        },
        ProposedClaim {
            id: "c2".into(),
            subject: "ghost".into(),
            statement: "ghost file exists".into(),
            kind: "structural".into(),
            evidence_refs: vec![EvidenceRef {
                kind: "file".into(),
                id: "src/does/not/exist.ts".into(),
                summary: "invented".into(),
                timestamp: None,
            }],
            method: "hallucination".into(),
            temporal_scope: "".into(),
            limitations: vec![],
            status: ClaimStatus::Unresolved,
        },
        ProposedClaim {
            id: "c3".into(),
            subject: "x".into(),
            statement: "no evidence".into(),
            kind: "causal".into(),
            evidence_refs: vec![],
            method: "guess".into(),
            temporal_scope: "".into(),
            limitations: vec![],
            status: ClaimStatus::Unresolved,
        },
    ];
    let v = verify_claims(&claims, &packet);
    assert_eq!(v[0].status, ClaimStatus::Supported);
    assert_eq!(v[1].status, ClaimStatus::Contradicted);
    assert_eq!(v[2].status, ClaimStatus::Unresolved);
}

#[test]
fn parse_reasoning_response_accepts_fenced_json() {
    let text = r#"Here you go:
```json
{
  "hypotheses": [{"id":"h1","statement":"retry path","status":"plausible","supporting":[],"contradicting":[],"claims":[]}],
  "requested_subjects": ["src/modules/core/services/order.service.ts"],
  "questions": [],
  "proposed_claims": [],
  "explanation": "brief"
}
```
"#;
    let r = parse_reasoning_response(text);
    assert_eq!(r.hypotheses.len(), 1);
    assert_eq!(r.hypotheses[0].id, "h1");
    assert_eq!(r.explanation, "brief");
}

#[test]
fn no_ai_fallback_produces_deterministic_result() {
    let s = store();
    seed_order_repo(&s, "/repo");
    let mut opts = options_from_question("order timeout retry");
    opts.no_ai = true;
    opts.seed_files = vec!["src/modules/core/services/order.service.ts".into()];
    let result = run_reasoning_investigation(opts, "/repo", &s, None).unwrap();
    assert_eq!(result.mode, "deterministic_only");
    assert!(result.model.is_none());
    assert!(!result.packet.investigation.core_candidates.is_empty() || !result.hypotheses.is_empty());
    assert!(!result.what_atlas_does_not_know.is_empty());
}

#[test]
fn fake_provider_hypotheses_are_verified() {
    let s = store();
    seed_order_repo(&s, "/repo");
    let path = "src/modules/core/services/order.service.ts";
    let fake = FakeReasoningProvider::new(AiReasoningResponse {
        hypotheses: vec![Hypothesis {
            id: "h1".into(),
            statement: "Timeout related to order service retry path".into(),
            status: ClaimStatus::Plausible,
            supporting: vec![EvidenceRef {
                kind: "file".into(),
                id: path.into(),
                summary: "order service".into(),
                timestamp: None,
            }],
            contradicting: vec![],
            claims: vec![],
        }],
        proposed_claims: vec![ProposedClaim {
            id: "c1".into(),
            subject: path.into(),
            statement: "order.service.ts is in the candidate set".into(),
            kind: "structural".into(),
            evidence_refs: vec![EvidenceRef {
                kind: "file".into(),
                id: path.into(),
                summary: "core candidate".into(),
                timestamp: None,
            }],
            method: "fake".into(),
            temporal_scope: "".into(),
            limitations: vec![],
            status: ClaimStatus::Unresolved,
        }],
        requested_subjects: vec![],
        questions: vec![],
        explanation: "Test explanation".into(),
    });

    let mut opts = options_from_question("order timeout");
    opts.seed_files = vec![path.into()];
    opts.max_rounds = 1;
    let result =
        run_reasoning_investigation(opts, "/repo", &s, Some(&fake as _)).unwrap();
    assert_eq!(result.mode, "local_ai");
    assert_eq!(result.model.as_deref(), Some("fake-local"));
    assert!(!result.hypotheses.is_empty());
    assert_eq!(result.claims[0].status, ClaimStatus::Supported);
    assert_eq!(result.explanation.as_deref(), Some("Test explanation"));
    assert_eq!(result.rounds.len(), 1);
    assert!(result.rounds[0].ai_invoked);
}

#[test]
fn chronology_orders_events() {
    let s = store();
    seed_order_repo(&s, "/repo");
    let path = "src/modules/core/services/order.service.ts";
    let packet = build_evidence_packet(
        &PacketOptions {
            question: "order".into(),
            anchors: vec![path.into()],
            seed_files: vec![path.into()],
            max_rounds_hint: 1,
        },
        "/repo",
        &s,
    )
    .unwrap();
    assert!(
        !packet.chronology.is_empty(),
        "expected commit chronology for seeded order.service"
    );
    for w in packet.chronology.windows(2) {
        assert!(w[0].timestamp <= w[1].timestamp);
    }
}

/// Sacred C4-ER regression: "orders timeout" + Redis file/issue must never be SUPPORTED
/// as a causal explanation. Existence ≠ causal support.
#[test]
fn sacred_orders_timeout_redis_not_supported() {
    let s = store();
    seed_order_repo(&s, "/repo");
    // Ingest Redis-related documentary noise (mirrors RWATP issue #19 pattern).
    let issue = atlas_ir::Issue {
        number: 19,
        title: "Configure Redis Command Timeout to Prevent Grey Failures".into(),
        state: "closed".into(),
        body: Some("redis command timeout".into()),
        author: "dev".into(),
        created_at: Some(DateTime::<Utc>::from_timestamp(1500, 0).unwrap()),
    };
    s.insert_issue(&issue, "/repo").unwrap();

    let redis_path =
        "src/infrastructure/rate-limiting/implementations/redis-rate-limiter.ts";
    let order_path = "src/modules/core/services/order.service.ts";
    commit(
        &s,
        "/repo",
        "ddd4444",
        1600,
        &[redis_path],
        "feat: redis command timeout",
    );

    let packet = build_evidence_packet(
        &PacketOptions {
            question: "orders timeout".into(),
            anchors: vec!["orders".into(), "timeout".into(), "redis".into()],
            seed_files: vec![order_path.into(), redis_path.into()],
            max_rounds_hint: 1,
        },
        "/repo",
        &s,
    )
    .unwrap();

    let causal_claims = vec![
        ProposedClaim {
            id: "redis-cause".into(),
            subject: redis_path.into(),
            statement: "The Redis command timeout is configured to prevent grey failures during order processing."
                .into(),
            kind: "structural".into(),
            evidence_refs: vec![
                EvidenceRef {
                    kind: "file".into(),
                    id: redis_path.into(),
                    summary: "rate limiting".into(),
                    timestamp: None,
                },
                EvidenceRef {
                    kind: "issue".into(),
                    id: "#19".into(),
                    summary: "Redis timeouts".into(),
                    timestamp: None,
                },
            ],
            method: "static code analysis and issue review".into(),
            temporal_scope: "".into(),
            limitations: vec![],
            status: ClaimStatus::Unresolved,
        },
        ProposedClaim {
            id: "hyp-style".into(),
            subject: "timeout".into(),
            statement: "The timeout issue is related to the Redis command configuration.".into(),
            kind: "causal".into(),
            evidence_refs: vec![EvidenceRef {
                kind: "issue".into(),
                id: "issue#19".into(),
                summary: "Redis".into(),
                timestamp: None,
            }],
            method: "association".into(),
            temporal_scope: "".into(),
            limitations: vec![],
            status: ClaimStatus::Unresolved,
        },
    ];
    let v = verify_claims(&causal_claims, &packet);
    for c in &v {
        assert_ne!(
            c.status,
            ClaimStatus::Supported,
            "SACRED: claim '{}' must not be Supported (got {:?})",
            c.id,
            c.status
        );
    }

    // Non-causal structural presence remains Supported.
    let ok = verify_claims(
        &[ProposedClaim {
            id: "presence".into(),
            subject: order_path.into(),
            statement: "order.service.ts is among the core candidates for this investigation."
                .into(),
            kind: "structural".into(),
            evidence_refs: vec![EvidenceRef {
                kind: "file".into(),
                id: order_path.into(),
                summary: "candidate".into(),
                timestamp: None,
            }],
            method: "candidate list".into(),
            temporal_scope: "".into(),
            limitations: vec![],
            status: ClaimStatus::Unresolved,
        }],
        &packet,
    );
    assert_eq!(ok[0].status, ClaimStatus::Supported);
}

#[test]
fn json_contract_roundtrip_result() {
    let s = store();
    seed_order_repo(&s, "/repo");
    let mut opts = options_from_question("order");
    opts.no_ai = true;
    opts.seed_files = vec!["src/modules/core/services/order.service.ts".into()];
    let result = run_reasoning_investigation(opts, "/repo", &s, None).unwrap();
    let json = serde_json::to_string(&result).unwrap();
    let back: atlas_ir::ReasoningInvestigationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.schema_version, result.schema_version);
    assert_eq!(back.question, result.question);
    assert_eq!(back.mode, "deterministic_only");
}

#[test]
fn reasoning_options_struct_defaults() {
    let o = ReasoningOptions {
        question: "q".into(),
        anchors: vec!["a".into()],
        seed_files: vec![],
        max_rounds: 2,
        no_ai: true,
    };
    assert!(o.no_ai);
}
