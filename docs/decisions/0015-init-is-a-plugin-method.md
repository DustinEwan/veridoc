# 0015 - `init` is a plugin method

Status: accepted, 2026-11-18. Implemented the same day: the capability is `initialization`
(`crates/veridoc-protocol/src/contract.rs`), the method is `Method::Init`
(`crates/veridoc-protocol/src/methods.rs`), the host side is `crates/veridoc-cli/src/init.rs`, the
rule is `init-creates-the-project` (`crates/veridoc-conformance/src/lib.rs`), and the profile names
both (`veridoc-1.profile.json`). This record supersedes the last paragraph of the Alternatives
section of `docs/decisions/0013`; every other rule of 0013 stands.

## Context

`docs/decisions/0013` removed the host-side `index.doc.ts` template and left the root document to
"the plugin's own tooling — a subcommand, a template in its own repository, or a documented shape".
The installed plugin has none of those. `veridoc-plugin-ts` ships three packages
(`packages/claims`, `packages/plugin-ts`, `packages/tree`), no `bin` entry, and no command-line
front end, so there is no subcommand to run and no template to copy. The only shape available to an
author starting a project is a file the plugin parses, whose shape is readable only in
`packages/plugin-ts/src/collect.ts`.

So the first command of a new project produced a project that could not be built. `veridoc init`
wrote `.veridoc/config.json`, printed the next step, and the next step named tooling that does not
exist; the first `veridoc check` was a collect error before the author had typed anything.

The host also already held the answers the plugin needs. `init` asks the author for the document
path, its audience, and its one question, because those are the questions a host can ask. Refusing
to pass them to the one component that writes the file routed the same three answers through the
author's editor instead of the pipe, and left the author to guess the file shape around them.

## Decision

**`init` is a method on the profile, gated on a new capability.** The request carries the project
`root`, the first `document` (path, audience, question, optional `notApplicable`), and `force`; the
result reports `written`, every file the plugin created relative to `root`, and `next`, the work only
the author can do (`docs/schema/veridoc-1/init-req.json`, `docs/schema/veridoc-1/init-result.json`).
The host still writes no project file the plugin must read: the rule 0013 exists to enforce is
unchanged, and the host's own output remains `.veridoc/config.json` and the ignore entry.

**The plugin answers before the host writes anything.** `crates/veridoc-cli/src/init.rs` builds the
configuration in memory, starts the plugin, and writes the configuration only after the answer
arrives, so a refusal leaves the project exactly as it was found. A configuration with no root
document is the half-written scaffold `docs/decisions/0006` forbids.

**A plugin without the capability is reported, not guessed at.** `veridoc init` writes the
configuration, prints that the plugin declares no `initialization`, and stops. The profile states
that behaviour in `capabilities.initialization.ifAbsent`, and the capability table in `docs/spec.md`
states it to a reader.

**The rule is measured, not asserted.** `init-creates-the-project` asks the plugin for a scratch
project outside the fixture, requires `written` to be non-empty and each named file to exist, and
requires a second request over the same project to be refused by naming one of those files rather
than replacing it. The scripted defect `{"init":{"written":[],"next":[]}}` in
`veridoc-1.profile.json` is run by `crates/xtask/tests/conformance.rs`, so the rule is shown able to
fail.

## Consequences

A new project runs one command and gets a tree its plugin parses. The TypeScript plugin must now
implement `init` or drop the capability from its handshake; either way the host's output is the same,
and the difference is the printed line the author reads.

`init` is the first profile method whose answer is a side effect on disk, so the conformance suite
now writes to the filesystem. Its scratch directory is per probe
(`veridoc-conformance-init-<pid>-<n>` under the temporary directory), because the harness runs
several plugins in one process and a probe that deleted another's directory would report its own
files as missing.

The host keeps no template. An author who wants a different document layout answers the flags
differently; the plugin still writes the file.

## Alternatives considered

Telling the author to run the plugin's own scaffold was rejected. The command differs per plugin, the
host cannot name it, and the suite cannot check that it exists, so the instruction is unverifiable
advice — which is what 0013 shipped.

Adding the method with no capability was rejected. A plugin that does not answer would turn the first
command of every project into a `-32601` failure, where the capability turns it into a printed
sentence and a working configuration.

Having the host write a language-neutral root document (`index.doc.json`, or a Markdown file the
plugin tolerates) was rejected for 0013's reason, which still holds: the host would be authoring a
file it cannot validate, and a second shape would compete with the one the plugin parses.
