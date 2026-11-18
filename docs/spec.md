# veridoc specification, v1 (draft)

This document gives the v1 design for someone who will implement veridoc or decide whether an idea
belongs in it. It describes intended behaviour, not current behaviour, because no part of it is built
yet. Where a rule exists to stop a specific failure, the failure sits next to it.

## Contents

- [The problem this solves](#the-problem-this-solves)
- [The five objects](#the-five-objects)
- [Two graphs that never merge](#two-graphs-that-never-merge)
- [Project layout](#project-layout)
- [Claims](#claims)
- [Assertions and bivalence](#assertions-and-bivalence)
- [The document tree](#the-document-tree)
- [Scenarios](#scenarios)
- [Evidence and outcomes](#evidence-and-outcomes)
- [Provenance citations](#provenance-citations)
- [Judge tasks](#judge-tasks)
- [Two artifact stages](#two-artifact-stages)
- [Engine and plugin boundary](#engine-and-plugin-boundary)
- [The report](#the-report)
- [What veridoc does not do](#what-veridoc-does-not-do)
- [Open questions](#open-questions)

## The problem this solves

Documentation written by an agent fails in ways that documentation written by a human also fails, but
faster and at a volume that makes the failures dominant. Each failure below is the reason for one rule
later in this document.

| Failure | Cause | Rule that targets it |
| --- | --- | --- |
| A document describes a symbol that no longer exists | The prose has no address | Referents are resolved, not written as strings |
| The same fact appears in five places and the copies disagree | Nothing says which copy owns the fact | Claim identity rejects a second claim with the same subject and predicate |
| Prose explains why a shortcut was acceptable | The writer had a free-text slot | No top-level prose slot exists, and `because` requires a typed citation |
| Prose hedges instead of committing | Normative and observational language share one slot | A semantic lint rule compares the sentence with the modality of its kind |
| A document cites evidence that nobody can re-run | The claim has no executable witness | A `witness` claim resolves an artefact and its recorded hash |
| Prose restates the signature | Nothing caps how much a document may contain | A claim that a plugin can derive from declarations is rejected |
| Documents multiply without anyone deciding to add one | Nothing closes the set of documents | The index closes the tree and obligates every node |
| A sentence is written that nothing evaluates | Coverage was a metric rather than a rule | Every claim ends in an outcome or in a build error |

One failure is not addressed by any rule here: whether a well-formed claim actually means what the author
intended. [Judge tasks](#judge-tasks) reduces that failure and does not remove it.

## The five objects

| Object | What it is | What identifies it |
| --- | --- | --- |
| Referent | A reference to one symbol, boundary, or declared concept, resolved by the plugin | A stable id that the plugin derives from the declaration, not from a file position |
| Claim | One atomic assertion about one subject, of a declared kind, with an assertion tree or a citation | `referentId#predicate`, stored as a hash |
| Scenario | A declared world and one action, bound to the claim's referent | Its file and scenario id |
| Document node | One declared element of the document tree, with an audience and one question | Its path in the tree |
| Verdict | A pinned judge answer about one section against the claims it names | `sha(sectionHash, claimSetHash, referentRevision, model, promptVersion)` |

A claim is atomic in one specific sense: it has one subject and one predicate. A sentence that describes
two behaviours is two claims. This is what makes deduplication possible, and it is the rule that an agent
author will break most often, so the checker enforces it by requiring one referent in the `subject` field
and rejecting a predicate that matches more than one assertion slug.

## Two graphs that never merge

veridoc maintains a claim graph and a document tree, and it never writes prose into the tree.

The claim graph is organised by concept. A claim exists once, and its identity is its subject and its
predicate. The document tree is organised by audience. A document node orders the sections, and each
section names the claims its prose is about.

The reason for the split is the duplication failure. Facts do not follow the file layout of an
implementation, and they do not follow the reading order of a reader, so a layout that keeps facts in one
of those shapes makes a reader copy them elsewhere. An author who needs a fact in two documents edits one
claim and names it in two sections.

Prose is written by hand, in `.spec.md` files, and the engine never writes it. The engine resolves every
`<doc-claim>` in a section to the claim it names, and it reports every section that names no claim. Where
prose asks for a value the code decides, a `<doc-each>` placeholder is filled at assembly, in the output
and never in the authored file. A section and a named claim that describe the same behaviour and disagree
are an S2 finding.

## Project layout

```
project/
  index.doc.<ext>                   the document tree: closed, obligating
  src/
    scanner.<ext>                   the code
    scanner.doc.<ext>               claims about scanner.<ext>
  docs/
    architecture.spec.md          authored prose in keyed sections
    harness-authoring.spec.md
  features/
    <concept>.feature             scenarios: a world and an action
    <concept>.obs.<ext>             declared observables and actions
  .veridoc/
    config.json                   coverage requirement, plugin command, run deadline, judge, output formatters, citation stores
    verdicts/*.json               the semantic cache, each entry holding the brief that produced the verdict
    result.json                   the result document every formatter consumes
    last-result-hash              the hash that lets an unchanged run skip the formatters
    out/<formatter>/              where the formatters write
```

`<ext>` is the extension of the project's own source language. The host names no language and no extension: it
asks a plugin to collect, and the plugin decides which files of its own language carry the index, the claims, and
the vocabulary. A layout rule that names `.ts` is a rule a project in any other language cannot satisfy.

A claim lives in the sidecar beside the file that holds its subject, which keeps documentation out of the
code and keeps review local. A concept is a field on the claim rather than a directory, so the same
deduplication rule works across files without a claims directory. A `.spec.md` file is ordinary markdown
that a person or an agent edits, with a `name` on every section naming the subject the section covers, so
the engine can show which claims that section covers.

## Claims

A claim is a call to a typed builder, so a claim that points at a renamed symbol stops compiling.

```ts
import { claim, ref, scenario, fixture } from "@veridoc/claims";

claim.ensures({
  subject: ref(parseConfig),                 // resolved by the plugin, not a string
  concept: "project-scan",
  predicate: "rejects-missing-project-root",
  text: "A missing project root is refused with a diagnostic that names the searched path.",
  scenario: scenario("config/rejects-missing-root"),
  asserts: {
    all: [
      { obs: "exit.code",       is: equals(2) },
      { obs: "stderr.text",     is: contains(fixture.searchedPath) },
      { obs: "writtenFiles",    is: satisfies((f: string[]) => f.length === 0) },
    ],
  },
});
```

### Kinds

| Kind | Modality | Channel | Shown to be false by |
| --- | --- | --- | --- |
| `requires` | Normative | scenario, or judge | An action that satisfies the precondition and breaks the postcondition |
| `ensures` | Normative | scenario, or judge | An action whose observed values disagree with the assertion tree |
| `invariant` | Normative | scenario over a declared generator, or judge | A reachable state that violates the property |
| `witness` | Observational | artefact | An artefact that no longer resolves, or no longer matches its recorded hash |
| `because` | Provenance | citation | A reference that no longer resolves, or cited text that does not support the reason |

Every kind carries `subject`, `concept`, `predicate`, `text`, and exactly one channel field: `scenario`
with `asserts`, `artefact`, or `citation`. A claim with two channels, or with none, is a bind error.

The distinction the kinds carry is normative against observational, and nothing else. A rule and a report
about the same symbol can both be true, and a reader needs to know which one a sentence is.

### Subject when there is no symbol

`because` and a claim about architecture often describe a concept rather than a symbol, and a claim needs a
subject to have an identity. A declared concept in the index is therefore a first-class subject. The index
declares concepts alongside document nodes, and a concept has a stable id that no source edit changes.

### Identity and the one-owner rule

Claim identity is the author's subject name joined with the predicate slug, and the store keeps that pair
as a fixed-length hash. The readable pair is the claim's name wherever a person or an agent reads or types
it: the result document, the report, the leaf citations a verdict makes, the argument to `veridoc witness`,
and the `name` of the `<doc-section>` that covers it.
The hash is a value, not a name, and it appears in two places only, as a suffix on both claims when two of
them slug alike, and in the names of cache files that no one reads. A second claim with the same identity is
an error that reports the path of the existing owner. The author then edits the owner or chooses a different
predicate, which is the behaviour that a duplication-prone author will not choose on their own.

A repeat of an existing claim in a second document is not a second claim. It is a second `<doc-claim>`
naming the same claim, and the claim stays one fact.

### The derivability ban

A claim is rejected as redundant when the plugin can derive its content from declarations. The v1 derivable
set is deliberately narrow, because a checker that over-rejects teaches the author to disable it:

- the parameter list and the return type of the subject;
- whether the subject is exported, and from which module;
- the members of an enum or a union that the subject accepts;
- the existence of an explicit raise site in the subject's body.

A claim that says only one of those things adds nothing that a reader cannot read off the declaration, and
it is one more sentence that goes stale when the declaration changes. The same test appears again in
[Assertions and bivalence](#assertions-and-bivalence), because an assertion that cannot fail is the same
error written in the other direction.

### Hedging

Hedging is a semantic lint rule, not a word list. `currently` inside a `witness` claim is honest, and `the
value is generally lowercase` inside an `ensures` claim hedges without using any word that a list would
catch. A sentence whose force is not backed by a decidable consequence is flagged, by comparison of the
claim text with the modality of its kind. See [Semantic linting](#semantic-linting).

### The residue

The residue is the set of claims that nothing in any channel reaches: a `because` claim whose citation
resolves but whose cited text is unreadable. The count is the headline number of a run, because it is the
share of the documentation that rests on nothing checkable. Prose about facts from outside the repository is
the permanent occupant of this set. A residue that grows with no external cause means the vocabulary is too
narrow, not that the project needs more prose.

## Assertions and bivalence

An assertion tree states, in data, what must hold over the values that a scenario produces. The engine compares
those values with the tree and produces the outcome, so there is no path by which an author reports a pass.

```ts
asserts: {
  all: [
    { obs: "thrown.name",    is: equals("RangeError") },
    { obs: "thrown.message", is: contains(fixture.limit) },
    { obs: "writtenFiles",   is: satisfies((f: string[]) => f.length === 0) },
  ],
}
```

`all`, `any`, and `not` compose, so a claim can assert a conjunction, a disjunction, or a negation over
several observables. Expected values come from constants or from the fixture, which is what lets an
assertion depend on the world rather than on a literal. `satisfies(fn)` is a typed function over one
observable, and it is where real logic lives.

### Bivalence

An assertion is bivalent when both outcomes are attainable over the declared type of its observables. An
assertion that cannot fail is not evidence, so bivalence is required before a claim may take the scenario
channel.

| Assertion | How bivalence is settled | Cost to the author |
| --- | --- | --- |
| A structured node such as `equals` or `contains` | By construction, checked against the declared type | Nothing |
| Code inside the shape below | By static analysis in the plugin | Nothing |
| Code outside that shape | Not provable | Supply one value the predicate accepts and one it rejects, which the engine re-evaluates |
| Neither | Not bivalent | Take the judge channel instead, or the claim is an error |

The shape that static analysis accepts is deliberately small: one `return` of a boolean expression, with no
statements, no loops, no early returns, and no calls outside a declared allow-list, and every leaf a
comparison between a projection of a declared observable and a constant or a fixture value. Within that
shape the analysis is constant folding, tautology rules, and enumeration over the declared types. It
rejects `x => true`, `typeof x === "string" || true`, `x => x === x`, and `s => s.length >= 0`, which has
no rejecting value in the domain of lengths.

Two limits are on the record. A tautology that needs number ordering and NaN semantics, such as `x > 0 ||
x <= 0 || x !== x`, survives unless the analysis carries those theories. And an assertion that is bivalent
but tests the wrong thing survives every static method available, because relevance is not a syntactic
property; the pair of witnesses a non-conforming leaf requires is visible in the diff, and the judge
compares them against the sentence.

### The claim's assertion is not the sentence

A weak assertion tree that satisfies a rich sentence is a documentation error rather than a harness error,
because the gap is between `text` and `asserts`. That gap is the comparison that the judge already makes, and
it is the reason that the tree stays structured rather than becoming one opaque function: a judge reasons far better
about a sentence beside a list of observables and checks than about a sentence beside a body of code.

## The document tree

The index declares the concepts, the document nodes, and the obligation each node carries.

```ts
import { tree } from "@veridoc/tree";

export default tree({
  concepts: ["project-scan", "harness-contract"],
  documents: [
    tree.doc({
      path: "docs/harness-authoring.spec.md",
      audience: "an author writing a new plugin",
      question: "What must my plugin answer, and in what order?",
    }),
    tree.doc({
      path: "docs/AGENTS.spec.md",
      audience: "an agent working in this repository",
      question: "Which rules bind every change here?",
    }),
  ],
});
```

### Closed and obligating

The index closes the tree: a document that the index does not declare cannot be written, and a `.spec.md`
file that no node declares is an error.

The index also obligates the tree. Every declared node must be satisfied or declared `notApplicable` with a
reason, and a new `notApplicable` fails the run until the reason is written down. Adding a node is a diff to
the index, which is the one place in the system where a human decision about scope is unavoidable and
visible.

### Sections

A section opens with `<doc-section name="SUBJECT">` on a line of its own and ends at the next opener or the
end of the file. Its `name` is the claim subject the prose is about. A `<doc-claim name="PREDICATE"/>`
written inside a sentence names one claim, and the claim it names is `SUBJECT#PREDICATE`, the readable name
the engine prints for the claim. Both directions are checked: a reference that names no claim stops the
build, and so does a claim that no section names. A section that names no claim is reported, and that report
is the deterministic half of what would otherwise be an S1 finding. A heading carries no meaning to the
engine, so an author uses headings inside a section wherever the prose wants one, and an element inside a
fenced code block is inert, so a document can show this grammar without using it. The one exception is the
block a `<doc-each>` repeats below: a placeholder inside that block is live, because a table of values is
exactly the thing an author must not be asked to copy.

### Injection

A fact the code decides has no place in prose that a person writes: the author copies it, and nothing
re-checks the copy (`docs/decisions/0011-an-authored-placeholder-pulls-its-content-at-assembly.md`). A
`<doc-each source="COLLECTION">` on a line of its own asks for that fact, and the one block that follows
it is the block that repeats. `<doc-each source="observables">` followed by
`- <doc-value field="name"/> is a <doc-value field="type"/>` becomes one list item per declared
observable. `<doc-value field="…"/>` marks where a value lands, and a bare `<doc-value/>` takes the item's
own name where the collection names its items.

The values come from the tree the build already holds, so injection asks nothing new of a plugin. The
source names one collection: `claims` (the claims that section names), `concepts`, `documents`,
`observables`, `actions`, or `fixtures`. A field is one of that collection's own: `id`, `subject`,
`predicate`, `concept`, `text`, `kind`, `referent`, `signature`, `file`, `line` or `channel` on `claims`;
`name` on `concepts` and `fixtures`; `path`, `audience` or `question` on `documents`; `name` or `type` on
`observables`; `name`, `calls` or `produces` on `actions`.

The authored file keeps the placeholder, because the engine never writes into a `.spec.md` file. The
assembled body, the one the brief carries and the section hash covers, is where the values appear, so
gathered content that changes invalidates the verdicts of every section that pulled it in and the report
names the element that did it. A source that names no collection, a collection that matches nothing, a
field the collection does not have, an opener with no block after it, and a `<doc-value>` outside any
block each stop the build: an empty render in the middle of a paragraph reads as a section that is
finished. Injected content is not a claim; the fact stays a claim in the sidecar, and the prose that
depends on it names it with a `<doc-claim>`.

### Focus

Every node carries one audience and one question. The S3 judge task compares the (audience, question) pairs
of all nodes and reports pairs that ask the same thing. This is the only mechanical handle on unfocused
growth, and it runs against the index, which is small enough to judge on every change.

## Scenarios

A scenario is a declared world and one action. It has no assertion slot, because the claim owns the
assertion, and a scenario that could assert anything is a scenario that could assert nothing.

```gherkin
Feature: project scan

  Scenario: A missing project root is refused
    Given the empty-directory fixture
    When afz probe-plugin runs in that directory
```

`Given` refers to a fixture from the declared set. `When` refers to a declared action, and that action must
resolve to a call of the claim's own referent, which the plugin checks at bind time. A scenario bound to a
claim whose action calls something else is an error before anything runs, and so is an assertion that names
an observable the action cannot produce.

### The vocabulary is closed

Observables and actions are declared once per project, with types, and a scenario can only refer to what is
declared. This is where slop used to collect, because a step defined as `the response should be correct`
can be written to accept anything; with no step bodies there is nothing left to write that way, and the
constraint moves to the declaration of each observable, which is a reviewed diff.

### Determinism

Evidence has to be re-runnable by someone who did not write it, which is failure six, and three static rules
settle it. The `Given` world refers to a fixture from the declared set, and a scenario that refers to no
fixture is not evidence. Every observable that an assertion reads is declared, and an undeclared observable
stops the build. A scenario that refers to anything outside those two sets is not evidence.

Nothing is detected at run time, because there is nothing for a detector to find: an assertion can only read
declared observables.

## Evidence and outcomes

Every claim ends in one of two outcomes, and the channel that produced it is recorded beside it.

| Outcome | Meaning |
| --- | --- |
| `supported` | The channel evaluated the claim and agreed with it |
| `contradicted` | The channel evaluated the claim and disagreed, so the sentence is wrong |

A channel that was asked and did not answer is a run failure: the run stops, the build is red, and no claim
is given an outcome.

Coverage is claims in state `supported` divided by claims, split by channel. A project sets a required
coverage figure in `.veridoc/config.json` and the run compares the achieved figure against it, so adoption
starts low and the figure only has to rise.

### Every path has a meaning

The build is the point of the system, so no path is allowed to end in silence. Each of these is an error
that names the missing thing.

| Path | Meaning |
| --- | --- |
| A referent does not resolve | The claim names something that is not there |
| A claim has two channels, or none | The claim does not say how it can be checked |
| A scenario-channel claim has no scenario | A checkable claim was left unevaluated |
| A judge-channel claim has no verdict | A semantic claim was left unevaluated |
| A scenario bound to a claim calls a different referent | The evidence is about other code |
| An assertion reads an observable the action cannot produce | The evidence cannot answer the claim |
| An assertion cannot fail | The evidence is not evidence |
| The build, the plugin, or a fixture fails | The run is red, recorded once at scenario level |
| A scenario outlives its deadline | The run is red; a slow machine is not blamed on the prose |

A claim outcome is never produced by any row in that table. A broken fixture is a run failure and not a
finding about the documentation, and it is not written as though one claim were weaker than the others,
because one broken build would then print thousands of unevidenced claims.

### When a claim is contradicted

A contradicted claim has two readings and the run cannot separate them. The code is wrong and the sentence is
right. The scenario or the assertion is wrong and the code is right. Both readings are presented with the values that
caused the disagreement, and a person or an agent chooses between them.

This is the moment the whole design exists for. A sentence that no evidence contradicts costs nothing to
leave wrong, so the contradiction is the one point where documentation has to become correct.

### Propagation

Evidence moves with what it describes, in both directions. A change that edits claim text, or the prose of a
section that a claim supports, also edits the scenario that supports it or the code that the scenario binds
to. A change that edits code which a scenario binds to also edits the scenario, or the claim and the prose
that the scenario supports.

A change set that moves one side on its own is reported as a decision that a person must confirm, because
that shape is also what repaired evidence looks like: the scenario or the assertion edited to match broken
code, with the claim left alone. The honest case and the repaired one are not separable from the diff alone,
which is why the run stops for a person.

## Provenance citations

A `because` claim carries a typed reference rather than a sentence about where a decision was discussed. A
bare URL or the phrase "discussed in the team channel" is a bind error.

```ts
claim.because({
  subject: ref(projectScan),
  concept: "project-scan",
  predicate: "walks-upward-before-failing",
  text: "The scan walks upward because a nested invocation must reuse the root it found first.",
  citation: { store: "session", id: "01a1e512", turn: 12 },
});
```

Stores are declared in `.veridoc/config.json` and resolved by a plugin. The deterministic half is that the
reference resolves: a git object exists and is an ancestor of the revision under review, a ticket exists and
is in a terminal state, which catches a reason that cites an open ticket for a decision already shipped, and
a conversation turn exists and returns its text. The remaining half, whether the cited text supports the
stated reason, is a judge task, and it is the same comparison the judge already makes, because a cited turn
is verbatim text.

A citation that names an undeclared store is an error. A declared store that cannot be reached is a run
failure, not a soft pass.

## Judge tasks

Three bounded tasks use a model in v1. The first two compare prose against claims whose outcome was
`supported`, and the third compares the document tree against itself. The judge tests nothing, reads no
code, and proposes no change to code or to prose. It returns a verdict and the spans that justify it.

Whether a claim holds is settled by the deterministic stage where a channel exists, and the outcome travels
with it into the brief, so the judge reads the result rather than guessing at it.

| Task | Input | Output |
| --- | --- | --- |
| S1 coverage | One section and the claims it names | The claims the section does not mention, and the assertions in the section that no claim states |
| S2 contradiction | One section and the claims that it mentions | The pair of spans that disagree, with the claim id, or `none` |
| S3 focus | The whole index | Pairs of document nodes whose audience and question ask the same thing |

S1 runs in both directions because the two directions fail differently. A claim that no section names is
dead weight that no reader will meet, and the build stops on it without a model. Prose that no
claim states is unverified text, and that half needs the model, because writing a sentence is cheaper than
writing a claim.

S2 catches the case where prose and claims are both present and disagree. The hedge case is one of them: a
claim states a rule flatly and the prose softens it to `usually`.

Every span carries an id that the engine assigned when it assembled the brief, so a finding routes back to
the section that owns it. A verdict that cites a span id the engine never issued is a run failure rather than
a soft pass, which matters because the author of the prose and the judge are often the same model family and
will agree for the same wrong reason.

Cost is bounded before the model is called. Sections and claims are already paired by the references the
author wrote, so the judge reads one section at a time rather than a cross product of the claim graph.

An earlier draft asked the model whether a scenario would fail if a claim were false, and whether any code
could contradict a claim. The first is a question about the strength of a test rather than about the
sentence, which makes it correctness testing of the system, and the second asks the model to invent
something. Both were dropped.

### Semantic linting

S1 and S2 are two rules from a family, and the family is open. Each rule consists of an id, the object that
the rule is run over (one claim, one section with the claims it names, or the whole index), its modality,
and the violation, which is recorded with the span that carries it. A rule returns a verdict and never a
change to code or to prose, so the rule that the judge proposes nothing holds for every rule in the
registry.

The rules in this document are built into the engine. Rules are also added by a plugin through the same
record, which is how an in-house convention is enforced, such as every `ensures` claim about an exported
function stating the diagnostic that a caller sees, without a fork of the engine.

A deterministic rule runs on every build and blocks. A semantic rule runs in the semantic stage, is cached
by the key below, and is recorded as a finding. An S2 contradiction whose two spans re-verify blocks; every
other semantic finding reports.

### The verdict cache

Semantic rules are the only expensive checks in a build, so a verdict is stored against the inputs that
produced it: the hash of one section, the hash of the claims it names, the referent revision, the model
id, and the prompt version. The key is per section, so editing one sentence does not re-judge a file. A
change to any part of the key is a miss, and the rule runs again on the next build, which is what would have
happened with no cache at all. Caching changes the cost of a build and never its verdict, and a miss is
reported so that a reader can see the work that the run did.

### What may block a merge

| Check | Blocks |
| --- | --- |
| Referent resolution, derivability, identity, tree obligation, bivalence, channel completeness | Yes |
| A claim in state `contradicted` | Yes |
| Evidence did not move with the prose, or prose did not move with the code | Yes |
| A deterministic lint rule | Yes |
| Achieved coverage below the configured figure | Yes |
| An S2 pair whose two span ids re-verify | Yes |
| An S1 finding, or a citation whose state is not terminal | No, reported as a warning |

A judge verdict that is wrong becomes proof once a reader relies on it, and readers stop looking afterwards.
The deterministic checks do not depend on what anyone believes, so they block. An S2 contradiction whose two
spans re-verify blocks, because publishing a sentence the claims deny is the failure the project exists to
prevent. A judge transport error, a timeout, an unusable verdict, or a verdict whose spans cannot be located
is a run failure: a check that did not run makes the build red rather than green.

## Two artifact stages

Two analyses run over the same claims, and they need opposite representations. A program reads a structure
best, so the deterministic stage consumes the sidecar through a plugin. A model reads prose best, so the
semantic stage consumes a generated brief written for a reader.

The brief is not a `.spec.md` file. A `.spec.md` file is written by a person or an agent and is an input.
The brief is put together by the engine from the written sections, the claim text, the resolved signature in
words, the assertion tree, and the outcomes, and it is discarded afterwards. Its markup belongs to the engine
and is not part of any contract.

The deterministic stage runs first and, when it passes, assembles the brief. The semantic stage reads only
the brief. It never calls a plugin, and it must run on a machine with no language toolchain installed,
because that requirement is the test of whether the stage boundary is real. A stage that reaches back for a
referent has no boundary.

The brief is an output and never an input. The run keeps the brief that produced a verdict beside that
verdict, so the verdict can be replayed. A stored brief is read as history rather than as input to a new
run.

[Decision 0003](decisions/0003-the-semantic-stage-reads-a-brief-not-code.md) records why the two stages do
not share one representation.

## Engine and plugin boundary

The split follows autofuzz for one reason: resolving a referent, deciding what is derivable, and judging
whether an assertion can fail all need the language, and the engine must not acquire the language knowledge
in order to do bookkeeping.

The engine owns the index, the claim graph, identity and deduplication, the tree obligations, the two
artifact stages, the judge, the verdict cache, injection, and the report. A plugin owns resolution,
derivability, bivalence, execution, and citation stores. One plugin may answer methods for all of them, and
the judge is engine-side rather than a plugin, because the model's task is the same whether the claim is
about TypeScript or Rust.

```ts
export interface VeridocPlugin {
  handshake(): Promise<{
    protocolVersion: string;             // "veridoc/1"
    name: string;
    version: string;
    languages: string[];
    capabilities: Capability[];          // "collect" | "resolution" | "derivability" | "bivalence" | "execution" | "citation" | "surface"
  }>;

  /** Read the index, every claim sidecar, and every vocabulary sidecar, and report what was declared. */
  collect(req: { root: string }): Promise<{
    concepts: string[];
    documents: Array<{ path: string; audience: string; question: string; notApplicable?: string }>;
    claims: RawClaim[];
    observables: Array<{ name: string; type: string }>;
    actions: Array<{ name: string; calls: RefToken; produces: string[] }>;
    fixtures: string[];
    skipped: Array<{ file: string; reason: string }>;
  }>;

  resolve(req: { refs: string[] }): Promise<{
    resolved: Array<{ ref: string; referentId: string; signature: string }>;
    unresolved: Array<{ ref: string; reason: string }>;
  }>;

  derivable(req: { referentId: string }): Promise<{ facts: string[] }>;

  bivalence(req: { claimId: string }): Promise<{
    bivalent: boolean;
    method: "structural" | "static-analysis" | "witnesses";
    witnessAccepts?: boolean;
    witnessRejects?: boolean;
    reason?: string;
  }>;

  run(req: {
    scenarioId: string; fixture: string; action: string; args: string[];
    deadlineMs: number; leaves: Array<{ leafId: string }>;
  }): Promise<{
    ok: boolean;
    observed: Array<{ observable: string; value: string }>;
    satisfied: Array<{ leafId: string; ok: boolean }>;
    failure?: { kind: "build" | "fixture" | "deadline" | "plugin"; detail: string };
  }>;

  cite(req: { citations: Array<{ store: string; id: string; turn?: number }> }): Promise<{
    resolved: Array<{ id: string; text: string; state?: string }>;
    unresolved: Array<{ id: string; reason: string }>;
  }>;

  surface(): Promise<{ referents: Array<{ referentId: string; name: string; public: boolean }> }>;
}
```

| Capability | What the engine does without it |
| --- | --- |
| `collect` | Nothing works. Without the claim set there is no build. |
| `resolution` | Nothing works. A plugin without it cannot be used. |
| `derivability` | The derivability ban is inert and is reported as inert. |
| `bivalence` | No claim may take the scenario channel, so every checkable claim is an error. |
| `execution` | Scenarios are parsed and bound but never run, so every scenario-channel claim is an error. |
| `citation` | A `because` claim cannot resolve, so it is an error rather than a pass. |
| `surface` | The run cannot report uncovered public symbols, and says so. |

A missing capability is reported, never silently treated as a pass. A plugin that answers a leaf id the
engine never issued is a run failure, so the routing is typed rather than hoped for.

The same contract exists in machine-readable form in
[`veridoc-1.profile.json`](../veridoc-1.profile.json): every method with the schema of its request and result, each
capability with the sentence above, one loss reason for each way a plugin can go away, and each conformance
rule with the scripted defect that shows the rule can fail. `prpc-core`, the runner the pluginrpc mechanism
publishes, reads that file and starts each defect itself, so a rule's `verified` mark is a measurement taken
against the fake plugin this repository builds rather than this document's word for itself.

## The report

A failing run writes no artifact and prints `file:line:` diagnostics. A succeeding run writes
`.veridoc/result.json` and runs the output formatters the project configures.

Harness detail, including every scenario that could not be evaluated and the reason, is the run failure on
stderr. A reader of the diagnostics is deciding what to change in the documentation, not what to repair in a
fixture.

| Metric | Definition |
| --- | --- |
| Coverage | Claims in state `supported`, divided by claims on a checkable channel, split by channel, against the configured figure |
| Custom logic | `satisfies` leaves the plugin did not mark bivalent, divided by leaves |
| Unmatched prose | Prose that no named claim states, from S1, as a count |
| Residue | Claims in the residue, as a count and a share |
| Redundancy | Claims rejected as derivable, as a count |
| Obligation | Declared tree nodes satisfied, divided by nodes that are not `notApplicable` |

## What veridoc does not do

- It does not show that a claim is true. It shows that the channel bound to the claim evaluated it and
  agreed, and that the evidence exercises the symbol that the claim names.
- It does not put harness problems into the claim model. A fixture that broke is not a finding about the
  documentation and is not presented as one.
- It does not write prose. There is no authoring mode, because the author is expected to be an agent working
  from the checker's output.
- It does not compare two implementations. A claim has one subject.
- It does not maintain a claim across a rename automatically. The plugin resolves a reference, and an
  unresolved reference is an error that a human or an agent resolves deliberately.
- It does not carry claims across projects. A claim graph is one repository.
- It does not ask the model to propose a change to code or to prose. The judge compares prose with claims
  that already passed and returns a verdict about what it was given.
- It does not audit whether an assertion is strong enough to catch a future regression. That is correctness
  testing of the system and it belongs to a different tool. veridoc asks whether an assertion can fail at
  all, which is a weaker question and a decidable one.

## Open questions

These are unresolved in v1 and need a pilot project to settle.

1. **Claim granularity.** One subject and one predicate is enforceable, but nothing yet stops an agent
   writing a predicate so broad that it covers several assertions. A pilot needs to show whether the
   predicate slug carries enough discipline on its own.
2. **The reach of the accepted shape.** The static analysis decides bivalence inside a small expression
   shape. A pilot needs to show how often real assertions fall outside it, because every one that does costs
   a pair of witnesses or a judge verdict, and a high rate means the shape is too narrow rather than the
   project too hard.
3. **Judge cost against full coverage.** A required coverage figure makes the judge load-bearing for every
   sentence that no scenario reaches. The per-section key contains the cost of editing, and a pilot needs to
   show what a full re-judge of a mature documentation set costs in tokens and wall time.
4. **Ambiguity without sampling.** The only detector for a span that means two things is disagreement between
   samples, and the default of one sample disables it. Whether a second detector exists that does not double
   the cost is open.
5. **Where the coverage figure starts.** A figure of one hundred makes the first run against an existing
   documentation set red on every sentence written before veridoc. The ramp is a configuration value, and its
   starting point and step size need a pilot rather than an argument.
