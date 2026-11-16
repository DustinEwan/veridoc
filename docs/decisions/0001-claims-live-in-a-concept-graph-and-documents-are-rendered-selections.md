# 0001 — Claims live in a concept graph; documents are rendered selections

Status: accepted, 2026-11-16.

## Context

A fact about a system does not follow the file layout of the implementation, and it does not follow
the reading order of a person who needs it. A layout that keeps an assertion in one of those shapes
makes a reader copy that assertion into every other place that shape reaches. Copies drift on their own
schedules, which is the inconsistency that the whole project exists to remove.

The obvious layout is a sidecar per source file, because that is where the code is and review is
local. That layout groups assertions by implementation file, so a cross-cutting fact, such as an
authorisation rule that spans thirty files, has no home. An author puts it in whichever sidecar they
happened to be editing, and the next author repeats the fact in theirs.

## Decision

veridoc keeps two structures and never merges them.

The claim graph is organised by concept. A claim exists once, and `sha(subjectId, predicate)` gives it
an identity that no other claim shares. Its subject may be a symbol in any part of the tree.

The document tree is declared in `index.doc.ts` and is organised by audience. A document node holds an
audience, one question, and a selection of claims. It holds no prose. Every readable document, the
agent rule files included, is a rendered selection.

## Consequences

Review locality moves from the source layout to the generated output, which renders a view per source
file. A reviewer reads a per-file view and edits a concept file, and those are different files. This
is the main cost of the decision, and it is the one that a reviewer is most likely to resent.

Duplication becomes a hard error rather than a smell, because a second claim with an existing identity
is rejected and points at the owner.

A change to the set of documents is an edit to the index, so a decision about scope is always a visible
diff rather than a new file appearing.
