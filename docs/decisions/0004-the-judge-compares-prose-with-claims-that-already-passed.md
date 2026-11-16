# 0004 — The judge compares prose with claims that already passed

Status: accepted, 2026-11-16.

## Context

The pipeline has four links: prose, then claims, then claim tests, then code. Each link is checked by
the one above it, and the two kinds of check are not alike. Whether a claim holds is decidable, so a
program settles it. Whether prose faithfully states the claims is not decidable, so a model answers it.

An earlier draft gave the judge four tasks, and two of them were wrong in ways that this division
exposes.

The first asked whether a scenario would fail if a claim were false. That is a question about the
strength of a test rather than about the sentence that the claim supports, which puts it in correctness
testing of the system, so it was dropped.

The second asked the model for the smallest change to a referent that a claim would exclude. That
asks a model to invent something. The judge would then have produced an artifact that the rest of the
pipeline consumes, which makes it an author rather than a checker, and an author is what this whole
design exists to distrust.

## Decision

Whether a claim holds is settled by the deterministic stage. A claim test supports the prose and runs
deterministically, and a failure has three causes: the code is wrong, the claim test is wrong, or the
environment is wrong, because a fixture broke, a dependency moved, or a service was not up. The third
is not a finding about the project.

Only claims with admissible evidence reach the document, so the judge never sees a claim that no test
supports.

Two tasks remain over that material, and both compare prose against claims. S1 reports the claims that
prose does not mention, and the assertions in prose that no claim states. S2 reports the pair of spans
that disagree. A third task, S3, compares the audience and question of every document node. The judge
proposes no change to code and no change to prose.

One deterministic rule sits alongside them. When a change set edits a claim test and the code that the
test binds to while the claim text stays put, the run stops and asks a person, because that change has
the shape of a test repaired to match broken code.

## Consequences

The judge's answer is only as good as the claims in front of it. Those claims are certified, so what is
left is translation rather than truth, and a model answers that narrower question more reliably.

A vacuous claim, meaning one that no code could ever contradict, is no longer detected directly. The
expectation is that it appears as prose that no claim states, which S1 finds. If that does not hold in
a pilot, the gap is real and v1 has no answer for it.

The rule about a test and its code moving together will report honest refactors too, because an honest
refactor does edit both. The run stops and asks rather than rejecting, which is the cheapest answer that
does not also push an agent to split the edit across two commits and evade the rule entirely.
