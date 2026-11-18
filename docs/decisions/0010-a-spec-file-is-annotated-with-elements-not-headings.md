# 0010 - A spec file is annotated with elements, not headings

Status: proposed, 2026-11-18. Not implemented. Supersedes the `#/` directive grammar drafted earlier the same day.

## Context

A `.spec.md` section is delimited by a markdown heading, and its identifier rides on the heading text in braces:
`## What the scanner refuses {#refuses selects="concept:project-scan kind:ensures"}`
(`crates/veridoc-engine/src/spec_md.rs:4-7`). Four things follow from that, and each costs something. A heading with
no `{#id}` is a parse error except the first `#` title (`spec_md.rs:89-96`), so an author cannot use a heading inside
a block of prose without creating a block. The heading level carries no meaning, because `Section.level` has no
consumer outside the parser. The `selects` value is a mini-language that exists to be split apart again, on spaces,
on colons, and on commas (`spec_md.rs:310-314`), and it reaches into three things a document should not name: a
claim's concept, its kind, and its referent. And a claim's identity is already the pair of its subject and its
predicate (`crates/veridoc-engine/src/hash.rs:30-31`, `crates/veridoc-protocol/src/contract.rs:119-129`), which the
heading syntax has no way to express, so the document names claims by filtering instead of by naming them.

A line-oriented annotation was drafted first for this file and rejected on the floor: a directive is a line, a line
break inside prose is a paragraph break, and an author who wanted to attach a claim to one sentence had to split the
sentence to do it.

## Decision

**A spec file is markdown annotated with custom elements, prefixed `doc-`.** The prefix reuses the word the project
already uses for the files an author writes, `src/<module>.doc.<ext>` and `index.doc.<ext>`. A section opens with an
element on its own line; a claim reference is an empty element that sits inside a sentence:

```markdown
<doc-section name="scanner">

## What the scanner refuses

The scanner reads its configuration from one directory <doc-claim name="rejects-missing-config"/> and never
searches for one. A directory with no config.json is refused with exit code 2 rather than guessed at.

<doc-claim name="rejects-a-config-of-the-wrong-kind"/>

A config.json whose top level is not an object is refused the same way.
```

**A section names the claim's subject, and a `doc-claim` names its predicate.** The two halves compose the claim's
existing readable name, `scanner#rejects-missing-config`, which is how `hash::claim_name` already builds it. Nothing
new is being named, and the document stops filtering: `selects`, and with it `Selects` and its `concept`, `kind`,
`referent`, and `claims` fields (`contract.rs:222-231`), is gone. A section covers the claims it names and nothing
else.

**A reference must resolve, and an unresolved one stops the build.** The claim is defined in the executable sidecar,
because it carries its scenario as code (`0006`), so a `doc-claim` in prose is a reference to a claim the plugin
registered under the section's subject. Today a name that matches nothing is never consulted at all
(`crates/veridoc-engine/src/bind.rs:834-840`), and the only signals are two warnings that do not fail a run
(`bind.rs:841-847`, `bind.rs:863-870`, `crates/veridoc-engine/src/pipeline.rs:4`). The reference grammar requires the
missing diagnostic: a name that matches no claim, or a claim no section names, is a blocking error naming the file,
the line, and the composed claim name.

**A section name may repeat, because coverage is the rule.** `name` is the claim's subject, and one subject belongs to
more than one section as a matter of course: the claims about `scanner` are written down in the architecture document
and in the tutorial, and the claims about `architecture` itself are spread across every document that reasons about the
system. What the build enforces is coverage in both directions, every reference resolving to a claim and every claim
named by at least one section, not one section per subject. Nothing in the format identifies a section, or a claim, by
the file it sits in, so a file rename moves no claim. The composed `subject#predicate` is unique project-wide, and two
claims that compose the same one are a duplicate error naming both owners (`bind.rs:475-482`). The duplicate-anchor
error at `bind.rs:1164` goes away with the uniqueness it enforced.

**The elements are valid markdown, and they are invisible where markdown is rendered.** CommonMark permits raw inline
HTML anywhere in a line, which is what lets a reference sit inside a sentence, and an unknown element renders as
nothing, so a browser shows clean prose for an inline element or for an opener on its own line. A closing tag that
follows a blank line is the one exception, because CommonMark ends a block of raw HTML at the blank line and then
prints the tag as text, so every element in this format is either inline or an opener. The annotation stays in the raw
text for the author who reads the file.

## Consequences

`doc-section` is an opener, not a wrapper, and a section ends at the next opener or the end of the file, which is why
no element in the format carries a closing tag.

The section hash covers the body, and a reference element inside a sentence changes every hash once, refilling the
judgement cache on the first run after the switch.

A reference placed beside a sentence lets the engine check that sentence itself, which is cheaper and surer than
asking a model whether the section mentions the claim, and `S1Verdict` already reports the other direction at span
granularity (`contract.rs:493-503`). Tightening the mention check to the paragraph is a separate decision from this
one; the placement makes it possible.

`supports` on a claim (`crates/veridoc-protocol/src/contract.rs:126`) exists to cite a document section, written as
`file#anchor`, and `docs/spec.md:289` checks that link in both directions. Under a one-way reference the field has
nothing left to say: the document names the claim, and coverage is computed from that name. The field goes, with the
second check, and `docs/spec.md:107` and `docs/spec.md:289` change with the implementation.

The syntax lives in five places and moves together: `spec_md.rs`, its tests, the error text in `bind.rs`, the
fixtures in `crates/veridoc-cli/tests/check.rs`, and the two example `architecture.spec.md` files.

## Alternatives considered

A `#/` line namespace was drafted first. It is rejected because a directive is a line and a line break is a paragraph
break, so annotating one sentence forced the author to split it.

Keeping the heading as the delimiter and making `{#id}` optional was smaller and allows a heading inside a section. It
is rejected because the document would still name claims by filtering, and the filter vocabulary, `concept`, `kind`,
and `referent`, selects on fields no build rule depends on.
