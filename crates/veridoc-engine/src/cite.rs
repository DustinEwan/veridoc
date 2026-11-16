//! The citation channel: every `because` claim's citation goes to the plugin in one `cite`
//! call. The deterministic half is that the reference resolves; the half where the cited
//! text supports the reason is the same S2 comparison the judge already makes, so this
//! module takes a judge callback rather than a model client.

use veridoc_protocol::{CiteResult, Citation};

use crate::bind::{BindError, BoundChannel, BoundClaim};
use crate::execute::Outcome;
use crate::plugin::LostReason;
use crate::result::Warning;

/// A citation state the engine treats as settled. The vocabulary is the engine's because the
/// plugin answers with a bare string; anything outside it warns rather than blocks
/// (`docs/spec.md:480` puts the open-ticket case in the warning table).
const TERMINAL_STATES: &[&str] = &[
    "done",
    "closed",
    "resolved",
    "shipped",
    "cancelled",
    "canceled",
    "rejected",
    "merged",
];

pub fn is_terminal_state(state: &str) -> bool {
    TERMINAL_STATES.contains(&state.to_lowercase().as_str())
}

#[derive(Debug, Clone)]
pub struct Cited {
    pub claim_id: String,
    pub outcome: Outcome,
    /// True when the cited text could not be read: the claim stays supported, and this flag
    /// is the single occupant of the Residue metric.
    pub residue: bool,
}

#[derive(Debug, Default)]
pub struct CiteOutcome {
    pub cited: Vec<Cited>,
    /// Unresolved citations: findings that block the run with exit 1, like bind errors.
    pub errors: Vec<BindError>,
    pub warnings: Vec<Warning>,
}

/// Cite every `because` claim in one call. A transport error or `LostReason` is a run
/// failure — `docs/spec.md:404` refuses a soft pass — and `Err` carries its detail.
/// `judge` answers whether the cited text supports the claim's sentence, supplied by the
/// caller so the verdict cache and the model client stay outside this module.
pub fn cite_all(
    claims: &[BoundClaim],
    mut cite: impl FnMut(&[Citation]) -> Result<CiteResult, LostReason>,
    mut judge: impl FnMut(&BoundClaim, &str) -> Outcome,
) -> Result<CiteOutcome, String> {
    let citing: Vec<&BoundClaim> = claims
        .iter()
        .filter(|claim| matches!(claim.channel, BoundChannel::Citation { .. }))
        .collect();
    if citing.is_empty() {
        return Ok(CiteOutcome::default());
    }

    let requests: Vec<Citation> = citing
        .iter()
        .map(|claim| match &claim.channel {
            BoundChannel::Citation { store, id, turn } => Citation {
                store: store.clone(),
                id: id.clone(),
                turn: *turn,
            },
            _ => unreachable!("filtered to citation claims"),
        })
        .collect();

    let answer = match cite(&requests) {
        Ok(answer) => answer,
        Err(lost) => return Err(format!("citation store could not be reached: {lost}")),
    };

    let mut out = CiteOutcome::default();
    for claim in citing {
        let BoundChannel::Citation { store, id, .. } = &claim.channel else {
            unreachable!("filtered to citation claims");
        };
        if let Some(gap) = answer
            .unresolved
            .iter()
            .find(|gap| gap.id == *id || gap.id == format!("{store}:{id}"))
        {
            out.errors.push(BindError::CitationUnresolved {
                file: claim.file.clone(),
                line: claim.line,
                citation: format!("{store}:{id}"),
                reason: gap.reason.clone(),
            });
            continue;
        }
        let Some(resolved) = answer.resolved.iter().find(|r| r.id == *id) else {
            // The plugin answered neither resolved nor unresolved for this reference: it
            // could not be reached, which is a run failure, not a soft pass.
            return Err(format!(
                "citation store could not be reached: {store}:{id} was not answered"
            ));
        };

        if let Some(state) = &resolved.state {
            if !is_terminal_state(state) {
                out.warnings.push(Warning {
                    file: Some(claim.file.clone()),
                    line: Some(claim.line),
                    message: format!(
                        "citation {store}:{id} is in state {state}; the decision it justifies has shipped"
                    ),
                });
            }
        }

        if resolved.text.trim().is_empty() {
            out.cited.push(Cited {
                claim_id: claim.id.clone(),
                outcome: Outcome::Supported,
                residue: true,
            });
            out.warnings.push(Warning {
                    file: Some(claim.file.clone()),
                    line: Some(claim.line),
                message: format!(
                    "claim {} cites {store}:{id}, whose text could not be read",
                    claim.id
                ),
            });
            continue;
        }

        let outcome = judge(claim, &resolved.text);
        out.cited.push(Cited {
            claim_id: claim.id.clone(),
            outcome,
            residue: false,
        });
    }
    Ok(out)
}
