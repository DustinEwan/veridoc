//! The bind stage: everything that must be true before anything runs. Order matters because
//! later rules read earlier results: collect, resolve, identity, channel completeness,
//! vocabulary, scenario bind, then the tree closed both ways. Every bind error blocks: the
//! run stops before any `run` call, because a claim that cannot be bound has no channel to
//! evaluate, and printing outcomes for the rest would bury the one thing the reader must fix.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use veridoc_protocol::*;

use crate::config::CitationStoreConfig;
use crate::feature::{self, FeatureFile, FeatureProblemKind};
use crate::hash;
use crate::plugin::sentence;
use crate::result::Warning;
use crate::spec_md::{self, SpecProblemKind};

/// The `collect` answer as the engine reads it. `asserts` stays a `Value` here so the bind
/// pass can report a malformed node with the claim id and the tree path, which an `untagged`
/// serde error cannot.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectWire {
    pub concepts: Vec<String>,
    pub documents: Vec<DocumentDecl>,
    pub claims: Vec<ClaimWire>,
    pub observables: Vec<ObservableDecl>,
    pub actions: Vec<ActionDecl>,
    pub fixtures: Vec<String>,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimWire {
    pub kind: Kind,
    pub subject: RefToken,
    pub concept: String,
    pub predicate: String,
    pub text: String,
    pub channel: ChannelWire,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelWire {
    #[serde(default)]
    pub scenario: Option<String>,
    #[serde(default)]
    pub asserts: Option<Value>,
    #[serde(default)]
    pub artefact: Option<Artefact>,
    #[serde(default)]
    pub citation: Option<Citation>,
}

impl ChannelWire {
    fn populated(&self) -> usize {
        let scenario = usize::from(self.scenario.is_some() || self.asserts.is_some());
        scenario + usize::from(self.artefact.is_some()) + usize::from(self.citation.is_some())
    }
}

/// One blocking bind failure. Each carries the file and line of the thing it names, and the
/// message is the exact wording the paths table in `docs/spec.md` requires.
#[derive(Debug, thiserror::Error)]
pub enum BindError {
    #[error("referent {reference} does not resolve: {reason}")]
    Unresolved {
        file: String,
        line: u32,
        reference: String,
        reason: String,
    },
    #[error("claim {predicate} declares {count} channels; exactly one is required")]
    Channels {
        file: String,
        line: u32,
        predicate: String,
        count: usize,
    },
    #[error("scenario-channel claim {claim_id} names no scenario")]
    NoScenario {
        file: String,
        line: u32,
        claim_id: String,
    },
    #[error("scenario-channel claim {claim_id} declares no assertion tree")]
    NoAsserts {
        file: String,
        line: u32,
        claim_id: String,
    },
    #[error("claim {predicate} duplicates {existing_name}, owned by {owner_file}:{owner_line}")]
    Duplicate {
        file: String,
        line: u32,
        predicate: String,
        existing_name: String,
        owner_file: String,
        owner_line: u32,
    },
    #[error("scenario {scenario_id} acts on {action_referent}, not the claim's referent {claim_referent}")]
    WrongReferent {
        file: String,
        line: u32,
        scenario_id: String,
        action_referent: String,
        claim_referent: String,
    },
    #[error("claim {claim_id} reads observable {obs}, which action {action} cannot produce")]
    Unproduced {
        file: String,
        line: u32,
        claim_id: String,
        obs: String,
        action: String,
    },
    #[error("observable {obs} is not declared by the plugin")]
    UndeclaredObservable {
        file: String,
        line: u32,
        obs: String,
    },
    #[error("scenario {scenario_id} refers to undeclared fixture {fixture}")]
    UndeclaredFixture {
        file: String,
        line: u32,
        scenario_id: String,
        fixture: String,
    },
    #[error("scenario {scenario_id} names no declared action in {text}")]
    UnknownAction {
        file: String,
        line: u32,
        scenario_id: String,
        text: String,
    },
    #[error("scenario {scenario_id} has no Given step; a scenario that refers to no fixture is not evidence")]
    NoGiven {
        file: String,
        line: u32,
        scenario_id: String,
    },
    #[error("scenario {scenario_id} has no When step; a scenario declares one action")]
    NoWhen {
        file: String,
        line: u32,
        scenario_id: String,
    },
    #[error("scenario has no @id:<slug> tag")]
    ScenarioTag { file: String, line: u32 },
    #[error("scenario step is not allowed: {text}; a scenario has one Given and one When, and the claim owns the assertion")]
    ForbiddenStep {
        file: String,
        line: u32,
        text: String,
    },
    #[error("unrecognized line in feature file: {text}")]
    Unrecognized {
        file: String,
        line: u32,
        text: String,
    },
    #[error("empty {what}")]
    EmptyStep {
        file: String,
        line: u32,
        what: String,
    },
    #[error("section opens with no subject; write <doc-section name=\"<subject>\">")]
    SubjectName { file: String, line: u32 },
    #[error("unknown element <{element}>; the grammar defines doc-section, doc-claim, doc-each and doc-value")]
    UnknownElement {
        file: String,
        line: u32,
        element: String,
    },
    #[error("<{element}> must sit on a line of its own")]
    MisplacedElement {
        file: String,
        line: u32,
        element: String,
    },
    #[error("reference {subject}#{predicate} names no claim")]
    UnknownReference {
        file: String,
        line: u32,
        subject: String,
        predicate: String,
    },
    #[error(
        "{subject}#{predicate} matches {count} claims; name one by its suffixed id: {candidates}"
    )]
    AmbiguousReference {
        file: String,
        line: u32,
        subject: String,
        predicate: String,
        count: usize,
        candidates: String,
    },
    #[error("claim {claim_id} is named by no section")]
    UnnamedClaim {
        file: String,
        line: u32,
        claim_id: String,
    },
    #[error("doc-each names no source; the collections are claims, concepts, documents, observables, actions, fixtures")]
    MissingSource { file: String, line: u32 },
    #[error("doc-each has no block after it to repeat")]
    MissingBlock { file: String, line: u32 },
    #[error("doc-value sits outside a doc-each block, where no value can land")]
    ValueOutsideEach { file: String, line: u32 },
    #[error("doc-each source {collection} names no collection")]
    UnknownSource {
        file: String,
        line: u32,
        collection: String,
    },
    #[error("doc-each source {collection} matched nothing")]
    EmptySource {
        file: String,
        line: u32,
        collection: String,
    },
    #[error("doc-each source {collection} has no field {field}")]
    UnknownField {
        file: String,
        line: u32,
        collection: String,
        field: String,
    },
    #[error("<doc-value> in doc-each source {collection} names no field, and {collection} items carry no name to default to")]
    MissingField {
        file: String,
        line: u32,
        collection: String,
    },
    #[error("no document node in the project's document tree declares this file")]
    UndeclaredSpec { file: String, line: u32 },
    #[error("declared node is neither satisfied nor notApplicable with a reason")]
    UnsatisfiedNode { file: String, line: u32 },
    #[error("citation names undeclared store {store}")]
    UndeclaredStore {
        file: String,
        line: u32,
        store: String,
    },
    #[error("claim {claim_id} has an assertion node with neither obs nor all/any/not at {path}")]
    BadNode {
        file: String,
        line: u32,
        claim_id: String,
        path: String,
    },
    #[error("claim {claim_id} has an empty {node} node")]
    EmptyNode {
        file: String,
        line: u32,
        claim_id: String,
        node: String,
    },
    #[error("claim {claim_id} asserts contains over observable {obs} of type {ty}; it must be string or string[]")]
    ContainsType {
        file: String,
        line: u32,
        claim_id: String,
        obs: String,
        ty: String,
    },
    #[error("artefact path {path} escapes the project root")]
    ArtefactEscape {
        file: String,
        line: u32,
        path: String,
    },
    #[error("claim {claim_id} cannot take the scenario channel: plugin \"{name}\" declares no {capability}")]
    CapabilityMissing {
        file: String,
        line: u32,
        claim_id: String,
        name: String,
        capability: String,
    },
    #[error(
        "claim {claim_id} cannot take the citation channel: plugin \"{name}\" declares no citation"
    )]
    CitationCapability {
        file: String,
        line: u32,
        claim_id: String,
        name: String,
    },
    #[error("claim {claim_id} is not bivalent by witnesses: {reason}")]
    WitnessPair {
        file: String,
        line: u32,
        claim_id: String,
        reason: String,
    },
    #[error("claim {claim_id} has a satisfies leaf without the witness pair at {path}")]
    WitnessLeaf {
        file: String,
        line: u32,
        claim_id: String,
        path: String,
    },
    #[error("citation {citation} does not resolve: {reason}")]
    CitationUnresolved {
        file: String,
        line: u32,
        citation: String,
        reason: String,
    },
}

