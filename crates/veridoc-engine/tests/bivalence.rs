//! The bivalence gate: `bivalent: false` routes to the judge channel, a `witnesses` method
//! without the accepted-and-rejecting pair is a bind error, and a plugin error is a run
//! failure that leaves the claim in no state at all.

use veridoc_engine::bind::{BindError, BoundChannel, BoundClaim};
use veridoc_engine::bivalence::{self, Outcome};
use veridoc_protocol::{AssertLeaf, Asserts, BivalenceMethod, BivalenceResult, Capability, Check};
use veridoc_protocol::{Call, Loss};

const ALL_CAPS: &[Capability] = &[
    Capability::Collect,
    Capability::Resolution,
    Capability::Derivability,
    Capability::Bivalence,
    Capability::Execution,
    Capability::Citation,
    Capability::Surface,
];

fn scenario_claim() -> BoundClaim {
    BoundClaim {
        id: "c8223c485f101870d".into(),
        kind: veridoc_protocol::Kind::Ensures,
        concept: "project-scan".into(),
        predicate: "rejects-missing-project-root".into(),
        text: "A missing project root is refused.".into(),
        file: "src/cli.doc.ts".into(),
        line: 3,
        subject: "probe".into(),
        referent_id: "veridoc:referent:probe".into(),
        signature: "function probe(dir: string): number".into(),
        channel: BoundChannel::Scenario {
            scenario_id: "config/rejects-missing-root".into(),
            fixture: "empty-directory".into(),
            action: "run-probe".into(),
            args: vec![],
            asserts: Asserts::Leaf(AssertLeaf {
                obs: "exit.code".into(),
                r#is: Check::Equals(serde_json::json!(2)),
            }),
        },
    }
}

fn run(caps: &[Capability], answer: Call<BivalenceResult>) -> Result<Outcome, String> {
    bivalence::gate(
        vec![scenario_claim()],
        "veridoc-fakeplugin",
        caps,
        move |_| answer.clone(),
    )
}

#[test]
fn a_structurally_bivalent_claim_keeps_the_scenario_channel() {
    let outcome = run(
        ALL_CAPS,
        Call::Result(BivalenceResult {
            bivalent: true,
            method: BivalenceMethod::Structural,
            witness_accepts: None,
            witness_rejects: None,
            reason: None,
        }),
    )
    .expect("gate");
    assert!(outcome.errors.is_empty());
    assert!(matches!(
        outcome.claims[0].channel,
        BoundChannel::Scenario { .. }
    ));
}

#[test]
fn a_non_bivalent_claim_is_routed_to_the_judge_channel() {
    let outcome = run(
        ALL_CAPS,
        Call::Result(BivalenceResult {
            bivalent: false,
            method: BivalenceMethod::StaticAnalysis,
            witness_accepts: None,
            witness_rejects: None,
            reason: Some("the predicate never returns false".into()),
        }),
    )
    .expect("gate");
    assert!(outcome.errors.is_empty());
    assert!(matches!(outcome.claims[0].channel, BoundChannel::Judge));
}

#[test]
fn witnesses_without_the_pair_is_a_bind_error_naming_the_reason() {
    let outcome = run(
        ALL_CAPS,
        Call::Result(BivalenceResult {
            bivalent: true,
            method: BivalenceMethod::Witnesses,
            witness_accepts: Some(true),
            witness_rejects: None,
            reason: Some("no rejecting value was supplied".into()),
        }),
    )
    .expect("gate");
    assert!(
        outcome.claims.is_empty(),
        "an errored claim is settled in no channel"
    );
    let BindError::WitnessPair { reason, .. } = &outcome.errors[0] else {
        panic!("expected WitnessPair, got {:?}", outcome.errors[0]);
    };
    assert_eq!(reason, "no rejecting value was supplied");
    assert_eq!(
        outcome.errors[0].diagnostic(),
        "src/cli.doc.ts:3: claim c8223c485f101870d is not bivalent by witnesses: no rejecting value was supplied"
    );
}

#[test]
fn witnesses_require_the_pair_on_every_satisfies_leaf() {
    let mut claim = scenario_claim();
    let BoundChannel::Scenario { asserts, .. } = &mut claim.channel else {
        unreachable!()
    };
    *asserts = Asserts::All {
        all: vec![Asserts::Leaf(AssertLeaf {
            obs: "writtenFiles".into(),
            r#is: Check::Satisfies(veridoc_protocol::SatisfiesSpec {
                source: "f => f.length > 0".into(),
                accepts: None,
                rejects: None,
            }),
        })],
    };
    let outcome = bivalence::gate(vec![claim], "p", ALL_CAPS, |_| {
        Call::Result(BivalenceResult {
            bivalent: true,
            method: BivalenceMethod::Witnesses,
            witness_accepts: Some(true),
            witness_rejects: Some(true),
            reason: None,
        })
    })
    .expect("gate");
    assert_eq!(
        outcome.errors[0].diagnostic(),
        "src/cli.doc.ts:3: claim c8223c485f101870d has a satisfies leaf without the witness pair at all[0]"
    );
}

#[test]
fn a_plugin_error_is_a_run_failure_and_leaves_the_claim_unsettled() {
    let result = run(ALL_CAPS, Call::Lost(Loss::NoAnswer));
    match result {
        Err(detail) => assert!(detail.contains("bivalence of claim")),
        Ok(_) => panic!("a plugin error must be a run failure"),
    }
}

#[test]
fn no_bivalence_capability_makes_the_scenario_claim_an_error() {
    let outcome = run(
        &[
            Capability::Collect,
            Capability::Resolution,
            Capability::Execution,
        ],
        Call::Result(BivalenceResult {
            bivalent: true,
            method: BivalenceMethod::Structural,
            witness_accepts: None,
            witness_rejects: None,
            reason: None,
        }),
    )
    .expect("gate");
    assert_eq!(
        outcome.errors[0].diagnostic(),
        "src/cli.doc.ts:3: claim c8223c485f101870d cannot take the scenario channel: plugin \"p\" declares no bivalence"
            .replace("plugin \"p\"", "plugin \"veridoc-fakeplugin\"")
    );
    assert!(outcome.claims.is_empty());
}
