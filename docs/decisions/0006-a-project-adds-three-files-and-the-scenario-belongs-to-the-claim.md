# 0006 — A project adds three files, and the scenario belongs to the claim

Status: proposed, 2026-11-18. Amended three times the same day: the sidecar became executable, the plugin for a
compiled language split into a library and an executable, and sharing lost its mechanism. No open decisions remain.
Implemented in the engine as of 2026-11-18: the readable claim name `hash::claim_name`, the three-file layout
at `docs/spec.md:81-108`, the per-claim scenario, and the per-claim run failure. The two plugins are being cut
over to the same contract.

## Context

`docs/spec.md:80-97` specifies the project layout, and an author who adopts veridoc today writes five
kinds of file, in the spelling of the plugin written first: `index.doc.<ext>`, `src/<module>.doc.<ext>`,
`src/<module>.obs.<ext>`, `features/<concept>.feature`,
and `docs/<name>.spec.md`. The harness the project hand-writes to answer the plugin is a sixth file the layout
section does not count.

The first, second, and fifth are the design the project started from. The third
and fourth arrived together, and both exist for one reason: the scenario is a separate object that the
engine parses from its own file (`feature.rs:68`, `bind.rs:424`) and that names the project's code by
string (`bind.rs:554`, `bind.rs:612`, `bind.rs:621`). A name forces a declaration, and the declaration
forced a file.

The measured return on those two files is three bind-time checks: a `Given` that names no declared fixture,
an assertion over an undeclared observable, and an assertion over an observable the chosen action does not
produce. They catch an authoring mistake on the author's own line instead of after the project has been
built. That benefit is real, and its size is bounded by a second rule: the first run failure aborts the
whole run (`execute.rs:38`, `execute.rs:85-105`), so one typo costs every other claim its verdict. I
observed both paths on the compiled-language example. A harness that would not compile produced one line and exit 2 with
no verdicts at all; a wrong artefact hash produced one `file:line` diagnostic and the other claims still
received verdicts.

Two further facts argue against keeping the current shape. Scenario reuse, the usual reason to give a
scenario its own identity and file, is not implemented: `execute.rs:50` issues one run per claim, so two
claims naming one scenario execute it twice. And the layout has already drifted from the specification:
`spec.md:90` places `.obs.<ext>` under `features/`, the plugin accepts it anywhere in the tree, and the
example project keeps it in `src/`.

The failure this record exists to prevent is a format that grows by addition. Each addition answers a local
problem, and nobody approves the total that an author has to learn.

## Decision

**A project adds three kinds of file.** `index.doc.<ext>` holds the document tree and the concepts.
`src/<module>.doc.<ext>` holds the claims about that module. `docs/<name>.spec.md` holds the prose. `.veridoc/`
is excluded from that count because it is the tool's own self-contained directory: configuration, the
verdict cache, and output, with a `.gitignore` of its own so the tool never edits the project's. The harness
entry point is counted with the code the project writes anyway, because it is one call into the plugin's crate.
Files that a scenario runs against, and artefact files that a witness claim pins, are project assets that the
project would keep regardless, so they are evidence rather than imposed format.

**The scenario belongs to the claim, and has no identity of its own.** The world, the call, the values it
returns, and the check are declared beside the claim. What the engine addresses at run time is the claim,
which already has an identity, its resolved subject joined with its predicate. The slug in
`features/<concept>.feature` existed so that a separate file could be referenced, and that file is going
away. The two step labels survive as plain strings, because a report that prints `Given an empty directory`
tells an author something that `config/rejects-missing` does not.

**The claim sidecar is executable code in every language.** A scenario that carries its world and its
call cannot live in a data file, so the sidecar is a source file that the plugin runs. The requirement on a plugin is
the same in either runtime model: one artifact answers the claim list and executes a claim's scenario, so there is no
second copy of the claim data for the two to disagree about. A language whose programs run on a host interpreter
answers it by loading the claim modules into that host and listing the claims they registered. A language that
compiles to a standalone binary answers it by building the claim modules into one executable that answers `--list`
with the claims and their scenarios as JSON and `--run <claim>` with the values that claim's scenario produced, and
the plugin's `collect` builds that executable and asks it. Each plugin's repository states how its language does
both.