impl BindError {
    pub fn location(&self) -> (&str, u32) {
        match self {
            BindError::Unresolved { file, line, .. }
            | BindError::Channels { file, line, .. }
            | BindError::NoScenario { file, line, .. }
            | BindError::NoAsserts { file, line, .. }
            | BindError::Duplicate { file, line, .. }
            | BindError::WrongReferent { file, line, .. }
            | BindError::Unproduced { file, line, .. }
            | BindError::UndeclaredObservable { file, line, .. }
            | BindError::UndeclaredFixture { file, line, .. }
            | BindError::UnknownAction { file, line, .. }
            | BindError::NoGiven { file, line, .. }
            | BindError::NoWhen { file, line, .. }
            | BindError::ScenarioTag { file, line }
            | BindError::ForbiddenStep { file, line, .. }
            | BindError::Unrecognized { file, line, .. }
            | BindError::EmptyStep { file, line, .. }
            | BindError::SubjectName { file, line }
            | BindError::UnknownElement { file, line, .. }
            | BindError::MisplacedElement { file, line, .. }
            | BindError::MissingSource { file, line }
            | BindError::MissingBlock { file, line }
            | BindError::ValueOutsideEach { file, line }
            | BindError::UnknownSource { file, line, .. }
            | BindError::EmptySource { file, line, .. }
            | BindError::UnknownField { file, line, .. }
            | BindError::MissingField { file, line, .. }
            | BindError::UnknownReference { file, line, .. }
            | BindError::AmbiguousReference { file, line, .. }
            | BindError::UnnamedClaim { file, line, .. }
            | BindError::UndeclaredSpec { file, line }
            | BindError::UnsatisfiedNode { file, line }
            | BindError::UndeclaredStore { file, line, .. }
            | BindError::BadNode { file, line, .. }
            | BindError::EmptyNode { file, line, .. }
            | BindError::ContainsType { file, line, .. }
            | BindError::ArtefactEscape { file, line, .. }
            | BindError::CapabilityMissing { file, line, .. }
            | BindError::WitnessPair { file, line, .. }
            | BindError::WitnessLeaf { file, line, .. }
            | BindError::CitationUnresolved { file, line, .. }
            | BindError::CitationCapability { file, line, .. } => (file, *line),
        }
    }

