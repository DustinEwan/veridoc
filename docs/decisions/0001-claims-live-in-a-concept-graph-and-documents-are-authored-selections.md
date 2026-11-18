# 0001 — Claims live in a concept graph; documents are authored selections

Status: accepted, 2026-11-16. Amended the same day: the claim source moved back to a sidecar beside the
code, and documents stopped being generated.

## Context

A fact about a system does not follow the file layout of the implementation, and it does not follow the
reading order of a person who needs it. A layout that keeps an assertion in one of those shapes makes a
reader copy that assertion into every other place that shape reaches. Copies drift on their own schedules,
which is the inconsistency that the whole project exists to remove.

Two layouts both fail on their own. A sidecar per source file is where review is local, but it groups
assertions by implementation file, so a cross-cutting fact, such as an authorisation rule that spans thirty
files, has no home: an author puts it in whichever sidecar they happened to be editing and the next author
repeats it. A directory organised by concept fixes that, but it moves documentation away from the code that
it describes and the reviewer stops connecting the two.

Generating every document from the claim graph fails differently. Architecture prose does not belong in a
sidecar, and a generated file cannot be edited, so a paragraph of reasoning has nowhere to go.

## Decision

veridoc keeps two structures and never merges them.

The claim graph is organised by concept. A claim exists once, and its identity is its subject joined with its
predicate as written, which no other claim shares; the name every surface prints is that subject joined with a slug of
the predicate. The subject is named by the author rather than derived from a path. Its source lives in the
sidecar beside the code the author is writing, and a concept field places it in the graph, so locality and grouping are
separate decisions rather than a choice between them.

The document tree is declared in `index.doc.<ext>` and is organised by audience. A document node states an audience,
one question, and the path of a `.spec.md` file. Those files are written by a person or an agent and are never written
by the engine. A section names the subject its prose is about and references claims inside its sentences,
and the engine injects the text of each referenced claim at the reference the author placed.

## Consequences

Review locality survives, which was the cost the earlier version of this record accepted. The claim beside
the code and the concept field both exist, so a reviewer reads the sidecar and the graph membership in the
same diff.

Prose is editable and is still accountable. A section owns the prose it authors, the claim owns the assertion, and the
engine reports a claim that no section names, so the freedom to write a paragraph does not become a place where nothing
is checked.

Duplication becomes a hard error rather than a smell, because a second claim with an existing identity is rejected and
points at the owner, and a fact needed in two documents is one claim named twice.

A change to the set of documents is an edit to the index, so a decision about scope is always a visible diff
rather than a new file appearing.
