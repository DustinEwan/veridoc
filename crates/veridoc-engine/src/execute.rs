//! Execution: the engine runs each scenario-channel claim's scenario and evaluates the
//! assertion tree itself. The plugin answers values and never a verdict, and the outcome has
//! exactly two members — a claim is either certified or the run is red (`docs/spec.md:356`).

use std::time::Duration;

use serde_json::Value;
use veridoc_protocol::{
    Asserts, Call, Check, LeafAnswer, Loss, RunFailure, RunFailureKind, RunLeaf, RunRequest,
    RunResponse, Timeouts,
};
use crate::bind::{BoundChannel, BoundClaim};

/// The two readings a contradiction cannot separate (`docs/spec.md:362-366`). Both travel
/// with every contradicted claim; a person or an agent chooses between them.
pub const READING_CODE_WRONG: &str = "the code is wrong and the sentence is right";
pub const READING_SCENARIO_WRONG: &str =
    "the scenario or the assertion is wrong and the code is right";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Supported,
    Contradicted,
}

/// One scenario-channel claim after its scenario ran.
#[derive(Debug, Clone)]
pub struct Executed {
    pub claim_id: String,
    pub outcome: Outcome,
    /// For a contradicted claim: every observed value in tree order, rendered
    /// `{observable} = {value}`; a leaf that read an absent observable shows `(absent)`.
    pub observed: Vec<String>,
}

/// Run every scenario-channel claim, sequentially, and evaluate each tree. `run` is the
/// plugin call supplied by the caller, given the request and this engine's own wait for it;
/// any `Err(RunFailure)` stops the whole run, because a run failure is never a claim state.
/// `deadline_ms` is that wait: the engine owns the kill timer, and the plugin's `deadlineMs` is
/// derived from it rather than being a second number written by hand (spec §9).
pub fn execute(
    claims: &[BoundClaim],
    deadline_ms: u64,
    mut run: impl FnMut(&RunRequest, Duration) -> Call<RunResponse>,
) -> Result<Vec<Executed>, RunFailure> {
    let host_timeout = Duration::from_millis(deadline_ms);
    let plugin_deadline = Timeouts::plugin_deadline(host_timeout);
    let mut executed = Vec::new();

    for claim in claims {
        let BoundChannel::Scenario {
            scenario_id,
            fixture,
            action,
            args,
            asserts,
        } = &claim.channel
        else {
            continue;
        };

        // Leaf ids are assigned by a pre-order walk, so the ids are stable for a given tree.
        let leaves: Vec<RunLeaf> = asserts
            .leaves()
            .into_iter()
            .map(|(index, _)| RunLeaf {
                leaf_id: format!("{}#l{index}", claim.id),
            })
            .collect();
        let request = RunRequest {
            scenario_id: scenario_id.clone(),
            fixture: fixture.clone(),
            action: action.clone(),
            args: args.clone(),
            deadline_ms: plugin_deadline,
            leaves,
        };

        let response = match run(&request, host_timeout) {
            Call::Result(response) => response,
            // A scenario that outlived its deadline is red with kind `deadline`; the prose
            // is not blamed on a slow machine (`docs/spec.md:354`). Only a loss says nothing
            // was answered. A refusal is an answer — the plugin is alive — so it is the
            // plugin's own fault, kind `plugin`, and must not be read as a deadline.
            Call::Lost(Loss::NoAnswer) => {
                return Err(RunFailure {
                    kind: RunFailureKind::Deadline,
                    detail: format!("scenario {scenario_id} outlived {deadline_ms} ms"),
                });
            }
            Call::Refusal(error) => {
                return Err(RunFailure {
                    kind: RunFailureKind::Plugin,
                    detail: format!(
                        "scenario {scenario_id}: the plugin refused run ({}): {}",
                        error.code, error.message
                    ),
                });
            }
            Call::Lost(lost) => {
                return Err(RunFailure {
                    kind: RunFailureKind::Plugin,
                    detail: format!("scenario {scenario_id}: {lost}"),
                });
            }
        };
        if !response.ok {
            let failure = response.failure.unwrap_or(RunFailure {
                kind: RunFailureKind::Plugin,
                detail: "the plugin answered ok: false with no failure".to_string(),
            });
            return Err(RunFailure {
                kind: failure.kind,
                detail: format!("scenario {scenario_id}: {}", failure.detail),
            });
        }

        let observed: Vec<(String, Value)> = response
            .observed
            .iter()
            .map(|o| (o.observable.clone(), decode(&o.value)))
            .collect();
        let satisfied: Vec<LeafAnswer> = response.satisfied.clone();

        let mut missing_leaf: Option<String> = None;
        let mut next_leaf = 0usize;
        let holds = evaluate(
            asserts,
            &claim.id,
            &observed,
            &satisfied,
            &mut next_leaf,
            &mut missing_leaf,
        );
        if let Some(leaf_id) = missing_leaf {
            // A `satisfies` leaf the plugin skipped is not a silent false.
            return Err(RunFailure {
                kind: RunFailureKind::Plugin,
                detail: format!("plugin answered no result for leaf {leaf_id}"),
            });
        }

        let outcome = if holds {
            Outcome::Supported
        } else {
            Outcome::Contradicted
        };
        let observed_lines = if holds {
            Vec::new()
        } else {
            rendering(asserts, &observed)
        };
        executed.push(Executed {
            claim_id: claim.id.clone(),
            outcome,
            observed: observed_lines,
        });
    }

    Ok(executed)
}