    /// The rendered diagnostic line: `{file}:{line}: {message}`.
    pub fn diagnostic(&self) -> String {
        let (file, line) = self.location();
        format!("{file}:{line}: {self}")
    }
}

/// Bind stops two ways: findings about the documentation (exit 1), or a run failure that says
/// the check itself could not happen (exit 2).
#[derive(Debug)]
pub enum BindFailure {
    Errors(Vec<BindError>),
    RunFailure(String),
}

/// One claim after bind: identity, resolved referent, and its channel with everything the
/// later stages need.
#[derive(Debug, Clone)]
pub struct BoundClaim {
    pub id: String,
    pub kind: Kind,
    pub concept: String,
    pub predicate: String,
    pub text: String,
    pub file: String,
    pub line: u32,
    /// The author's name for the subject: the left half of `id`. The referent id below is a
    /// reporting value and moves when a file is renamed; this does not.
    pub subject: String,
    pub referent_id: String,
    pub signature: String,
    pub channel: BoundChannel,
}

#[derive(Debug, Clone)]
pub enum BoundChannel {
    Scenario {
        scenario_id: String,
        fixture: String,
        action: String,
        args: Vec<String>,
        asserts: Asserts,
    },
    Artefact {
        path: String,
        sha256: String,
    },
    Citation {
        store: String,
        id: String,
        turn: Option<u32>,
    },
    /// Reached when the bivalence gate routes a scenario-channel claim here (step 6).
    Judge,
}

/// One `.spec.md` section after bind.
#[derive(Debug, Clone)]
pub struct BoundSection {
    pub file: String,
    /// The subject the section's prose is about, from `<doc-section name="…">`.
    pub subject: String,
    pub line: u32,
    pub spans: Vec<spec_md::Span>,
    pub body: String,
    /// The claim ids the section names, in claim order, each resolved from a `<doc-claim>`.
    pub claims: Vec<String>,
    /// What each `<doc-each>` in the section pulled in, in document order. Empty when the section
    /// declares no placeholder.
    pub injections: Vec<Injected>,
}

/// What one `<doc-each>` pulled in, as the report names it: the element's source and how many
/// values it resolved to. A section whose injected content changed is re-run with no prose diff,
/// and this is the line that tells the reader which element did it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Injected {
    pub source: String,
    pub items: usize,
}

/// The whole bound project: what every later stage reads.
#[derive(Debug, Clone)]
pub struct Bound {
    pub claims: Vec<BoundClaim>,
    pub sections: Vec<BoundSection>,
    pub documents: Vec<DocumentDecl>,
    pub concepts: Vec<String>,
    pub observables: Vec<ObservableDecl>,
    pub actions: Vec<ActionDecl>,
    pub fixtures: Vec<String>,
    pub warnings: Vec<Warning>,
}

