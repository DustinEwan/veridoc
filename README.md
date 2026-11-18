# veridoc

veridoc is a documentation checker for projects where an agent writes the documentation. You declare the
documents your project owes, you write one claim per assertion in a sidecar beside your code, and you write
the prose yourself in sections that name those claims. veridoc then evaluates every claim through the one
channel it declares: a scenario whose assertion can fail, a recorded artefact, or a judge that compares your
prose with claims that already passed.

The unit is the claim, not the document. A claim is one assertion bound to a symbol that the plugin resolves,
so renaming a function breaks the claim that describes it, and a claim that restates something the type
system already says is rejected as redundant. A claim ends supported, contradicted, or ambiguous, and any
other ending fails the build, so a claim can never sit unevaluated and pass.

**This repository holds a specification. Nothing here is implemented.** Start with
[`docs/spec.md`](docs/spec.md), and read the five records in `docs/decisions/` for the choices that
a reader is most likely to want to argue with.

## Why it exists

An agent writes documentation cheaply, which removes the cost that used to keep documentation short
and therefore roughly honest. What replaces that cost is volume: the same fact asserted in five places,
each copy drifting on its own schedule, and justification prose that the next agent reads as
permission to take the same shortcut. Every failure that veridoc targets comes from prose having no
address, no owner, and no condition under which it would be shown to be false.

The design follows one rule from that diagnosis: prose is only allowed in a slot with a declared kind,
and every kind has a checker. There is no top-level prose slot, because a format that needs judgement
about what may go into it produces the same slop in a new file extension.

## Contents

| Path | What it holds |
| --- | --- |
| [`docs/spec.md`](docs/spec.md) | The v1 specification: objects, rules, the two artifact stages, the plugin boundary, the report. |
| [`veridoc-1.profile.json`](veridoc-1.profile.json) | The plugin contract in machine-readable form: every method with the schema of its request and result, what each absent capability costs, what every loss reason becomes, and each conformance rule with the scripted defect that shows it can fail. `prpc-core` from the pluginrpc mechanism reads it and runs the defects; nothing of this repository sits between the plugin and the wire report. |
| [`docs/decisions/`](docs/decisions/) | Five dated records for the load-bearing choices. |

## Status

| Area | State |
| --- | --- |
| Specification | Draft v1, open questions listed at the end of `docs/spec.md`. |
| Implementation | Not started. |
| Licence | Not chosen. |