**A plugin for a compiled language ships a library and an executable.** The library is a dev-dependency the project
adds and the executable is what veridoc launches. The library holds the authoring DSL, the scenario registry, and the
harness runtime, so a project's harness is one call into it instead of a hand-written stdin and stdout protocol with a
dispatch on action-name strings. The executable keeps the `prpc/1` server, the source scan that answers `resolve`, and
the orchestration that builds the harness and spawns it per claim. The split removes the friction that produced the
extra files: an executable that must do everything in isolation cannot reach code inside the project, so every fact
that needs project code was pushed into a file the project had to write and the plugin had to parse. A plugin for a
language that runs on a host interpreter needs no build step and ships the one executable.

The library and the executable agree on versions by reporting them. The harness answers its library version
beside `--list`, the plugin refuses one outside the range it speaks, and `veridoc install-plugin` already
pins the plugin by commit, so a skew is a refusal at collect rather than a claim set read through the wrong
protocol.

**Reuse needs no mechanism.** Two claims in one sidecar that need the same world call the same function in
that file, and two modules that need it reference a module that exports it, which is what a module system
exists for. veridoc adds no vocabulary to the index, which holds the document tree and the concepts and
nothing else, as `0001` decided. The only reuse case I could name, two sentences about one function needing
one world, is the local case, and ordinary code answers it.

**The assertion stays data that the engine evaluates.** This does not change, and it is the reason the
values a scenario produces must be declared by that scenario. `bind.rs:631` refuses `contains` over a
declared number; without that declaration the check evaluates to false and the report accuses the code of
failing a check that could never have passed.

**`features/` and `*.obs.<ext>` are removed.** `feature.rs` and `load_features` (`bind.rs:1019`) leave the
engine, and a claim's scenario arrives with the claim: its two step labels, its world and call as the plugin
resolves them, and the names and types of the values it returns. Two of the three bind-time checks keep working
against that per-scenario declaration: the assertion over an undeclared observable, and the assertion over an
observable the chosen action does not produce. The third, the `Given` that names no declared fixture, has no table
to check against once the registries are gone, and it becomes a run-time discovery.
Per pluginrpc `spec/spec.md:193-199` this is a profile change and needs no mechanism revision.

**The unit of work is the claim, and a run failure is one claim's result.** The design is incremental: a
claim's verdict depends on that claim's text, its subject's source, its scenario, and the plugin that answers
for them, so a scenario that cannot run is recorded against that claim and every other claim keeps its
verdict. A claim is still never called supported or contradicted by a run that did not happen, which is the
rule that made `RunFailure` abort the whole build. Aborting is what the incremental unit replaces, not a
protection being weakened: once the name tables are gone an unresolvable world is discovered at run time, and
an abort would make one typo in one claim cost every verdict in the project.

**A claim's name is readable and derived, and the digest is a value.** `hash::claim_id` (`hash.rs:19-25`)
returns `c` plus 16 hex characters, and that string is currently the name on every surface: `ClaimResult.id`
in `result.json` (`result.rs:63-64`), the leaf ids `{claim_id}#l{n}` the judge cites, the argument to
`veridoc witness`, and every report line. It becomes `<referent>#<predicate-slug>`, because a reader or an
agent grepping a report learns nothing from `c1a2b3c4d5e6f7a8` and something from
`parse_config#lists-modules-in-deps-order`. The left side is the subject the author named and the right side a
bounded slug of the predicate, because those are the identity's only inputs (`hash.rs:17-18`), and a name
built from a document anchor would move when the document moves and invalidate verdicts that did not change.
The author never writes an id: it is derived from the sentence, so it cannot drift from it. The digest stays
as a stored field and as the collision suffix, `parse_config#lists-modules~a1b2c3`, and `bind.rs:101-109`
already reports a duplicate by predicate and owner, so two claims that slug alike name each other instead of
one overwriting the other. Cache file names keep the digest, because nobody greps them.

