//! The wire types for the `VeridocPlugin` interface in `docs/spec.md`. The contract test in
//! `crates/xtask/tests/contract.rs` parses that block and compares its method names, its
//! `capabilities` union members, and its `protocolVersion` literal with this vocabulary, so the
//! spec text cannot drift from the compiled contract.
//!
//! No policy lives in these types. `run` answers values, never a verdict: the engine evaluates
//! the assertion tree so there is no path by which an author reports a pass.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The only protocol revision this build speaks, as it appears in a handshake.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Handshake {
    pub protocol_version: String,
    pub name: String,
    pub version: String,
    pub languages: Vec<String>,
    pub capabilities: Vec<Capability>,
}

/// A method family the plugin answers. A missing capability is reported, never silently
/// treated as a pass; the engine's behaviour per absence is fixed by the capability table in
/// `docs/spec.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    Collect,
    Resolution,
    Derivability,
    Bivalence,
    Execution,
    Citation,
    Surface,
}

impl Capability {
    /// The wire spelling, the one the handshake answers with, the key the profile spec's
    /// `capabilities` block is keyed by, and the name a report blames when a rule was gated on
    /// a capability the plugin did not declare.
    pub fn name(&self) -> &'static str {
        match self {
            Capability::Collect => "collect",
            Capability::Resolution => "resolution",
            Capability::Derivability => "derivability",
            Capability::Bivalence => "bivalence",
            Capability::Execution => "execution",
            Capability::Citation => "citation",
            Capability::Surface => "surface",
        }
    }
}

/// The five claim kinds. The distinction the kinds carry is normative against observational.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Requires,
    Ensures,
    Invariant,
    Witness,
    Because,
}

/// What a claim is about: a resolved symbol or a declared concept. Internally tagged on
/// `kind` so the wire shape matches the builder call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RefToken {
    Symbol { name: String, from: String },
    Concept { name: String },
}

impl RefToken {
    /// The `resolve` key. Two sidecars importing the same symbol share one key and therefore
    /// one referent.
    pub fn key(&self) -> String {
        match self {
            RefToken::Symbol { name, from } => format!("{from}|{name}"),
            RefToken::Concept { name } => format!("concept|{name}"),
        }
    }

    /// The author's name for the thing the claim is about. It is the left half of the claim's
    /// readable name, and it comes from the author rather than from the path the code sits in
    /// (`docs/decisions/0009-the-subject-is-named-not-derived-from-a-path.md`).
    pub fn subject_name(&self) -> &str {
        match self {
            RefToken::Symbol { name, .. } | RefToken::Concept { name } => name,
        }
    }
}

/// The single channel field of a claim. Flat, not externally tagged, so the wire shape
/// matches the builder call and the "two channels or none" error is a direct count of the
/// populated fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Channel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asserts: Option<Asserts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artefact: Option<Artefact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation: Option<Citation>,
}

impl Channel {
    /// How many of the three channels (`scenario`+`asserts`, `artefact`, `citation`) are
    /// populated. Exactly one is required; bind enforces it.
    pub fn populated(&self) -> usize {
        let scenario = if self.scenario.is_some() || self.asserts.is_some() {
            1
        } else {
            0
        };
        scenario + usize::from(self.artefact.is_some()) + usize::from(self.citation.is_some())
    }
}

/// One declared claim, exactly as the sidecar registered it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RawClaim {
    pub kind: Kind,
    pub subject: RefToken,
    pub concept: String,
    pub predicate: String,
    pub text: String,
    pub channel: Channel,
    pub file: String,
    pub line: u32,
}

/// An assertion node: a conjunction, disjunction, negation, or leaf, with leaves written
/// inline. A malformed node yields an unusable `untagged` message, so the engine's bind pass
/// reports the shape error itself, naming the claim id and the tree path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Asserts {
    All { all: Vec<Asserts> },
    Any { any: Vec<Asserts> },
    Not { not: Box<Asserts> },
    Leaf(AssertLeaf),
}

impl Asserts {
    /// A pre-order walk yielding each leaf with its index, which is how leaf ids
    /// `{claim_id}#l{n}` are assigned.
    pub fn leaves(&self) -> Vec<(usize, &AssertLeaf)> {
        let mut out = Vec::new();
        collect_leaves(self, &mut out);
        out
    }
}

fn collect_leaves<'a>(node: &'a Asserts, out: &mut Vec<(usize, &'a AssertLeaf)>) {
    match node {
        Asserts::All { all } => {
            for child in all {
                collect_leaves(child, out);
            }
        }
        Asserts::Any { any } => {
            for child in any {
                collect_leaves(child, out);
            }
        }
        Asserts::Not { not } => collect_leaves(not, out),
        Asserts::Leaf(leaf) => out.push((out.len(), leaf)),
    }
}

/// One leaf: an observable and the check over its value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssertLeaf {
    pub obs: String,
    pub r#is: Check,
}

/// A check over one observed value. Externally tagged, so `Equals(2)` renders as
/// `{"equals":2}` and `Satisfies(..)` as `{"satisfies":{"source":".."}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Check {
    Equals(Value),
    Contains(Value),
    Satisfies(SatisfiesSpec),
}

