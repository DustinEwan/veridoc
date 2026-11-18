# 0009 - The subject is named, not derived from a path

Status: accepted, 2026-11-18. Implemented the same day: `hash::claim_identity` and `hash::claim_name`
(`crates/veridoc-engine/src/hash.rs:21-34`) take the author's subject name, `bind` passes it
(`crates/veridoc-engine/src/bind.rs:540-541`), and both real plugins compute the same name from the same
input.

## Context

A claim already carries everything this needs in `crates/veridoc-protocol/src/contract.rs:128-137`: `subject`, the
thing it is about, resolved by a plugin; `concept`, the author's grouping label; `predicate`, the author's short
phrase for the one thing asserted; and `text`, the prose sentence, which is in no key. Identity is the subject joined
with the predicate, and bind reports a second identical pair as a duplicate (`crates/veridoc-engine/src/bind.rs:555`).

The one problem is what the subject resolves to. A plugin derives a referent id from the file path and the symbol name
(`../veridoc-plugin-ts/packages/plugin-ts/src/ids.ts`, `../veridoc-plugin-rust/crates/plugin-rust/src/ids.rs`), so
renaming a file re-derives the id of every claim about that module: new claim names, every cached verdict a miss, every
pinned witness orphaned. Only the author knows that the module in `scanner_v2.<ext>` is the one that used to be
`scanner.<ext>`.

## Decision

**The subject's identity comes from the author, not from the path.** The claim keeps naming what it is about, and that
thing is named by the author rather than derived from where it is written. Identity stays the pair of subject and
predicate, `concept` stays the grouping label it is today, and the duplicate rule at `bind.rs:476` keeps working
unchanged.

Coverage and the referent table keep coming from the plugin's resolution of the subject, so a claim still has to point
at real code for its declaration to count as covered.

## Consequences

Renaming a file costs a rebuild and nothing else. Rewording a predicate changes that claim's id, invalidates its
verdicts, and orphans its witness, which is the author editing the claim rather than a side effect of moving a file.

The referent id is a reporting value and not an input to a claim's identity. `resolve` and `veridoc probe-plugin` print
it spelled out as `0007` fixes, because the file and the symbol are what a reader of those tables wants, and renaming
a file changes that string without touching a claim's name, verdict, or witness.

## Alternatives considered

Keeping the path-derived subject costs a re-run on every file rename, which is the whole problem here, and buys nothing
else: the duplicate rule it supports compares the predicate as written, so two authors phrasing one behaviour
differently already produce two claims today, and the semantic stage remains the only thing that can notice.

`docs/spec.md:55-57` claims the checker rejects "a predicate that matches more than one assertion slug". Nothing in
`bind.rs` implements it. That is a separate defect from this record and is not solved by either option here.
