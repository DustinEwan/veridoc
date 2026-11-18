# 0011 - An authored placeholder pulls its content at assembly

Status: accepted, 2026-11-18. Implemented the same day: `spec_md::parse` records the opener and the block
it repeats, `bind` assembles the body from the bound tree (`crates/veridoc-engine/src/bind.rs`), and the
section hash and the brief read the assembled body. The `source` vocabulary settled at implementation is
`claims`, `concepts`, `documents`, `observables`, `actions`, and `fixtures`, spelled out with each
collection's fields in `docs/spec.md:299-323`.

## Context

The only content the engine injected before this record was the text of a named claim beside the section that names
it, and the author did not place it (`docs/spec.md:74-78`). A fact the code decides, an exit-code table or a set of
config keys, had no such route: the author copied it into prose, and nothing re-checked the copy.
`docs/spec.md:69-72` already names this as the failure to avoid, that a shape which makes a reader copy facts
elsewhere is a shape that will be copied wrong.

## Decision

**A `doc-each` element asks for content, and the engine resolves it at assembly.** The element names what to gather in
its `source`, and `doc-value` is where each value lands, in the same inline form `doc-claim` uses, so a placeholder can
sit inside a sentence or a code sample without breaking it. `doc-each` is an opener like `doc-section`, and the one
block that follows it is the block that repeats. The grammar of a `source` expression is settled at implementation:

```markdown
<doc-each source="...">
```text
<doc-value/>
```
```

**The values come from the tree the build already holds.** Compilation and the test run assemble the whole tree before
any document is written: every section, claim, referent, witness, and the answers the plugins returned. An element
resolves against that tree, so it asks nothing new of a plugin, needs no include mechanism and no generated fragment,
and has nothing to keep in step. The assembled document is a rendering of the tree, and this element is one more thing
the renderer resolves. What can be named is whatever the tree holds at the time someone writes the element.

**The authored file keeps the placeholder.** The engine never writes into a `.spec.md` file, which is the rule at
`docs/spec.md:73`, and the assembled brief and the report are where the values appear. A reader who opens the file in
a viewer reads the spec as written, placeholder included; a reader of the output reads the values.

Injected content is not a claim. It co-locates the fact with the prose that depends on it; the verification of that
fact stays a claim in the executable sidecar, referenced by a `doc-claim`.

## Consequences

The section hash covers the assembled body, not the authored body. The judge reads the values, so gathered content
that changes invalidates the verdicts of every section that pulled it in, which is the reason to co-locate the prose
with the code in the first place. A section whose injected content changed is re-run even when the author touched no
prose, and the report names the element that did it.

A source expression that matches nothing is an error, not an empty render, because an empty result in the middle of a
paragraph reads as a section that is finished.

## Alternatives considered

Filling the placeholder in the authored file was rejected because it makes the engine a writer of prose, which
`docs/spec.md:73` forbids, and a filled file committed by accident cannot be told apart from a stale one.
