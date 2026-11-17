//! The bivalence gate: every scenario-channel claim is classified before anything runs,
//! because an assertion that cannot fail is not evidence (`docs/spec.md:212-214`). Routing
//! is `docs/decisions/0005` made executable: provably bivalent claims keep the scenario
//! channel, claims the plugin cannot settle go to the judge, and claims that are neither
//! error. There is no third outcome.

use veridoc_protocol::{Asserts, BivalenceMethod, BivalenceResult, Call, Capability, Check};

use crate::bind::{BindError, BoundChannel, BoundClaim};
use crate::plugin::sentence;

#[derive(Debug)]
pub struct Outcome {
    /// Every claim with its channel settled: scenario claims either kept the scenario channel
    /// or were routed to the judge channel.
    pub claims: Vec<BoundClaim>,
    /// Claims that may not take the scenario channel and were not routed to the judge.
    pub errors: Vec<BindError>,
}

/// Classify every scenario-channel claim. `bivalence` is the plugin call, supplied by the
/// caller. A refusal or a loss is a run failure — never a claim state — so the caller sees
/// `Err` and stops the run with kind `plugin`; `sentence` words the two differently, because a
/// refusal means the plugin is alive and a loss means it is not.
pub fn gate(
    claims: Vec<BoundClaim>,
    plugin_name: &str,
    capabilities: &[Capability],
    mut bivalence: impl FnMut(&str) -> Call<BivalenceResult>,
) -> Result<Outcome, String> {
    let mut errors: Vec<BindError> = Vec::new();
    let mut out = Vec::new();

    for mut claim in claims {
        let BoundChannel::Scenario { asserts, .. } = &claim.channel else {
            out.push(claim);
            continue;
        };

        if !capabilities.contains(&Capability::Bivalence) {
            errors.push(BindError::CapabilityMissing {
                file: claim.file.clone(),
                line: claim.line,
                claim_id: claim.id.clone(),
                name: plugin_name.to_string(),
                capability: "bivalence".to_string(),
            });
            continue;
        }

        let answer = sentence("bivalence", bivalence(&claim.id))
            .map_err(|why| format!("bivalence of claim {}: {why}", claim.id))?;

        if !answer.bivalent {
            // Not bivalent is not a failure: the sentence is still worth judging, it just
            // cannot be certified by running.
            claim.channel = BoundChannel::Judge;
            out.push(claim);
            continue;
        }

        if answer.method == BivalenceMethod::Witnesses {
            let settled =
                answer.witness_accepts == Some(true) && answer.witness_rejects == Some(true);
            if !settled {
                errors.push(BindError::WitnessPair {
                    file: claim.file.clone(),
                    line: claim.line,
                    claim_id: claim.id.clone(),
                    reason: answer
                        .reason
                        .clone()
                        .unwrap_or_else(|| "the plugin answered neither witness value".to_string()),
                });
                continue;
            }
            // The plugin could not settle the question by analysis, so every `satisfies`
            // leaf carries the author's own witness pair (`docs/spec.md:219-220`).
            let mut missing = Vec::new();
            witness_gaps(asserts, String::new(), &mut missing);
            if let Some(path) = missing.into_iter().next() {
                errors.push(BindError::WitnessLeaf {
                    file: claim.file.clone(),
                    line: claim.line,
                    claim_id: claim.id.clone(),
                    path,
                });
                continue;
            }
        }

        out.push(claim);
    }

    Ok(Outcome {
        claims: out,
        errors,
    })
}

/// Paths of `satisfies` leaves missing either half of the witness pair, in pre-order.
fn witness_gaps(node: &Asserts, path: String, out: &mut Vec<String>) {
    match node {
        Asserts::All { all } => {
            for (i, child) in all.iter().enumerate() {
                witness_gaps(child, format!("{path}all[{i}]"), out);
            }
        }
        Asserts::Any { any } => {
            for (i, child) in any.iter().enumerate() {
                witness_gaps(child, format!("{path}any[{i}]"), out);
            }
        }
        Asserts::Not { not } => witness_gaps(not, format!("{path}not"), out),
        Asserts::Leaf(leaf) => {
            if let Check::Satisfies(spec) = &leaf.r#is {
                if spec.accepts.is_none() || spec.rejects.is_none() {
                    out.push(if path.is_empty() {
                        "asserts".to_string()
                    } else {
                        path
                    });
                }
            }
        }
    }
}
