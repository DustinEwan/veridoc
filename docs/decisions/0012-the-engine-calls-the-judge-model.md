# 0012 - The engine calls the judge model

Status: accepted, 2026-11-18. Implemented the same day: `ModelJudge` over the `rig-core` provider
client (`crates/veridoc-engine/src/judge.rs`), the `judge` block of `.veridoc/config.json`
(`crates/veridoc-engine/src/config.rs`), and the deletion of the two transports that came before it.

## Context

Two things made this record necessary, and the second was measured rather than predicted.

[0002](0002-judge-verdicts-are-pinned-and-cited.md) settled that the reviewer of a claim no
deterministic check can settle is a model, and that every span a verdict cites is re-verified before
the verdict is kept. The engine shipped two ways to reach that model: a call to one vendor's Messages
API with the host hardcoded, and `judge.transport: "command"`, which wrote the brief to the stdin of a
program the project names and read a verdict from its stdout. The second existed because the engine
could reach one endpoint, and a project that wanted to run `check` where no model is reachable had no
other way to get an answer.

Making that a configuration default produced the failure this repository exists to prevent. A project
that accepted the default shipped a judge program that answered every task with an empty verdict. The
run exited 0, reported four claims supported and a residue of zero, and the prose had never been read
by anything. A check that did not run was indistinguishable from a check that passed.

The endpoint the projects actually use was neither of the two the engine spoke. A local or self-hosted
inference server speaks the OpenAI chat wire on a host the project chooses, so every project that
wanted a real judge wrote a program in front of it, in whatever language came to hand.

## Decision

**The engine calls the model.** `judge.provider` selects the wire, `judge.model` names the model on
it, `judge.baseUrl` redirects an OpenAI-compatible server, `judge.apiKeyEnv` names the environment
variable holding the credential, and `judge.additionalParams` merges provider-specific fields into the
request body. The providers are `openai`, `anthropic`, `gemini`, `ollama`, `groq`, `xai`, `deepseek`,
`openrouter`, and `mistral`; any other server that speaks the OpenAI chat wire is reachable by naming
its `baseUrl`. `transport` and `command` are gone, and because `judge` rejects unknown fields, a config
carrying either fails at load instead of ignoring a key.

**There is no judge program.** A process between the engine and the model decides what the model said,
and nothing in the run can tell a model's answer from a canned one. The earlier rule that a judge
transport error is a run failure stands; this removes the case where the transport is a script the
repository controls, which is the case where a green run could not be trusted.

**A project may opt out, and the opt-out is visible.** `judge` may be absent. S1, S2, S3, and the
support half of a citation then do not run. The run prints that the semantic stage did not run, the
result document carries the same statement as a warning, and a claim whose only channel is the judge,
or whose citation support was never judged, is an error. 0005's rule is intact: every claim is
evaluated, or the build is red.

## Consequences

A reasoning model spends most of its window thinking, so `timeoutMs` defaults to 900000 and
`maxTokens` to 32000. A call that exceeds the window is a run failure, never a skipped check.

The verdict cache key already carries the model id, so changing provider or model misses every cached
verdict and re-judges, rather than replaying one model's answer under another's name.

The semantic stage is a local check, not a continuous-integration check, and the reason is the
verdict cache. Locally the cache makes the stage incremental: a run whose sections and claims are
unchanged replays every verdict and calls no model, which on this repository's own project is the
difference between a 43-second judged run and a 0.1-second replay. A cold checkout in CI has neither
a warm verdict cache nor a warm scenario-harness build, so a run there re-judges every section before
it can report anything, and it records whatever the model said on that run, which makes the gate
non-reproducible. So the configuration a project's CI reads leaves `judge` out, CI reports the
semantic stage as not run, and the deterministic stage still gates the build. A project whose runners
reach a model may point `judge.baseUrl` at it and accept both costs; the engine does not prevent it,
and none of these repositories do it. There is no third choice, because the option that let a run look
as though it had judged — a program in the repository answering for the model — is gone.

The engine gained `rig-core` and a current-thread tokio runtime to drive it. The pipeline stays
synchronous; the runtime exists inside one model call and nothing else in the engine sees async.

## Alternatives considered

Keeping `command` as an opt-in, labelled escape hatch was rejected. The label would name the program
that answered, but the program is what decides the verdict, which is the thing under removal, and a
report whose verdicts came from a script is one a reader has to disbelieve anyway.

A `--no-semantic` flag was rejected because a flag is invisible in the committed configuration. The
absence of `judge` is in the file a reviewer reads.
