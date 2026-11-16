//! The derivability ban: a claim whose text a reader can already read off the declaration is
//! rejected as redundant. The v1 rule is exact-equality after normalization, deliberately
//! narrow — `docs/spec.md:160-163` says a checker that over-rejects teaches the author to
//! disable it, so containment is never tried.

use veridoc_protocol::Capability;

use crate::bind::BoundClaim;
use crate::plugin::LostReason;

/// Lowercase, drop backticks, drop ASCII punctuation, collapse runs of whitespace, trim.
pub fn normalize(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| *c != '`' && !c.is_ascii_punctuation())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// One claim the ban rejected, with the fact that made it redundant.
#[derive(Debug, Clone)]
pub struct Rejected {
    pub claim_id: String,
    pub file: String,
    pub line: u32,
    pub fact: String,
}

#[derive(Debug)]
pub struct Outcome {
    /// The claims the ban kept, in input order.
    pub kept: Vec<BoundClaim>,
    pub rejected: Vec<Rejected>,
    /// True when the plugin declares no `derivability`: the ban ran nowhere, and the
    /// Redundancy metric prints `0 (ban inert)`.
    pub inert: bool,
}

/// Run the ban over the bound claims. `derivable` is the plugin call, supplied by the caller
/// so the loss path stays in the caller's hands. A plugin error is a run failure: the ban
/// mis-answering must not silently keep or drop claims.
pub fn gate(
    claims: Vec<BoundClaim>,
    plugin_name: &str,
    capabilities: &[Capability],
    mut derivable: impl FnMut(&str) -> Result<veridoc_protocol::DerivableResult, LostReason>,
) -> Result<Outcome, String> {
    if !capabilities.contains(&Capability::Derivability) {
        // The inert line belongs on stderr by the plan's capability table; the caller prints it.
        eprintln!("derivability ban is inert: plugin \"{plugin_name}\" does not declare it");
        return Ok(Outcome {
            kept: claims,
            rejected: Vec::new(),
            inert: true,
        });
    }

    // One call per distinct referent: the facts describe the declaration, not the claim.
    let mut facts_by_referent: Vec<(String, Vec<String>)> = Vec::new();
    let mut kept = Vec::new();
    let mut rejected = Vec::new();
    for claim in claims {
        let facts = match facts_by_referent
            .iter()
            .find(|(referent, _)| referent == &claim.referent_id)
        {
            Some((_, facts)) => facts.clone(),
            None => match derivable(&claim.referent_id) {
                Ok(answer) => {
                    facts_by_referent.push((claim.referent_id.clone(), answer.facts.clone()));
                    answer.facts
                }
                Err(lost) => {
                    return Err(format!(
                        "derivability of {} could not be asked: {lost}",
                        claim.referent_id
                    ));
                }
            },
        };
        let text = normalize(&claim.text);
        match facts.iter().find(|fact| normalize(fact) == text) {
            Some(fact) => rejected.push(Rejected {
                claim_id: claim.id,
                file: claim.file,
                line: claim.line,
                fact: fact.clone(),
            }),
            None => kept.push(claim),
        }
    }
    Ok(Outcome {
        kept,
        rejected,
        inert: false,
    })
}
