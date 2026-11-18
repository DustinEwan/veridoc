# 0007 - A referent id carries the name it stands for

Status: proposed, 2026-11-18. Not implemented. Superseded on one point by
[0009](0009-the-subject-is-named-not-derived-from-a-path.md): the referent id is not the left half of a claim's name.

## Context

`docs/spec.md:152-158` now states that a claim's name is the readable pair `<referent>#<predicate>`, and the
engine implements it: `hash::claim_name` (`crates/veridoc-engine/src/hash.rs:27-32`) takes the last `:`-separated
segment of the referent id as the left half. That half is only as readable as the referent id's tail, and both
real plugins build a referent id that is nothing but a hash: the prefix plus twelve hex digits
(`../veridoc-plugin-ts/packages/plugin-ts/src/ids.ts:26-28`,
`../veridoc-plugin-rust/crates/plugin-rust/src/ids.rs:14-16`).
A claim about `parse_config` in a real project is therefore named `a1b2c3d4e5f6#reads-the-config-from-the-root`,
which is half an improvement. The fake plugin used by the engine's own tests scripts its referent ids by hand,
which is why the tests read well and the real plugins do not.

The path belongs in the id because the same exported name in two files has to be two referents, and the file is
what tells them apart. The digest over that path adds nothing to that property: renaming a file changes the path,
and changes the digest exactly as fast, so the digest contributes a fixed width and a form no reader can use, and
no stability the path does not already give.

What follows from keeping the path in the id is that the id itself is file-sensitive. Renaming a file rebuilds the
referent table and moves nothing else, because a claim's identity is the subject the author named joined with its
predicate. `docs/spec.md:50` says a referent id is derived from the declaration and not from a file position, and a
path is a file position; that is why the id is kept for reporting and not used as identity.

## Decision

**A referent id is the declaration it stands for, written out.** The form is
`veridoc:referent:<path>:<name>` for a symbol, where `<path>` is the root-relative POSIX path the plugin hashes
today, and `veridoc:concept:<name>` for a declared concept. The path is what distinguishes two `parse_config`
functions in two files, so it carries the uniqueness the digest was keeping; the digest added a fixed width and
nothing else. **A name or path containing the separator is encoded by the plugin.** Any `:` inside a path segment
or inside a subject name becomes `%3A`, so splitting the id at its last `:` always yields the whole name, whatever
the language's own qualifier syntax is; the plugin's repository states which of its declarations need it.
The claim's name takes its left half from the subject the author named, and the surfaces that print a
whole referent id,
`referentId` in `result.json` (`crates/veridoc-engine/src/result.rs:69`) and the referent table of
`veridoc probe-plugin` (`crates/veridoc-cli/src/probe_plugin.rs:93`), now name the file as well as the symbol.

The engine reads only the last `:`-separated segment of a referent id, so nothing in it depends on the segments before
that, and the change lands in the two plugins, in the plugin test that
asserts twelve hex digits (`../veridoc-plugin-rust/crates/plugin-rust/src/ids.rs:54-60`), and in the sentence
about the form at `docs/spec.md:50`.

The cost is a recomputation, not a failure. Every cached judgement is keyed on `claim_set_hash` and
`referent_revision` (`crates/veridoc-engine/src/hash.rs:36-40,53`), and both cover the ids, so an id change makes
every existing entry miss once and the next run refills the cache. No stale entry can be replayed and then refused,
because a key built from the old ids is never looked up again.

One risk is new rather than inherited. A concept's id becomes the name the author wrote in the index, so two
concepts declared with the same name collapse into one referent, and the second claim is reported as a
duplicate of the first instead of the index being reported as wrong. Bind rejects a duplicate claim today and
says nothing about a duplicate concept name in the index, so that check ships with this change.

Host and plugin cannot disagree about the collision suffix. The engine adds it after dropping claims whose
subject did not resolve, and a plugin computes names over everything it collected, so the two sets could in
principle differ. They do not: an unresolved subject is a bind error (`crates/veridoc-engine/src/bind.rs:449-456`),
and bind returns before any plugin call when any error is present (`bind.rs:850-852`). A run that reaches
`bivalence` or `run` therefore named every claim the plugin collected.

## Alternatives considered

Carrying a separate display label from the plugin to the engine keeps the referent id untouched and adds a wire
field to `resolve`, which the engine then has to thread through the claim, the result document, and the cache key.
It answers the same problem with more moving parts and leaves the id itself unreadable in the places where an agent
cites it.

Keeping the digest and appending a readable tail, `veridoc:referent:a1b2c3d4e5f6:parse_config`, was the first
draft of this record. It is rejected: the digest discriminates nothing that the path does not already
discriminate, so the id keeps a half that no reader can use and that no rule depends on.

Leaving the left half hashed is available and costs nothing today, but it leaves `referentId` in `result.json` and the
referent table of `veridoc probe-plugin` half a name, which is the thing the readable name was chosen to remove.