/// Bind the project. `resolve` is the one `resolve` call the engine makes, supplied by the
/// caller so the plugin loss path stays in the caller's hands.
pub fn bind(
    root: &Path,
    plugin_name: &str,
    capabilities: &[Capability],
    citation_stores: &[CitationStoreConfig],
    collect: CollectWire,
    mut resolve: impl FnMut(&[String]) -> Call<ResolveResult>,
) -> Result<Bound, BindFailure> {
    let mut errors: Vec<BindError> = Vec::new();
    let mut warnings: Vec<Warning> = Vec::new();

    // 1. A sidecar that will not import is a run failure: its claims would otherwise vanish
    //    from the build silently.
    if let Some(skipped) = collect.skipped.first() {
        return Err(BindFailure::RunFailure(format!(
            "sidecar {} did not import: {}",
            skipped.file, skipped.reason
        )));
    }

    // Sort before anything hashes: a plugin that answers in a different order cannot move a
    // claim id or the claim-set hash.
    let mut claims = collect.claims.clone();
    claims.sort_by(|a, b| (&a.file, &a.predicate).cmp(&(&b.file, &b.predicate)));

    // 2. One resolve call for every claim subject and every action call.
    let mut keys: Vec<String> = Vec::new();
    let push_key = |keys: &mut Vec<String>, key: String| {
        if !keys.contains(&key) {
            keys.push(key);
        }
    };
    for claim in &claims {
        push_key(&mut keys, claim.subject.key());
    }
    for action in &collect.actions {
        push_key(&mut keys, action.calls.key());
    }
    let resolved = sentence("resolve", resolve(&keys)).map_err(BindFailure::RunFailure)?;
    let mut referents: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut unresolved: BTreeMap<String, String> = BTreeMap::new();
    for r in &resolved.resolved {
        referents.insert(
            r.reference.clone(),
            (r.referent_id.clone(), r.signature.clone()),
        );
    }
    for u in &resolved.unresolved {
        unresolved.insert(u.reference.clone(), u.reason.clone());
    }

    // Feature files, parsed by the engine because Gherkin is veridoc's own format.
    let features = load_features(root, &mut errors);
    let scenarios: Vec<&feature::Scenario> = features.iter().flat_map(|f| &f.scenarios).collect();

    // `.spec.md` files on disk, and the index closed against them.
    let spec_files = load_spec_files(root, &mut errors, &mut warnings);

    let observable_types: BTreeMap<String, String> = collect
        .observables
        .iter()
        .map(|o| (o.name.clone(), o.ty.clone()))
        .collect();

    // 3-8. Per-claim rules.
    let mut bound: Vec<BoundClaim> = Vec::new();
    let mut owners: BTreeMap<String, (String, u32)> = BTreeMap::new();
    for claim in &claims {
        // Identity needs the referent; resolve first for this claim.
        let subject_key = claim.subject.key();
        let (referent_id, signature) = match referents.get(&subject_key) {
            Some(pair) => pair.clone(),
            None => {
                let reason = unresolved.get(&subject_key).cloned().unwrap_or_else(|| {
                    "the plugin answered neither resolved nor unresolved".to_string()
                });
                errors.push(BindError::Unresolved {
                    file: claim.file.clone(),
                    line: claim.line,
                    reference: subject_key,
                    reason,
                });
                continue;
            }
        };
        // The identity is what a duplicate is judged on; the name is what every surface prints.
        // Both take the author's subject name, not the resolved referent id (`0009`).
        let subject = claim.subject.subject_name().to_string();
        let identity = hash::claim_identity(&subject, &claim.predicate);
        let id = hash::claim_name(&subject, &claim.predicate);
        if let Some((owner_file, owner_line)) = owners.get(&identity) {
            errors.push(BindError::Duplicate {
                file: claim.file.clone(),
                line: claim.line,
                predicate: claim.predicate.clone(),
                existing_name: id,
                owner_file: owner_file.clone(),
                owner_line: *owner_line,
            });
            continue;
        }
        owners.insert(identity, (claim.file.clone(), claim.line));

        // Channel completeness: exactly one channel.
        let wire = &claim.channel;
        match wire.populated() {
            1 => {}
            n => {
                errors.push(BindError::Channels {
                    file: claim.file.clone(),
                    line: claim.line,
                    predicate: claim.predicate.clone(),
                    count: n,
                });
                continue;
            }
        }

        let channel = if wire.scenario.is_some() || wire.asserts.is_some() {
            let Some(scenario_id) = wire.scenario.clone() else {
                errors.push(BindError::NoScenario {
                    file: claim.file.clone(),
                    line: claim.line,
                    claim_id: id.clone(),
                });
                continue;
            };
            let Some(asserts_value) = wire.asserts.clone() else {
                errors.push(BindError::NoAsserts {
                    file: claim.file.clone(),
                    line: claim.line,
                    claim_id: id.clone(),
                });
                continue;
            };
            let asserts = match validate_asserts(
                &asserts_value,
                "",
                &id,
                &claim.file,
                claim.line,
                &mut errors,
            ) {
                Some(a) => a,
                None => continue,
            };
            // The bivalence and execution capabilities gate the scenario channel.
            if !capabilities.contains(&Capability::Bivalence) {
                errors.push(BindError::CapabilityMissing {
                    file: claim.file.clone(),
                    line: claim.line,
                    claim_id: id.clone(),
                    name: plugin_name.to_string(),
                    capability: "bivalence".to_string(),
                });
                continue;
            }
            if !capabilities.contains(&Capability::Execution) {
                errors.push(BindError::CapabilityMissing {
                    file: claim.file.clone(),
                    line: claim.line,
                    claim_id: id.clone(),
                    name: plugin_name.to_string(),
                    capability: "execution".to_string(),
                });
                continue;
            }
            // Scenario bind: the scenario exists, its steps name declared things, and its
            // action acts on the claim's own referent.
            let Some(scenario) = scenarios.iter().copied().find(|s| s.id == scenario_id) else {
                errors.push(BindError::NoScenario {
                    file: claim.file.clone(),
                    line: claim.line,
                    claim_id: id.clone(),
                });
                continue;
            };
            let scenario_file = features
                .iter()
                .find(|f| f.scenarios.iter().any(|s| s.id == scenario_id))
                .map(|f| f.path.clone())
                .unwrap_or_else(|| claim.file.clone());
            let Some(given) = &scenario.given else {
                errors.push(BindError::NoGiven {
                    file: scenario_file.clone(),
                    line: scenario.line,
                    scenario_id: scenario_id.clone(),
                });
                continue;
            };
            let Some(fixture) = collect
                .fixtures
                .iter()
                .find(|f| given.text.contains(f.as_str()))
            else {
                errors.push(BindError::UndeclaredFixture {
                    file: scenario_file.clone(),
                    line: given.line,
                    scenario_id: scenario_id.clone(),
                    fixture: given.text.clone(),
                });
                continue;
            };
            let Some(when) = &scenario.when else {
                errors.push(BindError::NoWhen {
                    file: scenario_file.clone(),
                    line: scenario.line,
                    scenario_id: scenario_id.clone(),
                });
                continue;
            };
            let Some(action) = collect.actions.iter().find(|a| when.text.contains(&a.name)) else {
                errors.push(BindError::UnknownAction {
                    file: scenario_file.clone(),
                    line: when.line,
                    scenario_id: scenario_id.clone(),
                    text: when.text.clone(),
                });
                continue;
            };
            let action_key = action.calls.key();
            let action_referent = match referents.get(&action_key) {
                Some((id, _)) => id.clone(),
                None => {
                    let reason = unresolved.get(&action_key).cloned().unwrap_or_else(|| {
                        "the plugin answered neither resolved nor unresolved".to_string()
                    });
                    errors.push(BindError::Unresolved {
                        file: claim.file.clone(),
                        line: claim.line,
                        reference: action_key,
                        reason,
                    });
                    continue;
                }
            };
            if action_referent != referent_id {
                errors.push(BindError::WrongReferent {
                    file: claim.file.clone(),
                    line: claim.line,
                    scenario_id: scenario_id.clone(),
                    action_referent,
                    claim_referent: referent_id.clone(),
                });
                continue;
            }
            // Vocabulary: every observable an assertion reads is declared, and the action
            // produces it. A `contains` over a non-string type is a bind error here too.
            let mut bad = false;
            for (_, leaf) in asserts.leaves() {
                let Some(ty) = observable_types.get(&leaf.obs) else {
                    errors.push(BindError::UndeclaredObservable {
                        file: claim.file.clone(),
                        line: claim.line,
                        obs: leaf.obs.clone(),
                    });
                    bad = true;
                    continue;
                };
                if !action.produces.iter().any(|p| p == &leaf.obs) {
                    errors.push(BindError::Unproduced {
                        file: claim.file.clone(),
                        line: claim.line,
                        claim_id: id.clone(),
                        obs: leaf.obs.clone(),
                        action: action.name.clone(),
                    });
                    bad = true;
                }
                if matches!(leaf.r#is, Check::Contains(_)) && !is_stringish(ty) {
                    errors.push(BindError::ContainsType {
                        file: claim.file.clone(),
                        line: claim.line,
                        claim_id: id.clone(),
                        obs: leaf.obs.clone(),
                        ty: ty.clone(),
                    });
                    bad = true;
                }
            }
            if bad {
                continue;
            }
            BoundChannel::Scenario {
                scenario_id,
                fixture: fixture.clone(),
                action: action.name.clone(),
                args: vec![],
                asserts,
            }
        } else if let Some(artefact) = &wire.artefact {
            if escapes_root(root, &artefact.path) {
                errors.push(BindError::ArtefactEscape {
                    file: claim.file.clone(),
                    line: claim.line,
                    path: artefact.path.clone(),
                });
                continue;
            }
            BoundChannel::Artefact {
                path: artefact.path.clone(),
                sha256: artefact.sha256.clone(),
            }
        } else if let Some(citation) = &wire.citation {
            if !capabilities.contains(&Capability::Citation) {
                errors.push(BindError::CitationCapability {
                    file: claim.file.clone(),
                    line: claim.line,
                    claim_id: id.clone(),
                    name: plugin_name.to_string(),
                });
                continue;
            }
            if !citation_stores.iter().any(|s| s.name == citation.store) {
                errors.push(BindError::UndeclaredStore {
                    file: claim.file.clone(),
                    line: claim.line,
                    store: citation.store.clone(),
                });
                continue;
            }
            BoundChannel::Citation {
                store: citation.store.clone(),
                id: citation.id.clone(),
                turn: citation.turn,
            }
        } else {
            // `populated()` already rejected zero and two-or-more.
            continue;
        };

        bound.push(BoundClaim {
            id: id.clone(),
            kind: claim.kind,
            concept: claim.concept.clone(),
            predicate: claim.predicate.clone(),
            text: claim.text.clone(),
            file: claim.file.clone(),
            line: claim.line,
            subject,
            referent_id,
            signature,
            channel,
        });
    }

    // The tree, closed both ways.
    let declared_paths: Vec<&str> = collect.documents.iter().map(|d| d.path.as_str()).collect();
    for path in spec_files.keys() {
        if !declared_paths.contains(&path.as_str()) {
            errors.push(BindError::UndeclaredSpec {
                file: path.clone(),
                line: 1,
            });
        }
    }
    for doc in &collect.documents {
        let satisfied = spec_files.contains_key(&doc.path)
            || doc
                .not_applicable
                .as_deref()
                .is_some_and(|r| !r.trim().is_empty());
        if !satisfied {
            errors.push(BindError::UnsatisfiedNode {
                file: doc.path.clone(),
                line: 1,
            });
        }
    }

    // Two predicates can slug alike. The identity suffix separates them before a document names
    // a claim, so the author and the engine agree on one spelling (`docs/spec.md:152-158`).
    let base_ids: Vec<String> = bound.iter().map(|claim| claim.id.clone()).collect();
    {
        let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (i, claim) in bound.iter().enumerate() {
            by_name.entry(claim.id.as_str()).or_default().push(i);
        }
        let suffixed: Vec<(usize, String)> = by_name
            .values()
            .filter(|indexes| indexes.len() > 1)
            .flat_map(|indexes| {
                indexes.iter().map(|&i| {
                    let claim = &bound[i];
                    let identity = hash::claim_identity(&claim.subject, &claim.predicate);
                    (i, format!("{}{}", claim.id, hash::suffix(&identity)))
                })
            })
            .collect();
        drop(by_name);
        for (i, name) in suffixed {
            bound[i].id = name;
        }
    }

    // Coverage, both directions: every `<doc-claim>` names one claim, and every claim is named
    // by at least one section. A reference that names nothing is prose describing a check that
    // does not exist, and a claim no section names is a check no reader will ever meet, so both
    // stop the build (`docs/decisions/0010-a-claim-reference-names-the-claim.md`).
    // Both spellings resolve: the bare `subject#predicate` the author writes, and the suffixed
    // id a collided slug forces them to fall back to.
    let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, id) in base_ids.iter().enumerate() {
        by_name.entry(id.as_str()).or_default().push(index);
        if bound[index].id != *id {
            by_name
                .entry(bound[index].id.as_str())
                .or_default()
                .push(index);
        }
    }
    let mut sections: Vec<BoundSection> = Vec::new();
    for (path, parsed) in &spec_files {
        for section in &parsed.sections {
            let mut named: Vec<String> = Vec::new();
            if !section.subject.is_empty() {
                for reference in &section.references {
                    let key = format!("{}#{}", section.subject, reference.predicate);
                    match by_name.get(key.as_str()) {
                        None => errors.push(BindError::UnknownReference {
                            file: path.clone(),
                            line: reference.line,
                            subject: section.subject.clone(),
                            predicate: reference.predicate.clone(),
                        }),
                        Some(indexes) if indexes.len() > 1 => {
                            let candidates = indexes
                                .iter()
                                .map(|&i| bound[i].id.clone())
                                .collect::<Vec<_>>()
                                .join(", ");
                            errors.push(BindError::AmbiguousReference {
                                file: path.clone(),
                                line: reference.line,
                                subject: section.subject.clone(),
                                predicate: reference.predicate.clone(),
                                count: indexes.len(),
                                candidates,
                            });
                        }
                        Some(indexes) => {
                            let id = bound[indexes[0]].id.clone();
                            if !named.contains(&id) {
                                named.push(id);
                            }
                        }
                    }
                }
            }
            sections.push(BoundSection {
                file: path.clone(),
                subject: section.subject.clone(),
                line: section.line,
                spans: section.spans.clone(),
                body: section.body.clone(),
                claims: named,
                injections: Vec::new(),
            });
        }
    }
    sections.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));

    // Assembly: a `<doc-each>` and its block become one copy of the block per value the source
    // names, so the body every later stage hashes, briefs, and cites is the assembled one.
    // Injected content therefore invalidates the verdicts of the section that pulled it in, which
    // is the reason to co-locate the prose with the code at all
    // (`docs/decisions/0011-an-authored-placeholder-pulls-its-content-at-assembly.md`).
    for section in &mut sections {
        let authored = spec_files
            .get(&section.file)
            .and_then(|parsed| parsed.sections.iter().find(|s| s.line == section.line));
        let Some(authored) = authored else { continue };
        if authored.injections.is_empty() {
            continue;
        }
        let mut body = section.body.clone();
        let mut pulled: Vec<Injected> = Vec::new();
        // Reverse order, so replacing one range leaves the offsets of the ones before it alone.
        for injection in authored.injections.iter().rev() {
            let Some(source) = Source::named(&injection.source, &bound, section, &collect) else {
                errors.push(BindError::UnknownSource {
                    file: section.file.clone(),
                    line: injection.line,
                    collection: injection.source.clone(),
                });
                continue;
            };
            if source.len() == 0 {
                errors.push(BindError::EmptySource {
                    file: section.file.clone(),
                    line: injection.line,
                    collection: injection.source.clone(),
                });
                continue;
            }
            let default = source.default_field();
            let mut fields_ok = true;
            for field in &injection.fields {
                let named = match (field, default) {
                    (Some(field), _) => field.clone(),
                    (None, Some(field)) => field.to_string(),
                    (None, None) => {
                        errors.push(BindError::MissingField {
                            file: section.file.clone(),
                            line: injection.line,
                            collection: injection.source.clone(),
                        });
                        fields_ok = false;
                        break;
                    }
                };
                if source.field(0, &named).is_none() {
                    errors.push(BindError::UnknownField {
                        file: section.file.clone(),
                        line: injection.line,
                        collection: injection.source.clone(),
                        field: named,
                    });
                    fields_ok = false;
                }
            }
            if !fields_ok {
                continue;
            }
            let rendered: Vec<String> = (0..source.len())
                .map(|item| {
                    spec_md::substitute(&injection.block, &|field| {
                        let named = if field.is_empty() {
                            default.unwrap_or_default()
                        } else {
                            field
                        };
                        source.field(item, named).unwrap_or_default()
                    })
                })
                .collect();
            body.replace_range(injection.start..injection.end, &rendered.join("\n"));
            pulled.push(Injected {
                source: injection.source.clone(),
                items: source.len(),
            });
        }
        pulled.reverse();
        section.body = body;
        section.spans = spec_md::assign_spans(&section.body);
        section.injections = pulled;
    }

    for claim in &bound {
        if !sections.iter().any(|s| s.claims.contains(&claim.id)) {
            errors.push(BindError::UnnamedClaim {
                file: claim.file.clone(),
                line: claim.line,
                claim_id: claim.id.clone(),
            });
        }
    }

    if !errors.is_empty() {
        return Err(BindFailure::Errors(errors));
    }
    Ok(Bound {
        claims: bound,
        sections,
        documents: collect.documents.clone(),
        concepts: collect.concepts.clone(),
        observables: collect.observables.clone(),
        actions: collect.actions.clone(),
        fixtures: collect.fixtures.clone(),
        warnings,
    })
}

