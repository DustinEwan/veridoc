# 0002 — Judge verdicts are pinned and cited

Status: accepted, 2026-11-16. Amended the same day by
[0003](0003-the-semantic-stage-reads-a-brief-not-code.md) and
[0004](0004-the-judge-compares-prose-with-claims-that-already-passed.md), which changed the task set and
what a verdict is recorded against.

## Context

Some claims cannot be lowered to a check, and for those the only available reviewer is a model. A model
answer is probabilistic, so it cannot be enforceable in the way that a compiler error is enforceable. Two
specific failures make a naive judge worse than no judge.

The first is an invented answer. Asked whether a passage of prose is supported by a set of claims, a model
tends to say that it is, because the question asks it to justify a yes.

The second is agreement for the wrong reason. The author of the prose and the judge are often the same
model family, so both can be wrong in the same way, and a fluent sentence reads as a correct one.

A verdict that blocks a merge becomes proof once a reader relies on it, and readers stop checking what
sits downstream of that block.

## Decision

Judge tasks are bounded and none of them asks whether documentation is good. S1 reports the claims that
prose does not mention and the assertions in prose that no claim states. S2 reports the pair of spans that
disagree. The judge proposes no change to code and no change to prose.

Every citation that a judge returns is re-verified deterministically before the verdict is kept, and a
verdict whose citation fails verification is discarded.

A verdict carries the hash of one section, the hash of the claims that section selects, the revision of the
referents behind those claims, the model id, and the prompt version, and those values are its identity. A
verdict is stale when any of them moves, and staleness is checked before a verdict is read.

Each task runs `k` samples, a value set per rule and defaulted to one, because a reasoning model converges.
Disagreement is reported as `ambiguous` rather than resolved by majority, because disagreement is the available
signal that a span means two things, and that signal only exists where `k` is greater than one.

Judge verdicts report and do not block. A semantic rule blocks only where the project configuration lists
it as blocking, and that is the only route by which a model answer stops a merge.

## Consequences

An optimistic verdict cannot hide an unevaluated claim, because a claim with no outcome is a build error rather
than a finding, and a cache key per section means a re-run covers the rest of the file without re-judging it.

An earlier draft let a verdict block when the author of the prose and the author of the claims were known
to be different identities. That clause is dropped. Nothing in v1 records authorship, and the propagation
rule in `docs/spec.md` covers the case the clause was reaching for without needing to know who wrote
what: evidence has to move with the prose and with the code, in both directions.

Sampling `k` times multiplies judge cost by `k`. The selection in the index already matches each section with
the claims that it selects, so the judge reads one section at a time rather than a cross product of the claim
graph.
