//! The wire types for the `VeridocPlugin` interface in `docs/spec.md`. The contract test in
//! `crates/xtask/tests/contract.rs` parses that block and compares its method names, its
//! `capabilities` union members, and its `protocolVersion` literal with this vocabulary, so the
//! spec text cannot drift from the compiled contract.
//!
//! No policy lives in these types. `run` answers values, never a verdict: the engine evaluates
//! the assertion tree so there is no path by which an author reports a pass.

use std::collections::BTreeMap;

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
    Initialization,
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
            Capability::Initialization => "initialization",
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
/// populated fields. `asserts` is the scenario channel: the scenario belongs to the claim and
/// arrives beside it in `CollectResult::scenarios`, so the channel carries the check and nothing
/// else (`docs/decisions/0006-a-project-adds-three-files-and-the-scenario-belongs-to-the-claim.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Channel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asserts: Option<Asserts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artefact: Option<Artefact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation: Option<Citation>,
}

impl Channel {
    /// How many of the three channels (`asserts`, `artefact`, `citation`) are populated. Exactly
    /// one is required; bind enforces it.
    pub fn populated(&self) -> usize {
        usize::from(self.asserts.is_some())
            + usize::from(self.artefact.is_some())
            + usize::from(self.citation.is_some())
    }
}

/// One declared claim, exactly as the sidecar registered it. The `id` is the plugin's: the
/// `<referent>#<predicate>` name the claim's own runtime derived, which the engine uses verbatim
/// rather than re-slugging the predicate and drifting from it. It is the address of the claim's
/// scenario in `CollectResult::scenarios` and the only thing `run` is asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RawClaim {
    pub id: String,
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

/// One declared document node, as the project's document tree reports it through `collect`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentDecl {
    pub path: String,
    pub audience: String,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_applicable: Option<String>,
}

/// One claim's scenario: its two step labels, and the names and types of the values its world
/// and call produce. It has no identity of its own — it is addressed by the claim's id — and the
/// declaration is what lets the engine refuse `contains` over a number instead of evaluating it
/// to false (`docs/decisions/0006-a-project-adds-three-files-and-the-scenario-belongs-to-the-claim.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScenarioDecl {
    /// The world, as an author wrote it: a report prints `Given an empty directory`.
    pub given: String,
    /// The call, as an author wrote it. The plugin resolves it to code; the engine never parses it.
    pub when: String,
    pub values: Vec<ValueDecl>,
}

/// One value a scenario produces: the name an assertion reads, and the type that makes a check
/// over it decidable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValueDecl {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ValueKind,
}

/// The declared type of a scenario value, spelled the five lowercase names the profile publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ValueKind {
    String,
    Number,
    Boolean,
    Array,
    Object,
}

impl ValueKind {
    /// Whether a `contains` check can hold over this type: a substring of a string, or an element
    /// of an array. Over anything else the check could never pass, so bind refuses it.
    pub fn holds_contains(&self) -> bool {
        matches!(self, ValueKind::String | ValueKind::Array)
    }

    /// The declared name, as it appears in `collect-result.json`.
    pub fn name(&self) -> &'static str {
        match self {
            ValueKind::String => "string",
            ValueKind::Number => "number",
            ValueKind::Boolean => "boolean",
            ValueKind::Array => "array",
            ValueKind::Object => "object",
        }
    }
}

/// A sidecar that would not import. The engine reports it as a run failure, because a
/// sidecar that vanished silently would take its claims with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Skipped {
    pub file: String,
    pub reason: String,
}

/// `init` parameters: what the host collected from the author, so the plugin can create the files
/// only it can write. The host writes none of them: `0013` puts the root document with the plugin,
/// and `docs/decisions/0006` fixes what a project adds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitReq {
    /// The project root, absolute, as the host resolved it.
    pub root: String,
    /// The first document of the tree: its audience, its one question, and the `.spec.md` path it
    /// reads. A second document is an edit to the index, not an answer to a prompt.
    pub document: DocumentDecl,
    /// Replace a file the plugin would create. Without it, a file that already exists is an error
    /// naming it, because a scaffold that overwrites an author's prose is worse than none.
    #[serde(default)]
    pub force: bool,
}

/// What `init` reports: the files the plugin created, and the work only the author can do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitResult {
    /// Every file the plugin created, relative to `root`.
    pub written: Vec<String>,
    /// One sentence each: the claims to declare, the prose to write, the command to run next.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<String>,
}

/// The `params` of `collect`: the one project root the plugin indexes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectReq {
    pub root: String,
}

/// What `collect` reports: the index, the raw claims, and each claim's own scenario. There is no
/// project-wide vocabulary any more: the values a scenario produces are declared by that
/// scenario, keyed by the claim's id, and a claim with no entry is judged by prose and source
/// only (`docs/decisions/0006-a-project-adds-three-files-and-the-scenario-belongs-to-the-claim.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectResult {
    pub concepts: Vec<String>,
    pub documents: Vec<DocumentDecl>,
    pub claims: Vec<RawClaim>,
    pub scenarios: BTreeMap<String, ScenarioDecl>,
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

/// One scenario run, addressed by the claim it belongs to. The scenario has no identity of its
/// own and no action or fixture name travels: the claim's id is the only address, and the plugin
/// runs the code that was declared beside that claim
/// (`docs/decisions/0006-a-project-adds-three-files-and-the-scenario-belongs-to-the-claim.md`).
///
/// A scenario that cannot run answers a JSON-RPC error with code `-32000` and a message naming
/// the claim; the host records that as the claim's `run-failed` result and keeps judging.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunRequest {
    pub claim: String,
    /// The engine's own wait, so the plugin can stop itself before the host kills the call.
    pub timeout_ms: u64,
}

/// One value the run produced: the name the claim's scenario declares for it, and the value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observed {
    /// One of the `values` names that claim's scenario declares.
    pub observable: String,
    /// The value JSON-encoded by the plugin, so `{"equals":2}` matches the string `"2"`.
    pub value: String,
}

/// One `satisfies` leaf the plugin evaluated. The engine sends no leaf ids: `{claimId}#l{n}`, in
/// the pre-order walk of the claim's own assertion tree, is computable from the claim's id and
/// the tree the plugin declared, so the answer arrives keyed the way the engine reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeafAnswer {
    pub leaf_id: String,
    pub ok: bool,
}

/// Why a scenario could not run. Never a claim stronger than `run-failed`, and never the whole
/// build's result (`docs/decisions/0006-a-project-adds-three-files-and-the-scenario-belongs-to-the-claim.md`).
/// `Fixture` is the scenario's own world or the assets it reads, not a declared fixture: with no
/// fixture registry there is no name to check before the run, so a missing world is discovered here.
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
