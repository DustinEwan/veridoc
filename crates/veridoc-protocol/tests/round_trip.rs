//! Every wire type must survive a serialize/deserialize round trip, and the shapes the
//! TypeScript plugin is written against must serialize verbatim as the spec's examples show.

use serde_json::{json, Value};
use veridoc_protocol::*;

#[test]
fn a_leaf_serializes_verbatim_as_the_spec_example() {
    let leaf = AssertLeaf {
        obs: "exit.code".into(),
        r#is: Check::Equals(Value::from(2)),
    };
    assert_eq!(
        serde_json::to_string(&leaf).unwrap(),
        r#"{"obs":"exit.code","is":{"equals":2}}"#
    );
}

#[test]
fn a_satisfies_check_serializes_as_an_object() {
    let check = Check::Satisfies(SatisfiesSpec {
        source: "(f) => f.length === 0".into(),
        accepts: Some(json!(["a"])),
        rejects: Some(json!([])),
    });
    let value = serde_json::to_value(&check).unwrap();
    assert_eq!(value["satisfies"]["source"], "(f) => f.length === 0");
    let back: Check = serde_json::from_value(value).unwrap();
    assert_eq!(back, check);
}

#[test]
fn asserts_composes_with_inline_leaves() {
    let tree: Asserts = serde_json::from_value(json!({
        "all": [
            { "obs": "exit.code", "is": { "equals": 2 } },
            { "not": { "obs": "stderr.text", "is": { "contains": "" } } },
            { "any": [{ "obs": "writtenFiles", "is": { "equals": [] } }] },
        ]
    }))
    .unwrap();
    assert_eq!(tree.leaves().len(), 3);
    assert_eq!(tree.leaves()[1].1.obs, "stderr.text");
    let round = serde_json::to_value(&tree).unwrap();
    assert_eq!(round, tree_clone_value(&tree));
}

fn tree_clone_value(tree: &Asserts) -> Value {
    serde_json::to_value(tree).unwrap()
}

#[test]
fn a_claim_round_trips_with_its_channel() {
    let claim = RawClaim {
        kind: Kind::Ensures,
        subject: RefToken::Symbol {
            name: "parseConfig".into(),
            from: "src/scanner.ts".into(),
        },
        concept: "project-scan".into(),
        predicate: "rejects-missing-project-root".into(),
        text: "A missing project root is refused.".into(),
        channel: Channel {
            scenario: Some("config/rejects-missing-root".into()),
            asserts: Some(Asserts::Leaf(AssertLeaf {
                obs: "exit.code".into(),
                r#is: Check::Equals(Value::from(2)),
            })),
            artefact: None,
            citation: None,
        },
        file: "src/scanner.doc.ts".into(),
        line: 3,
    };
    let value = serde_json::to_value(&claim).unwrap();
    assert_eq!(value["subject"]["kind"], "symbol");
    assert_eq!(value["channel"]["scenario"], "config/rejects-missing-root");
    assert_eq!(serde_json::from_value::<RawClaim>(value).unwrap(), claim);
    assert_eq!(claim.channel.populated(), 1);
    assert_eq!(claim.subject.key(), "src/scanner.ts|parseConfig");
    assert_eq!(
        RefToken::Concept {
            name: "project-scan".into()
        }
        .key(),
        "concept|project-scan"
    );
}

#[test]
fn channel_counts_its_populated_channels() {
    assert_eq!(Channel::default().populated(), 0);
    let scenario = Channel {
        scenario: Some("s".into()),
        asserts: Some(Asserts::Leaf(AssertLeaf {
            obs: "o".into(),
            r#is: Check::Equals(Value::from(1)),
        })),
        ..Channel::default()
    };
    assert_eq!(scenario.populated(), 1);
    let three = Channel {
        artefact: Some(Artefact {
            path: "a".into(),
            sha256: "h".into(),
        }),
        citation: Some(Citation {
            store: "session".into(),
            id: "01a1e512".into(),
            turn: Some(12),
        }),
        ..scenario
    };
    assert_eq!(three.populated(), 3);
}

#[test]
fn run_round_trips() {
    let req = RunRequest {
        scenario_id: "config/rejects-missing-root".into(),
        fixture: "empty-directory".into(),
        action: "run-probe-plugin".into(),
        args: vec![],
        deadline_ms: 30_000,
        leaves: vec![RunLeaf {
            leaf_id: "c000000000000001#l0".into(),
        }],
    };
    assert_eq!(
        serde_json::from_value::<RunRequest>(serde_json::to_value(&req).unwrap()).unwrap(),
        req
    );
    let resp = RunResponse {
        ok: true,
        observed: vec![Observed {
            observable: "exit.code".into(),
            value: "2".into(),
        }],
        satisfied: vec![LeafAnswer {
            leaf_id: "c000000000000001#l0".into(),
            ok: true,
        }],
        failure: None,
    };
    let value = serde_json::to_value(&resp).unwrap();
    assert!(
        value.get("failure").is_none(),
        "failure is omitted, not null"
    );
    assert_eq!(serde_json::from_value::<RunResponse>(value).unwrap(), resp);
}

#[test]
fn collect_round_trips() {
    let result = CollectResult {
        concepts: vec!["project-scan".into()],
        documents: vec![DocumentDecl {
            path: "docs/architecture.spec.md".into(),
            audience: "an author".into(),
            question: "What must my plugin answer?".into(),
            not_applicable: None,
        }],
        claims: vec![],
        observables: vec![ObservableDecl {
            name: "exit.code".into(),
            ty: "number".into(),
        }],
        actions: vec![ActionDecl {
            name: "run-probe-plugin".into(),
            calls: RefToken::Symbol {
                name: "probe".into(),
                from: "src/cli.ts".into(),
            },
            produces: vec!["exit.code".into()],
        }],
        fixtures: vec!["empty-directory".into()],
        skipped: vec![Skipped {
            file: "src/broken.doc.ts".into(),
            reason: "import threw".into(),
        }],
    };
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(value["observables"][0]["type"], "number");
    assert_eq!(
        serde_json::from_value::<CollectResult>(value).unwrap(),
        result
    );
}