/// Validate one assertion node. `path` names the node inside the tree for the error message.
/// Returns `None` after recording the error.
fn validate_asserts(
    value: &Value,
    path: &str,
    claim_id: &str,
    file: &str,
    line: u32,
    errors: &mut Vec<BindError>,
) -> Option<Asserts> {
    let Some(object) = value.as_object() else {
        errors.push(bad_node(claim_id, file, line, path));
        return None;
    };
    if let Some(children) = object.get("all") {
        return match children.as_array() {
            Some(items) if items.is_empty() => {
                errors.push(BindError::EmptyNode {
                    file: file.to_string(),
                    line,
                    claim_id: claim_id.to_string(),
                    node: "all".to_string(),
                });
                None
            }
            Some(items) => {
                let mut out = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    let child_path = if path.is_empty() {
                        format!("all[{i}]")
                    } else {
                        format!("{path}.all[{i}]")
                    };
                    out.push(validate_asserts(
                        item,
                        &child_path,
                        claim_id,
                        file,
                        line,
                        errors,
                    )?);
                }
                Some(Asserts::All { all: out })
            }
            None => {
                errors.push(bad_node(claim_id, file, line, path));
                None
            }
        };
    }
    if let Some(children) = object.get("any") {
        return match children.as_array() {
            Some(items) if items.is_empty() => {
                errors.push(BindError::EmptyNode {
                    file: file.to_string(),
                    line,
                    claim_id: claim_id.to_string(),
                    node: "any".to_string(),
                });
                None
            }
            Some(items) => {
                let mut out = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    let child_path = if path.is_empty() {
                        format!("any[{i}]")
                    } else {
                        format!("{path}.any[{i}]")
                    };
                    out.push(validate_asserts(
                        item,
                        &child_path,
                        claim_id,
                        file,
                        line,
                        errors,
                    )?);
                }
                Some(Asserts::Any { any: out })
            }
            None => {
                errors.push(bad_node(claim_id, file, line, path));
                None
            }
        };
    }
    if let Some(child) = object.get("not") {
        let child_path = if path.is_empty() {
            "not".to_string()
        } else {
            format!("{path}.not")
        };
        let inner = validate_asserts(child, &child_path, claim_id, file, line, errors)?;
        return Some(Asserts::Not {
            not: Box::new(inner),
        });
    }
    // A leaf: `obs` plus `is`, and `is` is one of the three check shapes.
    let obs = object.get("obs").and_then(Value::as_str);
    let is = object.get("is");
    let (Some(obs), Some(is)) = (obs, is) else {
        errors.push(bad_node(claim_id, file, line, path));
        return None;
    };
    let check = match is.as_object().map(|o| o.keys().collect::<Vec<_>>()) {
        Some(keys) if keys.len() == 1 => match keys[0].as_str() {
            "equals" => Check::Equals(is["equals"].clone()),
            "contains" => Check::Contains(is["contains"].clone()),
            "satisfies" => {
                let Ok(spec) = serde_json::from_value::<SatisfiesSpec>(is["satisfies"].clone())
                else {
                    errors.push(bad_node(claim_id, file, line, path));
                    return None;
                };
                Check::Satisfies(spec)
            }
            _ => {
                errors.push(bad_node(claim_id, file, line, path));
                return None;
            }
        },
        _ => {
            errors.push(bad_node(claim_id, file, line, path));
            return None;
        }
    };
    Some(Asserts::Leaf(AssertLeaf {
        obs: obs.to_string(),
        r#is: check,
    }))
}

