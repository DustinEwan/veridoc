# 0013 - The plugin owns the root document

Status: accepted, 2026-11-18; superseded in part by `docs/decisions/0015`, which adds the plugin
`init` method this record rejected. Implemented the same day: `veridoc init` writes
`.veridoc/config.json` and the ignore entry and nothing else (`crates/veridoc-cli/src/init.rs`), no
engine source names an authored file extension (`crates/veridoc-engine/src/bind.rs`,
`crates/veridoc-protocol/src/contract.rs`), and
`crates/veridoc-engine/tests/no_language_knowledge.rs` enforces it with no allowlist.

## Context

`veridoc init` created `index.doc.ts` from a template that declared one document node. The host cannot
read that file. It is TypeScript, and the rule the host runs on is that the plugin parses the project's
language: the engine learns the document tree from the `documents` field of the `collect` answer, and it
parses nothing of the project itself. So `init` handed the author a file whose shape only the plugin
knows, in a language the host cannot check, for a host that might not read TypeScript at all.

The violation was already visible inside the engine. The diagnostic for an undeclared observable read
`observable … is not declared in any .obs.ts`, and `crates/veridoc-engine/tests/no_language_knowledge.rs`,
the test that keeps the engine language-agnostic, carried an allowlist naming `index.doc.ts` and `.obs.ts`
so that the message could stay. A test that exists to forbid a boundary was listing the places the
boundary was crossed.

## Decision

**The plugin owns the file that holds the document tree.** `veridoc init` writes the configuration, adds
the ignore entry, and prints what to do next. It writes no document file, no claim sidecar, and no
vocabulary file. The plugin's own tooling creates and maintains the root document, because the plugin is
the component that reads it.

**The host asks and the plugin answers.** The engine's knowledge of the root document is the `documents`
field of the `collect` answer, whose shape is in the published contract. A host that cannot read the file
cannot be wrong about it: a stale or hand-edited root document surfaces as a collect error from the
plugin, which is the component that can explain it.

**The engine names no authored file.** The observable diagnostic names the plugin, not an extension, and
the contract comment names the document tree, not `index.doc.ts`. The allowlist is gone, so an engine
source that mentions `.ts` fails the build.

## Consequences

A new project runs `veridoc init` and then asks its plugin how to declare a root document. The first
`veridoc check` on a project with no document is a collect error naming the plugin, not a missing-file
error naming a file the host invented.

A plugin that wants to hand authors a starting root document provides that itself — a subcommand, a
template in its own repository, or a documented shape. The host offers none, and a host-side template
would be a second, competing shape for a file the host cannot validate.

## Alternatives considered

Writing the template behind a flag was rejected. The file it writes is still wrong for any plugin that
does not read TypeScript, and an author who passes the flag has no way to check that the file matches
what their plugin expects.

Having `init` ask the plugin to scaffold the file, as a method on the profile, was rejected for now: it
adds a method to the mechanism's vocabulary for one command's convenience, and the plugin can create the
file with its own tooling without the host in the path. That reasoning failed on its last clause: the
installed plugin has no tooling of its own, and `docs/decisions/0015` adds the method.
