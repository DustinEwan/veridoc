# 0005 — Every claim is evaluated or the build fails

Status: accepted, 2026-11-16.

## Context

An earlier draft let a claim sit in a state meaning that nothing had evaluated it, and reported that state
as a coverage figure. That state communicated nothing. It was not a statement about the claim, the code, or
the sentence, and because it was tolerated, a claim could exist forever without ever being checked, which is
the same condition that veridoc exists to detect, recorded as a passing run.

This is a build-time check and it ends in a result, so every path that a claim can take has to mean something.
Three kinds of evidence exist and they are not interchangeable. A scenario produces values and compares them
with an assertion. An artefact resolves and matches a recorded hash. A judge compares a section with the
claims that it selects. Anything else is not evidence.

One of those channels has a gate in front of it. An assertion that cannot fail is not evidence, so a claim
may only take the scenario channel when its assertion is bivalent, which means both outcomes are attainable
over the declared types of its observables. That property is decidable for a structured assertion and for
code inside a small expression shape, and undecidable for arbitrary code, which is the reason that the second
channel exists rather than being a fallback for laziness.

## Decision

A claim declares exactly one channel, and a claim with two channels or none is a bind error. Every claim ends
in `supported`, `contradicted`, or `ambiguous`, and every other path is a build error that reports the missing
thing: a referent that does not resolve, a checkable claim with no scenario, a semantic claim with no verdict,
an action that calls something other than the claim's referent, an assertion that reads an observable the
action cannot produce, an assertion that cannot fail.

A run failure is not a claim outcome. A build that will not compile, a plugin that will not start, a fixture
that cannot be constructed, and a scenario that outlives its deadline are recorded once at scenario level and
make the run red. They never produce a claim state, because one broken build would otherwise print thousands
of unevidenced claims and teach a reader to ignore the report.

Coverage is a figure that the project sets and that the run compares, so the requirement rises over time
instead of being complete on the first day.

## Consequences

The judge stops being advisory. Every sentence that no scenario reaches is judged, so the verdict cache key
becomes per section rather than per document, otherwise editing one sentence re-judges a file, and the
sampling default becomes one because a second sample of a converging model buys nothing.

The residue narrows to what it should have been. It no longer holds claims that nobody got round to
evaluating, because those are errors. It holds the claims that no channel can reach, which in v1 means a
provenance claim whose cited text cannot be read and a semantic claim whose verdict is missing, stale, or
ambiguous.

Nothing detects an assertion that can fail but tests the wrong thing. Bivalence is the strongest property
available at build time and it is weaker than correctness, so a bivalent assertion beside a sentence that
says more than it checks is a documentation error, and the only thing that reaches it is the comparison
between the sentence and the assertion tree.