fn bad_node(claim_id: &str, file: &str, line: u32, path: &str) -> BindError {
    BindError::BadNode {
        file: file.to_string(),
        line,
        claim_id: claim_id.to_string(),
        path: if path.is_empty() {
            "asserts".to_string()
        } else {
            path.to_string()
        },
    }
}

fn is_stringish(ty: &str) -> bool {
    let ty = ty.trim();
    ty == "string" || ty == "string[]" || ty == "String" || ty == "String[]" || ty == "any"
}

/// True when the artefact path resolves outside the project root.
fn escapes_root(root: &Path, path: &str) -> bool {
    let p = Path::new(path);
    if p.is_absolute() {
        return !p.starts_with(root);
    }
    // Lexically normalize `.` and `..` against the joined path, then check containment:
    // `../sibling` escapes the root even though it never climbs above the filesystem root.
    let joined = root.join(p);
    use std::path::Component;
    let mut normalized: Vec<Component> = Vec::new();
    for component in joined.components() {
        match component {
            Component::ParentDir => match normalized.last() {
                Some(Component::Normal(_)) => {
                    normalized.pop();
                }
                _ => return true,
            },
            Component::CurDir => {}
            other => normalized.push(other),
        }
    }
    !normalized
        .into_iter()
        .collect::<PathBuf>()
        .starts_with(root)
}

