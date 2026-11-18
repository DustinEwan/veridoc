# 0004 — The judge compares prose with claims that already passed

Status: accepted, 2026-11-16.

## Context

The pipeline has four links: prose, then claims, then scenarios, then code. Each link is checked by
the one above it, and the two kinds of check are not alike. Whether a bivalent assertion holds over the values
that a scenario produced is decidable, so a program settles it. Whether prose faithfully states the claims is
not decidable, so a model answers it, and the author decides which of the two a claim asks by declaring exactly one
channel for it.

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

Whether a bivalent claim holds is settled by the deterministic stage. A claim declares exactly one channel, and an
assertion that cannot fail is a build error rather than a referral to the judge. The outcome for a claim is one of
three states: `supported`, `contradicted`, or `ambiguous`, and `ambiguous` is not a truth value but an absence of one.
A build that will not compile stops the run, and a fixture that cannot be constructed or a scenario that outlives its
deadline is recorded as a run failure against that claim and never as one of its three states, while every other claim
keeps its verdict. One broken build would otherwise print thousands of unevidenced claims and teach a reader to ignore
the report, which is why the build alone still stops the run.

The evidence state of every claim travels with it into the brief, so the judge reads the outcome rather
than guessing at it.

Two of the three tasks read that material, and both compare prose against claims. S1 reports the assertions in prose
that no claim states, because a claim that no section names is a blocking error before the judge runs. S2 reports the
pair of spans
that disagree. A third task, S3, compares the audience and question of every document node. The judge
proposes no change to code and no change to prose.

One deterministic rule sits alongside them. When a change set edits a scenario, or the assertion of a claim, and
the code that the scenario binds to, while the claim text stays put, the run stops and asks a person, because
that change has the shape of evidence rewritten to match broken code.

## Consequences

The judge's answer is only as good as the claims in front of it. Each carries the state its channel produced, and a
claim in state `ambiguous` carries the absence of one, so what is left for the rest is translation rather than truth,
and a model answers that narrower question more reliably.

A vacuous assertion, meaning one that cannot fail, is caught by the bivalence gate before a claim may take the
scenario channel. What is still undetected is an assertion that can fail but tests the wrong thing, which is a
bivalent assertion beside a sentence that says more than the assertion checks. That gap sits between `text` and
`asserts`, and only the comparison that the judge already makes reaches it.

The rule about a scenario and its code moving together will report honest refactors too, because an honest
refactor does edit both. The run stops and asks rather than rejecting, which is the cheapest answer that
does not also push an agent to split the edit across two commits and evade the rule entirely.
