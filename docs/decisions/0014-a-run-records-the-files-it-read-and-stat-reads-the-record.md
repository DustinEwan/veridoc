# 0014 - A run records the files it read, and `stat` reads the record

Status: accepted, 2026-11-18. Implemented the same day: `check` writes the record at
`crates/veridoc-engine/src/pipeline.rs` step 16, the record itself is `crates/veridoc-engine/src/inputs.rs`,
and the gate is `crates/veridoc-cli/src/stat.rs`.

## Context

A continuous-integration job cannot run the semantic stage. `docs/decisions/0012` made the engine call the
judge model, so a job with no model credential gets a run failure, and a configuration that leaves the judge
out reports the semantic stage as not run. Either way the job learns nothing about the prose. What the job can
do is answer a narrower question, and the question authors actually ask is this one: were the files in this
commit the files the last judged run read? If they were not, a green record proves nothing about this commit.

Re-running `check` in the job is not the answer, for two reasons. The verdict cache is a run output that
`veridoc init` adds to the ignore list, so a fresh checkout has nothing to replay from and every task is a
miss that needs a model. And the cache is keyed on the section body, its claims, and the referent signatures,
so a source edit that changes no signature would replay a verdict earned against different code. A key built
to skip work that cannot change the answer is not a gate that detects a changed checkout.

## Decision

**A completed run records what it read.** `check` writes `.veridoc/inputs.json` beside the result document:
every file the project's commit would carry, each with the sha256 of its content. Failing to write it is a run
failure, because a run that recorded nothing is a run no later job can check against.

**The record covers the commit, not the slice a verdict covers.** In a git checkout the file set is
`git ls-files --cached --others --exclude-standard`: tracked plus added-and-unstaged, minus whatever the ignore
rules hide. Outside a repository the directory is walked with an exclusion list. The engine's own outputs
(`result.json`, `last-result-hash`, `inputs.json`, `out/`, `engine.log`, `verdicts/`) are never in the record,
so the record cannot contain itself or change because a formatter ran.

**`veridoc stat` reads the record and starts nothing.** It hashes the checkout, compares, and reports through
the exit code, because the reader is a job that looks at nothing else: 0 the record covers this checkout, 1 it
does not, 3 there is no record to compare against. No plugin is started, no scenario harness is built, and no
model is asked anything, so the gate costs one pass over the tree.

**The gate is not a check.** It does not re-derive an outcome, because that means running the scenario, and it
does not prove the deterministic stage passed at the recorded commit. Those are the jobs of `check`, and `check`
needs the model and the harness.

## Consequences

An author commits a generated file. Every source edit changes `.veridoc/inputs.json`, so it churns the way a
lockfile does. That is accepted because both sides are cheap: the gate is one comparison per file with no model
and no plugin, and the repair is a local `veridoc check`, where the verdict cache replays every section that did
not change. The alternative, fingerprinting only the files one verdict key covers, would let a change to a file
no key names pass unrecorded, which is the failure the gate exists to catch.

`inputs` is the one place the engine touches a project's source files, and it only hashes them (`docs/spec.md`,
"The record and the gate"). The language-agnosticism test still passes because the engine parses nothing: the
file names and the byte counts are all it reads. The git call adds no new capability, because
`crates/veridoc-engine/src/git.rs` already shells out to git for citation stores.

Adoption is two commits deep. A project runs `veridoc check`, commits `.veridoc/inputs.json`, and only then adds
`veridoc stat` to its job: a job that runs `stat` before any record is committed exits 3, which is the honest
answer, since there is nothing to check the checkout against.

The command is named `stat`, not `verify` or `guard`. `verify` overclaims in a tool whose name is a verb about
truth, and the engine already has `artefact::verify` and `veridoc_engine::verdicts::verify_ids` meaning something
else. `guard` names the role rather than the check, so a workflow line reading `veridoc guard` invites the belief
that the documentation was checked at this commit. `stat` understates, and the direction it errs in is the safe
one: a reader who assumes it merely printed something still gets the exit code that blocks the job.