fn load_features(root: &Path, errors: &mut Vec<BindError>) -> Vec<FeatureFile> {
    let dir = root.join("features");
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return files;
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "feature"))
        .collect();
    paths.sort();
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rel = relative(root, &path);
        let (parsed, problems) = feature::parse(&rel, &text);
        for problem in problems {
            errors.push(match problem.kind {
                FeatureProblemKind::MissingTag => BindError::ScenarioTag {
                    file: problem.file,
                    line: problem.line,
                },
                FeatureProblemKind::ForbiddenStep(text) => BindError::ForbiddenStep {
                    file: problem.file,
                    line: problem.line,
                    text,
                },
                FeatureProblemKind::SecondGiven => BindError::ForbiddenStep {
                    file: problem.file,
                    line: problem.line,
                    text: "a second Given".to_string(),
                },
                FeatureProblemKind::SecondWhen => BindError::ForbiddenStep {
                    file: problem.file,
                    line: problem.line,
                    text: "a second When".to_string(),
                },
                FeatureProblemKind::Empty(what) => BindError::EmptyStep {
                    file: problem.file,
                    line: problem.line,
                    what,
                },
                FeatureProblemKind::Unrecognized(text) => BindError::Unrecognized {
                    file: problem.file,
                    line: problem.line,
                    text,
                },
            });
        }
        files.push(parsed);
    }
    files
}