/// A predicate the plugin evaluates. `source` is the predicate text; `accepts` and `rejects`
/// are the witness pair the author supplies when static analysis cannot settle bivalence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SatisfiesSpec {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepts: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejects: Option<Value>,
}

/// A declared artefact: the file and the hash the claim records for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artefact {
    pub path: String,
    pub sha256: String,
}

/// A typed citation against a declared store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Citation {
    pub store: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<u32>,
}

/// One declared document node, as `index.doc.ts` reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentDecl {
    pub path: String,
    pub audience: String,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_applicable: Option<String>,
}

/// A declared observable with its declared type name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservableDecl {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

/// A declared action: the symbol it calls and the observables it produces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionDecl {
    pub name: String,
    pub calls: RefToken,
    pub produces: Vec<String>,
}

/// A sidecar that would not import. The engine reports it as a run failure, because a
/// sidecar that vanished silently would take its claims with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Skipped {
    pub file: String,
    pub reason: String,
}

/// The `params` of `collect`: the one project root the plugin indexes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectReq {
    pub root: String,
}

/// What `collect` reports: the index, the raw claims, and the observable/action vocabulary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectResult {
    pub concepts: Vec<String>,
    pub documents: Vec<DocumentDecl>,
    pub claims: Vec<RawClaim>,
    pub observables: Vec<ObservableDecl>,
    pub actions: Vec<ActionDecl>,
    pub fixtures: Vec<String>,
    pub skipped: Vec<Skipped>,
}

/// The `params` of `resolve`: the keys to settle, each a `symbol:<name>#<from-file>` or
/// `concept:<name>` key taken from a claim's referent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolveReq {
    pub refs: Vec<String>,
}

/// The answer for one requested reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedRef {
    #[serde(rename = "ref")]
    pub reference: String,
    pub referent_id: String,
    pub signature: String,
}

/// An unresolved reference is a normal answer with a reason, not a protocol error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnresolvedRef {
    #[serde(rename = "ref")]
    pub reference: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolveResult {
    pub resolved: Vec<ResolvedRef>,
    pub unresolved: Vec<UnresolvedRef>,
}

/// The `params` of `derivable`: the referent whose declaration is read for the facts a reader
/// can already see in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DerivableReq {
    pub referent_id: String,
}

/// The facts about one referent a reader can already see in the declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DerivableResult {
    pub facts: Vec<String>,
}

/// How the plugin settled (or failed to settle) bivalence. `static-analysis` is spelled with
/// the kebab case of the spec's union, not serde's camelCase default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum BivalenceMethod {
    #[serde(rename = "structural")]
    Structural,
    #[serde(rename = "static-analysis")]
    StaticAnalysis,
    #[serde(rename = "witnesses")]
    Witnesses,
}

/// The `params` of `bivalence`: the claim whose predicates must each settle to one value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BivalenceReq {
    pub claim_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BivalenceResult {
    pub bivalent: bool,
    pub method: BivalenceMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness_accepts: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness_rejects: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// One leaf the engine asks the plugin to evaluate, addressed by the id the engine issued.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunLeaf {
    pub leaf_id: String,
}

/// One scenario run. The engine parses the scenario and passes the leaves; the plugin answers
/// values and never a verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunRequest {
    pub scenario_id: String,
    pub fixture: String,
    pub action: String,
    pub args: Vec<String>,
    pub deadline_ms: u64,
    pub leaves: Vec<RunLeaf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observed {
    pub observable: String,
    /// The value JSON-encoded by the plugin, so `{"equals":2}` matches the string `"2"`.
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeafAnswer {
    pub leaf_id: String,
    pub ok: bool,
}

/// Why a scenario could not run. Never a claim state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RunFailureKind {
    Build,
    Fixture,
    Deadline,
    Plugin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunFailure {
    pub kind: RunFailureKind,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunResponse {
    pub ok: bool,
    pub observed: Vec<Observed>,
    pub satisfied: Vec<LeafAnswer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<RunFailure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedCitation {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnresolvedCitation {
    pub id: String,
    pub reason: String,
}

/// The `params` of `cite`: the citations to resolve against their declared stores.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CiteReq {
    pub citations: Vec<Citation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CiteResult {
    pub resolved: Vec<ResolvedCitation>,
    pub unresolved: Vec<UnresolvedCitation>,
}

/// One entry of the declared surface, so the run can report symbols that no claim covers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SurfaceReferent {
    pub referent_id: String,
    pub name: String,
    pub public: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SurfaceResult {
    pub referents: Vec<SurfaceReferent>,
}

/// S1: which named claims the section does not mention, and which assertions in the
/// section no claim states.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct S1Verdict {
    pub unmentioned_claims: Vec<String>,
    pub unstated_assertions: Vec<Unstated>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Unstated {
    pub span: String,
    pub text: String,
}

/// S2: pairs of spans that disagree, with the claim id between them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct S2Verdict {
    pub pairs: Vec<S2Pair>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct S2Pair {
    pub span_a: String,
    pub span_b: String,
    pub claim_id: String,
}

/// S3: pairs of document nodes whose audience and question ask the same thing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct S3Verdict {
    pub pairs: Vec<S3Pair>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct S3Pair {
    pub node_a: String,
    pub node_b: String,
}