#[test]
fn handshake_and_surface_round_trip() {
    let hs = Handshake {
        protocol_version: PROTOCOL_VERSION.into(),
        name: "veridoc-fakeplugin".into(),
        version: "0.1.0".into(),
        languages: vec!["typescript".into()],
        capabilities: vec![Capability::Collect, Capability::Resolution],
    };
    assert_eq!(
        serde_json::from_value::<Handshake>(serde_json::to_value(&hs).unwrap()).unwrap(),
        hs
    );
    let surface = SurfaceResult {
        referents: vec![SurfaceReferent {
            referent_id: "veridoc:referent:abc".into(),
            name: "parseConfig".into(),
            public: true,
        }],
    };
    assert_eq!(
        serde_json::from_value::<SurfaceResult>(serde_json::to_value(&surface).unwrap()).unwrap(),
        surface
    );
}

#[test]
fn resolve_derivable_and_cite_round_trip() {
    let r = ResolveResult {
        resolved: vec![ResolvedRef {
            reference: "src/scanner.ts|parseConfig".into(),
            referent_id: "veridoc:referent:abc".into(),
            signature: "function parseConfig(root: string): ProjectConfig".into(),
        }],
        unresolved: vec![UnresolvedRef {
            reference: "src/scanner.ts|gone".into(),
            reason: "no exported member 'gone'".into(),
        }],
    };
    let value = serde_json::to_value(&r).unwrap();
    assert_eq!(value["resolved"][0]["ref"], "src/scanner.ts|parseConfig");
    assert_eq!(serde_json::from_value::<ResolveResult>(value).unwrap(), r);
    let d = DerivableResult {
        facts: vec!["parseConfig takes one parameter named root.".into()],
    };
    assert_eq!(
        serde_json::from_value::<DerivableResult>(serde_json::to_value(&d).unwrap()).unwrap(),
        d
    );
    let c = CiteResult {
        resolved: vec![ResolvedCitation {
            id: "01a1e512".into(),
            text: "we walk upward".into(),
            state: Some("merged".into()),
        }],
        unresolved: vec![UnresolvedCitation {
            id: "ffff".into(),
            reason: "object not found".into(),
        }],
    };
    assert_eq!(
        serde_json::from_value::<CiteResult>(serde_json::to_value(&c).unwrap()).unwrap(),
        c
    );
}

#[test]
fn bivalence_carries_the_witness_answers() {
    let b = BivalenceResult {
        bivalent: true,
        method: BivalenceMethod::Witnesses,
        witness_accepts: Some(true),
        witness_rejects: Some(true),
        reason: None,
    };
    let value = serde_json::to_value(&b).unwrap();
    assert_eq!(value["method"], "witnesses");
    assert_eq!(serde_json::from_value::<BivalenceResult>(value).unwrap(), b);
    let static_value = serde_json::to_value(BivalenceMethod::StaticAnalysis).unwrap();
    assert_eq!(static_value, "static-analysis");
}

#[test]
fn verdicts_round_trip() {
    let s1 = S1Verdict {
        unmentioned_claims: vec!["c1a2b3c4d5e6f7a8".into()],
        unstated_assertions: vec![Unstated {
            span: "sp2".into(),
            text: "the scanner caches".into(),
        }],
    };
    assert_eq!(
        serde_json::from_value::<S1Verdict>(serde_json::to_value(&s1).unwrap()).unwrap(),
        s1
    );
    let s2 = S2Verdict {
        pairs: vec![S2Pair {
            span_a: "sp1".into(),
            span_b: "sp2".into(),
            claim_id: "c1a2b3c4d5e6f7a8".into(),
        }],
    };
    let value = serde_json::to_value(&s2).unwrap();
    assert_eq!(value["pairs"][0]["spanA"], "sp1");
    assert_eq!(serde_json::from_value::<S2Verdict>(value).unwrap(), s2);
    let s3 = S3Verdict {
        pairs: vec![S3Pair {
            node_a: "docs/a.spec.md".into(),
            node_b: "docs/b.spec.md".into(),
        }],
    };
    assert_eq!(
        serde_json::from_value::<S3Verdict>(serde_json::to_value(&s3).unwrap()).unwrap(),
        s3
    );
}

#[test]
fn every_capability_wire_spelling_is_a_single_lowercase_word() {
    for cap in [
        Capability::Collect,
        Capability::Resolution,
        Capability::Derivability,
        Capability::Bivalence,
        Capability::Execution,
        Capability::Citation,
        Capability::Surface,
    ] {
        let wire = serde_json::to_value(cap).unwrap();
        let word = wire.as_str().expect("a capability is a string");
        assert!(
            !word.is_empty() && word.chars().all(|c| c.is_ascii_lowercase()) && !word.contains('_'),
            "capability wire spelling {word:?} is not a single lowercase word"
        );
    }
}

#[test]
fn request_omits_params_for_methods_without_them() {
    let req = Request::empty(7, Method::Handshake);
    let line = serde_json::to_string(&req).unwrap();
    assert!(
        !line.contains("params"),
        "params is omitted, not null: {line}"
    );
    let back = parse_request(&line).unwrap();
    assert_eq!(back, req);
}