fn load_spec_files(
    root: &Path,
    errors: &mut Vec<BindError>,
    warnings: &mut Vec<Warning>,
) -> BTreeMap<String, spec_md::SpecFile> {
    let mut out = BTreeMap::new();
    let mut paths = Vec::new();
    collect_spec_paths(root, &mut paths);
    paths.sort();
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rel = relative(root, &path);
        let (parsed, problems) = spec_md::parse(&rel, &text);
        for problem in problems {
            match problem.kind {
                SpecProblemKind::MissingSubjectName => errors.push(BindError::SubjectName {
                    file: problem.file,
                    line: problem.line,
                }),
                SpecProblemKind::UnknownElement(element) => {
                    errors.push(BindError::UnknownElement {
                        file: problem.file,
                        line: problem.line,
                        element,
                    })
                }
                SpecProblemKind::MisplacedElement(element) => {
                    errors.push(BindError::MisplacedElement {
                        file: problem.file,
                        line: problem.line,
                        element,
                    })
                }
                SpecProblemKind::MissingSource => errors.push(BindError::MissingSource {
                    file: problem.file,
                    line: problem.line,
                }),
                SpecProblemKind::MissingBlock => errors.push(BindError::MissingBlock {
                    file: problem.file,
                    line: problem.line,
                }),
                SpecProblemKind::ValueOutsideEach => errors.push(BindError::ValueOutsideEach {
                    file: problem.file,
                    line: problem.line,
                }),
                SpecProblemKind::NoReferences => warnings.push(Warning {
                    file: Some(problem.file.clone()),
                    line: Some(problem.line),
                    message: "the section names no claim".to_string(),
                }),
            }
        }
        out.insert(rel, parsed);
    }
    out
}

fn collect_spec_paths(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name.starts_with('.') || name == "node_modules" || name == "target" || name == "dist"
            {
                continue;
            }
            collect_spec_paths(&path, out);
        } else if name.ends_with(".spec.md") {
            out.push(path);
        }
    }
}

pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// What a `<doc-each source="…">` gathers: one of the collections the bound tree already holds,
/// so injection asks nothing new of a plugin and needs no include mechanism
/// (`docs/decisions/0011-an-authored-placeholder-pulls-its-content-at-assembly.md`). This set is
/// the whole vocabulary, and a source naming anything else is an error rather than an empty render.
enum Source<'a> {
    Claims(Vec<&'a BoundClaim>),
    Concepts(&'a [String]),
    Documents(&'a [DocumentDecl]),
    Observables(&'a [ObservableDecl]),
    Actions(&'a [ActionDecl]),
    Fixtures(&'a [String]),
}

impl<'a> Source<'a> {
    /// The collection a source names, resolved against the tree. `claims` is the section's own
    /// claims, because a section's prose is about them; every other collection is the project's.
    fn named(
        source: &str,
        bound: &'a [BoundClaim],
        section: &BoundSection,
        collect: &'a CollectWire,
    ) -> Option<Self> {
        match source {
            "claims" => Some(Source::Claims(
                section
                    .claims
                    .iter()
                    .filter_map(|id| bound.iter().find(|claim| &claim.id == id))
                    .collect(),
            )),
            "concepts" => Some(Source::Concepts(&collect.concepts)),
            "documents" => Some(Source::Documents(&collect.documents)),
            "observables" => Some(Source::Observables(&collect.observables)),
            "actions" => Some(Source::Actions(&collect.actions)),
            "fixtures" => Some(Source::Fixtures(&collect.fixtures)),
            _ => None,
        }
    }

    fn len(&self) -> usize {
        match self {
            Source::Claims(items) => items.len(),
            Source::Concepts(items) => items.len(),
            Source::Documents(items) => items.len(),
            Source::Observables(items) => items.len(),
            Source::Actions(items) => items.len(),
            Source::Fixtures(items) => items.len(),
        }
    }

    /// The value of one field of one item. `None` is a field this collection does not have, which
    /// is an error the author sees before any document is written.
    fn field(&self, item: usize, field: &str) -> Option<String> {
        match self {
            Source::Claims(claims) => {
                let claim = *claims.get(item)?;
                Some(match field {
                    "id" => claim.id.clone(),
                    "subject" => claim.subject.clone(),
                    "predicate" => claim.predicate.clone(),
                    "concept" => claim.concept.clone(),
                    "text" => claim.text.clone(),
                    "kind" => serde_json::to_value(claim.kind).ok()?.as_str()?.to_string(),
                    "referent" => claim.referent_id.clone(),
                    "signature" => claim.signature.clone(),
                    "file" => claim.file.clone(),
                    "line" => claim.line.to_string(),
                    "channel" => channel_name(&claim.channel).to_string(),
                    _ => return None,
                })
            }
            Source::Concepts(names) | Source::Fixtures(names) => match field {
                "name" => names.get(item).cloned(),
                _ => None,
            },
            Source::Documents(documents) => {
                let document = documents.get(item)?;
                Some(match field {
                    "path" => document.path.clone(),
                    "audience" => document.audience.clone(),
                    "question" => document.question.clone(),
                    _ => return None,
                })
            }
            Source::Observables(observables) => {
                let observable = observables.get(item)?;
                Some(match field {
                    "name" => observable.name.clone(),
                    "type" => observable.ty.clone(),
                    _ => return None,
                })
            }
            Source::Actions(actions) => {
                let action = actions.get(item)?;
                Some(match field {
                    "name" => action.name.clone(),
                    "calls" => action.calls.subject_name().to_string(),
                    "produces" => action.produces.join(", "),
                    _ => return None,
                })
            }
        }
    }

    /// The field a bare `<doc-value/>` answers. Every collection but `claims` and `documents`
    /// names its items, so those two require the attribute rather than guessing one.
    fn default_field(&self) -> Option<&'static str> {
        match self {
            Source::Claims(_) | Source::Documents(_) => None,
            _ => Some("name"),
        }
    }
}

/// The channel that produced a claim's outcome, spelled the way the result document spells it.
fn channel_name(channel: &BoundChannel) -> &'static str {
    match channel {
        BoundChannel::Scenario { .. } => "scenario",
        BoundChannel::Artefact { .. } => "artefact",
        BoundChannel::Citation { .. } => "citation",
        BoundChannel::Judge => "judge",
    }
}
