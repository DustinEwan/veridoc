# veridoc specification, v1 (draft)

This document gives the v1 design for someone who will implement veridoc or decide whether an idea
belongs in it. It describes intended behaviour, not current behaviour, because no part of it is
built yet. Where a rule exists to stop a specific failure, the failure sits next to it.

## Contents

- [The problem this solves](#the-problem-this-solves)
- [The five objects](#the-five-objects)
- [Two graphs that never merge](#two-graphs-that-never-merge)
- [Project layout](#project-layout)
- [Claims](#claims)
- [The document tree](#the-document-tree)
- [Scenarios](#scenarios)
- [Admissible evidence](#admissible-evidence)
- [Judge tasks](#judge-tasks)
- [Two artifact stages](#two-artifact-stages)
- [Engine and plugin boundary](#engine-and-plugin-boundary)
- [The report](#the-report)
- [What veridoc does not do](#what-veridoc-does-not-do)
- [Open questions](#open-questions)

## The problem this solves

Documentation written by an agent fails in ways that documentation written by a human also fails, but
faster and at a volume that makes the failures dominant. Each failure below is the reason for one
rule later in this document.

| Failure | Cause | Rule that targets it |
| --- | --- | --- |
| A document describes a symbol that no longer exists | The prose has no address | Referents are resolved, not written as strings |
| The same fact appears in five places and the copies disagree | Nothing says which copy owns the fact | Claim identity rejects a second claim with the same subject and predicate |
| Prose explains why a shortcut was acceptable | The writer had a free-text slot | No top-level prose slot exists; `because` requires an external citation |
| Prose hedges instead of committing | Normative and observational language share one slot | Normative kinds reject observational modifiers |
| A document references a test that nobody can re-run | The claim has no executable witness | A `witness` claim must resolve to a runnable artefact |
| Prose restates the signature | Nothing caps how much a document may contain | A claim that a plugin can derive from declarations is rejected |
| Documents multiply without anyone deciding to add one | Nothing closes the set of documents | The index closes the tree and obligates every node |

One failure is not addressed by any rule here: whether a well-formed, well-evidenced claim actually
means what the author intended. Section [Judge tasks](#judge-tasks) reduces that failure and does not
remove it.

## The five objects

| Object | What it is | What identifies it |
| --- | --- | --- |
| Referent | A reference to one symbol, boundary, or declared concept, resolved by the plugin | A stable id that the plugin derives from the declaration, not from a file position |
| Claim | One atomic assertion about one subject, of a declared kind | `sha(subjectId, predicate)` |
| Scenario | An executable witness in Given/When/Then form, bound to one or more referents | Its file and scenario name |
| Document node | One declared element of the document tree, with an audience and one question | Its path in the tree |
| Verdict | A pinned judge answer about one claim against one scenario set | `sha(claimHash, referentRevision, scenarioSetHash, model, promptVersion)` |

A claim is atomic in one specific sense: it has one subject and one predicate. A sentence that
describes two behaviours is two claims. This is what makes deduplication possible, and it is the rule
that an agent author will break most often, so the checker enforces it by requiring one referent in
the `subject` field and rejecting a predicate that matches more than one assertion slug.

## Two graphs that never merge

veridoc maintains a claim graph and a document tree, and it never writes a claim into the tree.

The claim graph is organised by concept. A claim exists once, and its identity is its subject and its
predicate. The document tree is organised by audience. A document node selects claims and orders them;
it holds no prose of its own beyond its declared question.

The reason for the split is the duplication failure. Facts do not follow the file layout of an
implementation, and they do not follow the reading order of a reader, so a layout that keeps facts
in one of those shapes makes a reader copy them elsewhere. An author who needs a fact in two
documents edits one claim and selects it twice.

Rendering produces every readable document, including the agent-facing rule files. A generated file
records its source, and a hand edit to a generated file is an error.

## Project layout

```
project/
  index.doc.ts                    the document tree: closed, obligating
  docs/
    claims/<concept>.doc.ts       claims, organised by concept
    generated/**/*.md             rendered documents, never hand-edited
  features/
    <concept>.feature             scenarios, Given/When/Then
    <concept>.steps.ts            step definitions, drawn from the declared vocabulary
  .veridoc/
    <runId>/veridoc.jsonl         the run journal
    <runId>/report.md             the human-readable report
    verdicts.json                 the pinned verdict cache
```

A concept file holds the claims about one concept, and its subjects may be symbols in any part of the
tree. Per-file review locality comes from the generated output, which renders a view per source file,
rather than from the layout of the claim sources.

## Claims

A claim is a call to a typed builder, so a claim that points at a renamed symbol stops compiling.

```ts
import { claim } from "@veridoc/claims";

claim.ensures({
  subject: ref(parseConfig),                 // resolved by the plugin, not a string
  predicate: "rejects-missing-project-root",
  text: "A missing project root is refused with a diagnostic that names the searched path.",
  evidence: [scenario("config/rejects-missing-root")],
});
```

### Kinds

| Kind | Modality | Shown to be false by | Lowers to |
| --- | --- | --- | --- |
| `term` | Definitional | A product decision, which is outside the repository | Nothing |
| `requires` | Normative | A call that satisfies the precondition and breaks the postcondition | A scenario `Given`, or a property check |
| `ensures` | Normative | A call whose observable result contradicts the postcondition | A scenario `Then`, or a property check |
| `invariant` | Normative | A reachable state that violates the property | A property check over a declared state space |
| `witness` | Observational | A re-run that no longer reproduces | A runnable artefact id |
| `because` | Provenance | A citation that no longer resolves, or an external decision | Nothing |
| `nonGoal` | Normative, negative | A scenario that the claim says will not pass, and does | A probe scenario |

Every kind carries `subject`, `predicate`, `text`, and optional `evidence`. `because` additionally
requires `citation`, and `witness` additionally requires `artefact`.

### Subject when there is no symbol

`term` and `because` often describe a concept rather than a symbol, and a claim needs a subject to
have an identity. A declared concept in the index is therefore a first-class subject. The index
declares concepts alongside document nodes, and a concept has a stable id that no source edit changes.

### Identity and the one-owner rule

Claim identity is `sha(subjectId, predicate)`. A second claim with the same identity is an error that
reports the path of the existing owner. The author then edits the owner or chooses a different
predicate, which is the behaviour that a duplication-prone author will not choose on their own.

A repeat of an existing claim in a second document is not a second claim. It is a selection of the
first one, and the renderer writes it in both places.

### The derivability ban

A claim is rejected as redundant when the plugin can derive its content from declarations. The v1
derivable set is deliberately narrow, because a checker that over-rejects teaches the author to
disable it:

- the parameter list and the return type of the subject;
- whether the subject is exported, and from which module;
- the members of an enum or a union that the subject accepts;
- the existence of an explicit raise site in the subject's body.

A claim that says only one of those things adds nothing that a reader cannot read off the declaration,
and it is one more sentence that goes stale when the declaration changes.

### The hedging rule

Normative kinds reject observational modifiers. The v1 list is closed and small: `currently`, `as of`,
`in practice`, `today`, `right now`, `actually`, `for now`, `temporarily`, `usually`, `mostly`.

Observational kinds require a temporal or observational anchor, so `witness` and `because` reject a
`text` field with none of those words in it. The list is legal exactly where it is honest, which is
what makes this rule enforceable rather than a style preference.

### The residue

Every claim declares `lowersTo` as `property`, `scenario`, or `none`, and `none` requires a reason.
The claims that lower to nothing are the residue, and the residue count is the headline number of a
veridoc run, because it is the set that no machine can show to be false. Prose about facts that come
from outside the repository is the permanent occupant of this set. A residue that grows without an
external cause is the signal that the check vocabulary is too narrow, not that the project needs more
prose.

## The document tree

`index.doc.ts` declares the concepts, the document nodes, and the obligation each node carries.

```ts
import { tree } from "@veridoc/tree";

export default tree({
  concepts: ["project-scan", "harness-contract"],
  documents: [
    tree.doc({
      path: "generated/harness-authoring.md",
      audience: "an author writing a new plugin",
      question: "What must my plugin answer, and in what order?",
      selects: { concept: "harness-contract" },
    }),
    tree.doc({
      path: "generated/AGENTS.md",
      audience: "an agent working in this repository",
      question: "Which rules bind every change here?",
      selects: { kind: ["nonGoal", "invariant"] },
    }),
  ],
});
```

### Closed and obligating

The index closes the tree: a document that the index does not declare cannot be written, and a
generated file that no node declares is an error.

The index also obligates the tree. Every declared node must be satisfied or declared `notApplicable`
with a reason, and a new `notApplicable` fails the run until the reason is written down. Adding a
node is a diff to the index, which is the one place in the system where a human decision about scope
is unavoidable and visible.

### Focus

Every node carries one audience and one question. The S3 judge task compares the (audience, question)
pairs of all nodes and reports pairs that ask the same thing. This is the only mechanical handle on
unfocused growth, and it runs against the index, which is small enough to judge on every change.

## Scenarios

A scenario is Gherkin, because the three slots are the same three slots as a claim: `Given` is state,
`When` acts on a referent, `Then` asserts an observable predicate. Writing evidence in the same shape
as the claim is what makes the judge task in [Judge tasks](#judge-tasks) answerable at all.

```gherkin
Feature: project scan

  Scenario: A missing project root is refused
    Given a directory with no autofuzz.json
    When afz probe-plugin runs in that directory
    Then the run stops with exit code 2
    And the diagnostic names the path that was searched
```

Each scenario declares its referents in a header comment that the checker parses, so a scenario points
at the code that it describes, and drift in that code is visible.

### The step vocabulary is closed

Slop moves to the step definitions if nothing constrains them, because a step such as `Then the
response should be correct` can be defined to assert anything. The project declares every step once,
with typed arguments and a predicate over one observable that the step reads.

```ts
steps.then("the run stops with exit code {int}", {
  observable: "process.exitCode",
  predicate: (actual: number, expected: number) => actual === expected,
});
```

Adding a step definition is a reviewed diff. A step whose predicate always returns true, or whose
observable is never read, is rejected. A `Then` step with no predicate is rejected.

### Determinism

A scenario that can pass and fail on the same input cannot support a sentence, so admissible evidence
requires that the `Given` world be a declared fixture rather than a live system. A scenario that
reaches a live service still runs and still reports, and its result is marked inadmissible.

## Admissible evidence

A claim is evidenced when the obligation that the engine generated from it passes on the revision under
review, inside a scenario that is deterministic and bound to the claim's referent.

The engine builds that obligation from the claim, from the closed step vocabulary, and from the resolved
signature of the referent. The author states what must hold and names the observable, and the engine
writes the check. That is where the strength of the evidence comes from, and it is why nothing audits a
test afterwards: the author did not write the assertion, and a step whose predicate accepts every value
is rejected by the vocabulary rule.

A claim with no scenario bound to it is reported as `unwitnessed`. It is not reported as false, because
a claim can be true and simply untested.

### When a claim test fails

A failed claim test has three causes, and the run names the one it saw. The code is wrong. The claim
test is wrong. The environment is wrong, because a fixture broke, a dependency moved, or a service was
not up. The third is not a finding about the project, and reporting it as one teaches a reader to
ignore the report.

The first two are the triage that someone acts on, and the run cannot tell them apart. It presents both
readings with the evidence, and a person or an agent chooses between them.

One pattern is worth stopping a merge for. When a change set edits a claim test and the code that the
test binds to, and the claim text did not move, the change has the shape of a test repaired to match
broken code. That is the justification comment moved into a test file, so the run reports it as a
decision that a person must confirm.

## Judge tasks

Three bounded tasks use a model. The first two compare prose against claims that already passed, and
the third compares the document tree against itself. The judge tests nothing, reads no evidence, and
proposes no change to code or to prose. It returns a verdict and the spans that justify it.

The deterministic stage decides whether a claim holds. Only claims with admissible evidence reach the
document, so the judge never sees a claim that no test supports.

| Task | Input | Output |
| --- | --- | --- |
| S1 coverage | One prose span and the claims selected for its document | The claims that the span does not mention, and the assertions in the span that no claim states |
| S2 contradiction | One prose span and the claims that it mentions | The pair of spans that disagree, with the claim id, or `none` |
| S3 focus | The whole index | Pairs of document nodes whose audience and question ask the same thing |

S1 runs in both directions because the two directions fail differently. A claim that no prose mentions
is dead weight that no reader will meet. Prose that no claim states is unverified text, and it is the
common case, because writing a sentence is cheaper than writing a test.

S2 catches the case where prose and claims are both present and disagree. The hedge case is one of
them: a claim states a rule flatly and the prose softens it to `usually`.

Every span carries an id that the engine assigned when it generated the document, so a finding routes
back to the sidecar slot that owns it. A verdict that cites a span id the engine never issued is
discarded rather than trusted, which matters because the author of the prose and the judge are often the
same model family and will agree for the same wrong reason.

Cost is bounded before the model is called. Claims and prose spans in a document are already paired by
the selection in the index, so the judge reads one document at a time rather than a cross product of
the claim graph.

An earlier draft asked the model whether a scenario would fail if a claim were false, and whether any
code could contradict a claim. The first is a question about the strength of a test rather than about the
sentence, which makes it correctness testing of the system, and the second asks the model to invent
something. Both were dropped.

### Verdict pins and staleness

A verdict carries the hash of the document that the model read, plus the model id and the prompt
version, and those three values are its identity. The document hash covers the claim text, the resolved
signature, the evidence, and the outcomes in one value. A stored document is read as history only, so a
verdict stays replayable while the current tree moves on. A verdict about a claim that no longer exists
is the out-of-date-document failure rebuilt one layer up, so staleness is checked before a verdict is
read.

Each task runs `k` samples, and disagreement between samples is reported as `ambiguous` rather than
resolved by majority. Disagreement is the cheapest available detector for a claim that means two
things, and majority voting would hide it.

### What may block a merge

| Check | Blocks |
| --- | --- |
| Referent resolution, derivability, hedging, identity, tree obligation | Yes |
| The obligation generated from a claim fails, or its scenario is inadmissible | Yes |
| A claim test and its bound code edited together while the claim text stayed put | Yes |
| An S1 or S2 verdict | No, unless the prose author and the claim author are different identities |

A judge verdict that is wrong becomes proof once a reader relies on it, and readers stop looking
afterwards. The deterministic checks do not depend on what anyone believes, so they block. The judge is
the convenience layer, so it reports.

## Engine and plugin boundary

The split follows autofuzz for one reason: resolving a referent and deciding what is derivable both need
the language, and the engine must not acquire the language knowledge in order to do bookkeeping.

The engine owns the index, the claim graph, identity and deduplication, the tree obligations, the two
artifact stages, the judge, the verdict cache, rendering, and the report. A plugin owns resolution,
derivability, and scenario execution. One plugin may answer methods for both stages, and the
judge is engine-side rather than a plugin, because the model's task is the same whether the claim is
about TypeScript or Rust.

```ts
export interface VeridocPlugin {
  handshake(): Promise<{
    protocolVersion: string;             // "vd/1"
    name: string;
    version: string;
    languages: string[];
    capabilities: Capability[];          // "resolution" | "derivability" | "execution"
  }>;

  /** Resolve declared references. An unresolved reference is a normal answer with a reason. */
  resolve(req: { refs: string[] }): Promise<{
    resolved: Array<{ ref: string; referentId: string; signature: string }>;
    unresolved: Array<{ ref: string; reason: string }>;
  }>;

  /** The facts about one referent that a reader can already see in the declaration. */
  derivable(req: { referentId: string }): Promise<{ facts: string[] }>;

  /** Run one scenario against the current build. */
  run(req: { scenarioId: string; deadlineMs: number }): Promise<{
    outcome: "passed" | "failed" | "errored" | "timed-out";
    detail?: string;
  }>;

  /** The declared surface, so the run can report symbols that no claim covers. */
  surface(): Promise<{ referents: Array<{ referentId: string; name: string; public: boolean }> }>;
}
```

| Capability | What the engine does without it |
| --- | --- |
| `resolution` | Nothing works. A plugin without it cannot be used. |
| `derivability` | The derivability ban is inert and is reported as inert. |
| `execution` | Scenarios are parsed and bound but never run, so every claim is `unwitnessed`. |

A missing capability is reported, never silently treated as a pass.

## Two artifact stages

Two analyses run over the same claims, and they need opposite representations. A program reads a
structure best, so the deterministic stage consumes the sidecar through a plugin. A model reads prose
best, so the semantic stage consumes a generated document written for a reader.

The deterministic stage runs first and, when it passes, generates that document. The document holds the
claim text, the resolved signature in words, the evidence, and the outcomes. Its markup belongs to the
engine and is not part of any contract.

The semantic stage reads only the document. It never calls a plugin, and it must run on a machine with
no language toolchain installed, because that requirement is the test of whether the stage boundary is
real. A stage that reaches back for a referent has no boundary.

The document is an output and never an input. The run keeps the document that produced a verdict beside
that verdict, so the verdict can be replayed. A stored document is read as history rather than as input
to a new run.

[Decision 0003](decisions/0003-the-semantic-stage-reads-a-document-not-code.md) records why the two
stages do not share one representation.

## The report

A run writes `.veridoc/<runId>/veridoc.jsonl`, one record per line, with a final `finished` record that
holds the complete document, and `.veridoc/<runId>/report.md` for the reader.

`report.md` is ordered by what a reader should do, not by object type:

1. Failed claim tests, split by the three causes, with the cases where a test and its bound code moved
   together listed first.
2. Uncovered claims, that is claims with no admissible evidence, grouped by concept.
3. Prose that no claim states, and claims that no prose mentions, from S1.
4. Prose that contradicts a claim, from S2.
5. Ambiguous spans, from judge sample disagreement.
6. The residue, that is claims that lower to nothing, with the reason for each.
7. Rejected-as-derivable claims, with the fact that made each one redundant.
8. Stale verdicts that were re-derived during this run.
9. Symbols on the declared surface that no claim covers.

| Metric | Definition |
| --- | --- |
| Coverage | Claims with admissible evidence, divided by claims, split by kind |
| Unmatched prose | Prose spans that no claim states, as a count and a share of prose |
| Residue | Claims with `lowersTo: none`, as a count and a share |
| Redundancy | Claims rejected as derivable, as a count |
| Ambiguity | Prose spans with an `ambiguous` verdict, as a count |
| Obligation | Declared tree nodes satisfied, divided by nodes that are not `notApplicable` |

## What veridoc does not do

- It does not show that a claim is true. It shows that an obligation was generated from the claim, that
  the obligation ran, and that it passed.
- It does not accept a live system as evidence. A scenario that reaches a running service is marked
  inadmissible rather than trusted.
- It does not write prose. There is no authoring mode, because the author is expected to be an agent
  working from the checker's output.
- It does not compare two implementations. A claim has one subject.
- It does not maintain a claim across a rename automatically. The plugin resolves a reference, and an
  unresolved reference is an error that a human or an agent resolves deliberately.
- It does not carry claims across projects. A claim graph is one repository.
- It does not ask the model to propose a change to code or to prose. The judge compares prose with
  claims that already passed and returns a verdict about what it was given.
- It does not audit whether a test is strong enough. That is correctness testing of the system and it
  belongs to a different tool. veridoc generates the obligation from the claim, runs it, and stops.

## Open questions

These are unresolved in v1 and need a pilot project to settle.

1. **Claim granularity.** One subject and one predicate is enforceable, but nothing yet stops an agent
   writing a predicate so broad that it covers several assertions. A pilot needs to show whether the
   predicate slug carries enough discipline on its own.
2. **Evidence format per claim kind.** A contract on one symbol, a generated-input property test, a
   boundary scenario, a recorded artefact, and a model-based trace each suit a different claim kind, and
   one format serves none of them well. v1 specifies the scenario form only. The open work is the
   normalised evidence record that lets one judge read all five, and the rule that lets the claim kind
   select the format so an author does not choose the easiest one.
3. **Where `term` claims get their discipline.** A declared concept has a stable id, so a `term` claim
   has an identity, but nothing can contradict a definition. Whether the residue grows without bound
   around `term` is the main risk in the design.
4. **One author identity or several.** The policy in [Judge tasks](#judge-tasks) about what may block a
   merge depends on knowing whether the author of the prose and the author of the claims are the same
   identity, and nothing in v1 records authorship. A pilot needs a convention before the policy can be
   enforced rather than merely described.
