# 0002 — Judge verdicts are pinned, cited, and never block a same-author pair

Status: accepted, 2026-11-16. Amended the same day by
[0003](0003-the-semantic-stage-reads-a-document-not-code.md) and
[0004](0004-the-judge-compares-prose-with-claims-that-already-passed.md), which changed the task set and
what a verdict is recorded against. The rule about a same-author pair is unchanged.

## Context

Some claims cannot be lowered to a check, and for those the only available reviewer is a model. A model
answer is probabilistic, so it cannot be enforceable in the way that a compiler error is enforceable.
Two specific failures make a naive judge worse than no judge.

The first is an invented answer. Asked whether a passage of prose is supported by a set of claims, a
model tends to say that it is, because the question asks it to justify a yes.

The second is agreement for the wrong reason. The author of the prose and the judge are often the same
model family, so both can be wrong in the same way, and a fluent sentence reads as a correct one.

A verdict that blocks a merge becomes proof once a reader relies on it, and readers stop checking what
sits downstream of that block.

## Decision

Judge tasks are bounded and none of them asks whether documentation is good. S1 reports the claims that
prose does not mention and the assertions in prose that no claim states. S2 reports the pair of spans
that disagree. The judge proposes no change to code and no change to prose.

Every citation that a judge returns is re-verified deterministically before the verdict is kept, and a
verdict whose citation fails verification is discarded.

A verdict carries the hash of the document that the model read, plus the model id and the prompt version,
and those three values are its identity. A verdict is stale when any of those move, and staleness is
checked before a verdict is read.

Each task runs `k` samples. Disagreement is reported as `ambiguous` rather than resolved by majority,
because disagreement is the available signal that a claim means two things.

Judge verdicts report and do not block, except where the author of the prose and the author of the
claims are known to be different.

## Consequences

The uncovered set stays visible even when the judge is optimistic, because admissibility comes from a
run rather than from a verdict.

Recording who wrote each artefact is a prerequisite for the one case where a verdict may block, and
nothing in v1 records it. That is open question 4 in `docs/spec.md`.

Sampling `k` times multiplies judge cost by `k`. The selection in the index already matches each prose
span with the claims of its own document, so the judge reads one document at a time rather than a cross
product of the claim graph.