/// The plugin JSON-encodes each observed value, so `"2"` is the number 2 and an
/// unparseable answer stays the raw string.
fn decode(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}

fn evaluate(
    node: &Asserts,
    claim_id: &str,
    observed: &[(String, Value)],
    satisfied: &[LeafAnswer],
    next_leaf: &mut usize,
    missing_leaf: &mut Option<String>,
) -> bool {
    // No short-circuiting: every leaf consumes its pre-order index and gets its chance to
    // report a missing `satisfies` answer, whichever way the composites fall.
    match node {
        Asserts::All { all } => {
            let mut holds = true;
            for child in all {
                holds &= evaluate(child, claim_id, observed, satisfied, next_leaf, missing_leaf);
            }
            holds
        }
        Asserts::Any { any } => {
            let mut holds = false;
            for child in any {
                holds |= evaluate(child, claim_id, observed, satisfied, next_leaf, missing_leaf);
            }
            holds
        }
        Asserts::Not { not } => {
            !evaluate(not, claim_id, observed, satisfied, next_leaf, missing_leaf)
        }
        Asserts::Leaf(leaf) => match &leaf.r#is {
            Check::Equals(expected) => {
                *next_leaf += 1;
                observed
                    .iter()
                    .find(|(name, _)| name == &leaf.obs)
                    .is_some_and(|(_, value)| value == expected)
            }
            Check::Contains(needle) => {
                *next_leaf += 1;
                observed
                    .iter()
                    .find(|(name, _)| name == &leaf.obs)
                    .is_some_and(|(_, value)| contains(value, needle))
            }
            Check::Satisfies(_) => {
                let leaf_id = format!("{claim_id}#l{next_leaf}");
                *next_leaf += 1;
                match satisfied.iter().find(|answer| answer.leaf_id == leaf_id) {
                    Some(answer) => answer.ok,
                    None => {
                        if missing_leaf.is_none() {
                            *missing_leaf = Some(leaf_id);
                        }
                        false
                    }
                }
            }
        },
    }
}

/// `contains` over a string is substring; over an array it is element membership; over
/// anything else it cannot hold (bind already refused other declared types).
fn contains(value: &Value, needle: &Value) -> bool {
    match value {
        Value::String(haystack) => match needle {
            Value::String(needle) => haystack.contains(needle.as_str()),
            needle => haystack.contains(&needle.to_string()),
        },
        Value::Array(items) => items.iter().any(|item| item == needle),
        _ => false,
    }
}

/// Every leaf's value in tree order, for the contradiction diagnostic.
fn rendering(node: &Asserts, observed: &[(String, Value)]) -> Vec<String> {
    let mut out = Vec::new();
    for (_, leaf) in node.leaves() {
        let value = observed
            .iter()
            .find(|(name, _)| name == &leaf.obs)
            .map(|(_, value)| match value {
                Value::String(text) => text.clone(),
                value => value.to_string(),
            })
            .unwrap_or_else(|| "(absent)".to_string());
        out.push(format!("{} = {value}", leaf.obs));
    }
    out
}
