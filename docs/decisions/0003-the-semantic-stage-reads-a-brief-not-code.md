# 0003 — The semantic stage reads a brief, not code

Status: accepted, 2026-11-16. Amended the same day: the artifact a model reads is called a brief, because
`.spec.md` files are documents too and the two words were doing different jobs under one name.

## Context

Two analyses run over the same claims. The first is deterministic: does a referent resolve, does the claim
restate something visible in a declaration, does the document tree still hold its obligations, does the
assertion attached to a claim hold over the values that a scenario produced. The second is semantic: does the
prose appear to mean what it is taken to mean, is it supported by the claims it selects, is it broader than
those claims.

The two stages need opposite representations. A program reads a structure best, so the deterministic stage
should consume the sidecar source through a plugin that understands it. A model reads prose best, and a model
that is handed a syntax tree or a nested record spends its attention on the container rather than on the
claim.

Three problems appear when both stages consume one representation. The model is asked to read a structure that
was designed for a program. The semantic stage needs the language toolchain to reach the code behind a claim,
which makes the boundary between the two analyses nominal rather than real. The expensive stage has no key to
cache on, so every run spends model calls again.

## Decision

veridoc runs the two stages against two artifacts with two audiences.

The deterministic stage consumes the sidecar through a plugin and, when it passes, assembles a brief for a
reader. That brief holds the written text of one section, the text of the claims that section selects, the
resolved signature in words, the assertion tree, and the outcomes.

The semantic stage consumes only that brief. It never calls a plugin, and it must be able to run on a machine
with no language toolchain installed. That requirement is the test of whether the boundary is real: if the
semantic stage reaches back for a referent, the boundary is decorative.

The brief is assembled and owned by the engine. It is an output, never an input, and the markup that carries it
is not part of any contract. A `.spec.md` file is a different artifact: a person or an agent writes it, and the
engine reads it.

A verdict is replayable, so the brief that produced it is kept beside the verdict. A stored brief is evidence
about a past run and is never read as input to a new one.

## Consequences

The cache key is one section together with the claims that section selects, rather than a whole file, so
editing one sentence does not re-judge a document. Model id, prompt version, and the referent revision are
recorded in the same key because they change without the prose changing.

Assembling on demand and storing for replay pull in opposite directions. A brief built on demand is true of the
claim graph at that moment, and a stored brief stops being true of the current graph as soon as the graph
moves. The rule that keeps them apart is that a stored brief is only ever read as history.

Anything that appears in the brief only as an identifier is invisible to the stage that has to reason about it.
A signature appears in words, and a hash appears only where the engine, not a model, will read it.

A claim can pass every deterministic rule and still say nothing that any code could contradict. The
deterministic stage cannot detect that. An earlier draft gave the semantic stage a fourth question for it, and
that question was dropped, because it asked the model to invent a change rather than to compare two artifacts
that already exist. A claim of that kind now appears as prose that no claim states.
