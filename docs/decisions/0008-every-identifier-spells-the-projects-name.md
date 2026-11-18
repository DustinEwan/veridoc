# 0008 - Every identifier spells the project's name

Status: accepted, 2026-11-18, by the project's owner. Implementation is in progress in the three
repositories.

## Context

The name is shortened to `vd` in places a reader meets, not only in prose. The wire protocol version is `vd/1`
(`crates/veridoc-protocol/src/lib.rs:12`). The committed schema directory is derived from that string
(`crates/veridoc-protocol/src/lib.rs:17`), which gives `docs/schema/vd-1/` in the host and the same directory name
in the TypeScript plugin. The machine-readable profile is `vd-1.profile.json`. The two plugins build subject ids
as `vd:r:` and `vd:c:` (`../veridoc-plugin-ts/packages/plugin-ts/src/ids.ts`,
`../veridoc-plugin-rust/crates/plugin-rust/src/ids.rs`), and the host's example configurations carry `"protocol":
"vd/1"`. Roughly two hundred lines across the three repositories carry the shortening.

The same conversation produced a second problem, visible in `0007`: a record in the host's directory reasoned about
the identifier syntax of one language, because that language's method paths contain the character the host's split
rule uses. The host delegates a subject's resolution to a plugin precisely so it does not know what a symbol looks
like in any given language, and a host document that names one contradicts that.

## Decision

**Nothing shortens the name.** Any identifier the project introduces spells `veridoc` in full, and an identifier that
names a thing rather than the project takes the word the project already uses for that thing. The protocol version is
`veridoc/1`, the schema directory it derives is `veridoc-1`, the profile is `veridoc-1.profile.json`, the subject
prefixes are `veridoc:referent:` and `veridoc:concept:`, and prose in both READMEs and every record spells `veridoc`.

**A host document names no implementation language.** The host's specification and decision records state the
requirement a plugin must meet and leave the language-specific consequence to that plugin's repository. Where a
requirement genuinely differs between languages, it differs by runtime model, a language whose programs run on a host
interpreter and a language that compiles to a standalone binary, and the host names the model rather than the
language. Where the host needs a rule about a character that some language's identifiers contain, the host's rule is
the encoding, and the plugin's document explains why that language needs it.

## Consequences

The cost is a recomputation and a refused handshake, both one-time. Cached judgements are keyed on values that
include the subject ids, so every entry misses once and the next run refills the cache. A plugin that still reports
`vd/1` is refused at the handshake rather than silently accepted, which is the behaviour the version field exists
for, and the two plugins and the two example configurations move to `veridoc/1` with this change.

Because the schema directory name is derived from the version string, renaming the version renames the directory,
and the committed-schema test compares the generated directory against the checked-in one, so a half-done rename
fails a test rather than passing quietly.

## Alternatives considered

Changing prose and leaving `vd/1` on the wire keeps every plugin working and was the smaller edit. It is rejected
because the wire string is the one a plugin author copies into a handshake and a reader meets in `result.json`, so
the shortening would survive in exactly the places it is read.
