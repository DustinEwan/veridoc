//! Execution: the engine runs each scenario-channel claim's scenario and evaluates the
//! assertion tree itself. The plugin answers values and never a verdict, and a claim's result has
//! three members: the tree held, the tree did not hold, or the scenario never ran
//! (`docs/decisions/0006-a-project-adds-three-files-and-the-scenario-belongs-to-the-claim.md`).

use std::time::Duration;

use crate::bind::{BoundChannel, BoundClaim};
use serde_json::Value;
use veridoc_protocol::{
    Asserts, Call, Check, LeafAnswer, Loss, RunFailureKind, RunRequest, RunResponse, Timeouts,
};

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

/// One scenario-channel claim after its scenario ran, or after it failed to run.
#[derive(Debug, Clone)]
pub enum Executed {
    /// The scenario ran, and the engine evaluated the tree against the values it answered.
    Evaluated {
        claim_id: String,
        outcome: Outcome,
        /// For a contradicted claim: every value in tree order, rendered `{observable} = {value}`;
        /// a leaf that read a value the run did not answer shows `(absent)`.
        observed: Vec<String>,
    },
    /// The scenario did not run. That is this claim's result and nothing stronger — never
    /// `supported`, never `contradicted` — and the run goes on so that one typo costs no other
    /// claim its verdict.
    RunFailed {
        claim_id: String,
        kind: RunFailureKind,
        detail: String,
    },
}

/// The four ways a run answers nothing, spelled the way the report spells them.
pub fn kind_word(kind: RunFailureKind) -> &'static str {
    match kind {
        RunFailureKind::Build => "build",
        RunFailureKind::Fixture => "fixture",
        RunFailureKind::Plugin => "plugin",
        RunFailureKind::Deadline => "deadline",
    }
}

/// Run every scenario-channel claim, sequentially, and evaluate each tree. `run` is the plugin
/// call supplied by the caller, given the request and this engine's own wait for it; a `run` that
/// is refused, is lost, outlives its deadline, or answers `ok: false` is recorded as that claim's
/// `run-failed` result and the loop moves on. `deadline_ms` is the engine's wait: the engine owns
/// the kill timer, and the plugin's `timeoutMs` is derived from it rather than being a second
/// number written by hand (spec §9).
pub fn execute(
    claims: &[BoundClaim],
    deadline_ms: u64,
    mut run: impl FnMut(&RunRequest, Duration) -> Call<RunResponse>,
) -> Vec<Executed> {
    let host_timeout = Duration::from_millis(deadline_ms);
    let plugin_timeout = Timeouts::plugin_deadline(host_timeout);
    let mut executed = Vec::new();

    for claim in claims {
        let BoundChannel::Scenario { asserts, .. } = &claim.channel else {
            continue;
        };

        // The claim is the only address: the scenario belongs to it, and the leaf ids the plugin
        // answers are the pre-order walk of the assertion tree the plugin itself declared.
        let request = RunRequest {
            claim: claim.id.clone(),
            timeout_ms: plugin_timeout,
        };

        let response = match run(&request, host_timeout) {
            Call::Result(response) => response,
            // A scenario that outlived its deadline is a deadline failure; the prose is not
            // blamed on a slow machine (`docs/spec.md:354`). Only a loss says nothing was
            // answered. A refusal is an answer — the plugin is alive — so it is the plugin's own
            // fault, kind `plugin`, and must not be read as a deadline.
            Call::Lost(Loss::NoAnswer) => {
                executed.push(ran_nothing(
                    claim,
                    RunFailureKind::Deadline,
                    format!("claim {} outlived {deadline_ms} ms", claim.id),
                ));
                continue;
            }
            Call::Refusal(error) => {
                executed.push(ran_nothing(
                    claim,
                    RunFailureKind::Plugin,
                    format!("the plugin refused run ({}): {}", error.code, error.message),
                ));
                continue;
            }
            Call::Lost(lost) => {
                executed.push(ran_nothing(claim, RunFailureKind::Plugin, lost.to_string()));
                continue;
            }
        };
        if !response.ok {
            // The plugin said the scenario did not run, and named the kind of nothing that
            // happened; its own words travel with the claim's result.
            let (kind, detail) = match response.failure {
                Some(failure) => (failure.kind, failure.detail),
                None => (
                    RunFailureKind::Plugin,
                    "the plugin answered ok: false with no failure".to_string(),
                ),
            };
            executed.push(ran_nothing(claim, kind, detail));
            continue;
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
            // A `satisfies` leaf the plugin skipped is not a silent false, and not a verdict.
            executed.push(ran_nothing(
                claim,
                RunFailureKind::Plugin,
                format!("the plugin answered no result for leaf {leaf_id}"),
            ));
            continue;
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
        executed.push(Executed::Evaluated {
            claim_id: claim.id.clone(),
            outcome,
            observed: observed_lines,
        });
    }

    executed
}

/// One claim's result when its scenario did not run: the kind, and the plugin's own words.
fn ran_nothing(claim: &BoundClaim, kind: RunFailureKind, detail: String) -> Executed {
    Executed::RunFailed {
        claim_id: claim.id.clone(),
        kind,
        detail,
    }
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
                holds &= evaluate(
                    child,
                    claim_id,
                    observed,
                    satisfied,
                    next_leaf,
                    missing_leaf,
                );
            }
            holds
        }
        Asserts::Any { any } => {
            let mut holds = false;
            for child in any {
                holds |= evaluate(
                    child,
                    claim_id,
                    observed,
                    satisfied,
                    next_leaf,
                    missing_leaf,
                );
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