**A new file kind, directory, or format rule requires a decision record approved before implementation.**
`spec.md`'s layout section is the single normative list of what an author writes, and an implementation that
differs from it is a bug rather than a variation.

## Alternatives considered

Keeping `features/` and adding run grouping, so that a shared scenario earns its file by being executed
once for many claims, is coherent and I considered it first. I rejected it because the reason authors value
a feature file is readable prose, and the step labels survive in the sidecar, while the reuse it would buy
is an optimisation the engine can add later without a format change.

Declaring vocabulary on a document branch and inheriting it down the tree was the earlier form of the sharing
rule. It is rejected rather than deferred: it makes what a claim asserts depend on where its document hangs,
so moving a document in the tree silently re-resolves the claims under it, and the reuse it enables is reuse
that a module reference already provides.

Leaving the run-failure rule alone and keeping the pre-run name checks as the protection against a typo was
the status quo, and its cost is that one misspelled word removes every verdict from a project's run.

Keeping a compiled language's claims in JSON, with the executable half of a scenario keyed by id inside the harness,
is what exists today, and the decision above closes it. I rejected it because it makes co-location impossible in one of
the two runtime models, and a rule that holds for a hosted language and not for a compiled one is a convention of that
plugin rather than a design decision. The price of the executable sidecar is that `collect` needs a successful build in
a compiled language: a compile error in a project's claim module stops collection with a build failure instead of
returning a claim set, and the first `collect` pays a compile that reading JSON does not.

Leaving the plugin as a standalone executable with the project hand-writing the harness protocol is what
runs today, and the cost is visible in the example: the harness reimplements the request and answer shapes,
and the action name is a string that appears in the claim file and again in a dispatch elsewhere, so renaming one
leaves a run failure rather than a compile error.

## Consequences

An author deletes `features/` and every `*.obs.<ext>`, and writes one file per module instead of two. A plugin for a
hosted language loses its fixture and action registries and the files its emitter writes; a plugin for a compiled
language gains a library the project adds as a dev-dependency, moves its claims from data sidecars into the claim
module beside the code, and shrinks the harness a project writes to one call into that library. Both plugin
executables keep the server, the source scan, and the build-and-spawn orchestration. The engine loses 229 lines of
Gherkin parsing and gains a `scenarios` field in `collect-result.json`.

Two things are genuinely lost. The engine no longer owns a scenario grammar, so two plugins can drift on
what a step means, which is the reason the grammar moved into the engine in v1. And a misspelled world name
inside a claim is discovered at run time rather than at bind time, which the scoped run failure keeps from
costing more than that one claim.

## Implementation notes

Nothing below is a format decision. Which build target a compiled language's harness uses, and where it sits in that
project's tree, is stated in that plugin's repository, because it is a property of the package manager; what the host
requires is that the target compiles with the project's test-only dependencies available. The plugin library exposes
the builder, the scenario registry, and the harness runtime, and
the project's harness names the claim modules it links, so the binary states which claims it will answer
for rather than discovering them by linkage.
The step labels are plain strings, with no grammar in any language.

The cache in the tree is not that unit yet. It stores judge verdicts at rule-by-section granularity
(`verdicts.rs:16-31`), and the two digests in its key, `claim_set_hash` (`hash.rs:36-40`) and
`referent_revision` (`hash.rs:53`), fold every claim and every referent of the project into one value, so one
edited claim invalidates every section. Execution is not cached at all: `pipeline.rs:190` runs every claim on
every check. An incremental run needs a per-claim key covering the claim's text, its subject's source, its
scenario's source, and the plugin identity, and it needs `pipeline.rs:135` to record a failure as a result
instead of returning early. A `build` failure must not be stored under such a key, because the key does not
cover the whole build and an unrelated fix would leave a stale failure; fixture, plugin, and deadline failures
are claim-local and may be.
