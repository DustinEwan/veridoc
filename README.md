# veridoc

veridoc is a documentation checker for projects where an agent writes the documentation. You declare
the documents that your project owes, you write one assertion per claim in a file beside your code,
and veridoc checks every claim against the code that it describes and against a scenario that can
fail.
It prints the claims that no evidence supports, so the reader reviews the unsupported set instead of
reading all the prose.

The unit is the claim, not the document. A claim is one assertion bound to a symbol that the compiler
resolves, so renaming a function breaks the claim that describes it, and a claim that restates
something that the type system already says is rejected as redundant. Prose survives only where
nothing in the code can contradict it.

**This repository holds a specification. Nothing here is implemented.** Start with
[`docs/spec.md`](docs/spec.md), and read the four records in `docs/decisions/` for the choices that
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
| [`docs/decisions/`](docs/decisions/) | Four dated records for the load-bearing choices. |

## Status

| Area | State |
| --- | --- |
| Specification | Draft v1, open questions listed at the end of `docs/spec.md`. |
| Implementation | Not started. |
| Licence | Not chosen. |
