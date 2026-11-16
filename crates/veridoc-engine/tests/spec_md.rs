//! The `.spec.md` grammar: ids on headings, the selection attribute list, spans, and the
//! problems bind turns into errors and warnings.

use veridoc_engine::spec_md::{self, SpecProblemKind};

#[test]
fn a_section_with_an_id_and_selection_parses_cleanly() {
    let text = "# Title\n\n## What the scanner refuses {#refuses selects=\"concept:project-scan kind:ensures\"}\n\nThe scanner walks upward. It stops at the first root.\n";
    let (file, problems) = spec_md::parse("docs/a.spec.md", text);
    assert!(
        problems.is_empty(),
        "unexpected problems: {problems:?}"
    );
    // The title carries no id and is exempt; the section carries one.
    assert_eq!(file.sections.len(), 2);
    let refuses = &file.sections[1];
    assert_eq!(refuses.anchor, "refuses");
    assert_eq!(refuses.heading, "What the scanner refuses");
    assert_eq!(refuses.selects.concept.as_deref(), Some("project-scan"));
    assert_eq!(refuses.spans.len(), 2, "two sentences are two spans");
    assert_eq!(refuses.spans[0].id, "sp1");
    assert_eq!(refuses.spans[0].text, "The scanner walks upward.");
    assert_eq!(refuses.spans[1].text, "It stops at the first root.");
}

#[test]
fn a_heading_without_an_id_is_a_problem_naming_the_line() {
    let text = "# Title\n\n## No id here\n\nbody\n";
    let (_file, problems) = spec_md::parse("docs/a.spec.md", text);
    assert_eq!(
        problems,
        vec![spec_md::SpecProblem {
            file: "docs/a.spec.md".into(),
            line: 3,
            kind: SpecProblemKind::MissingId,
        }]
    );
}

#[test]
fn a_section_with_an_id_but_no_selection_warns() {
    let text = "# Title\n\n## Just an id {#only}\n\nbody\n";
    let (_file, problems) = spec_md::parse("docs/a.spec.md", text);
    assert!(
        problems
            .iter()
            .any(|p| p.kind == SpecProblemKind::NoSelection && p.line == 3),
        "expected a NoSelection warning: {problems:?}"
    );
}

#[test]
fn a_duplicate_section_id_is_a_problem() {
    let text = "# T\n\n## A {#dup}\nx\n\n## B {#dup}\ny\n";
    let (_file, problems) = spec_md::parse("docs/a.spec.md", text);
    assert!(
        problems
            .iter()
            .any(|p| matches!(&p.kind, SpecProblemKind::DuplicateId(id) if id == "dup")),
        "expected a DuplicateId problem: {problems:?}"
    );
}

#[test]
fn a_list_item_and_a_code_block_are_each_one_span() {
    let body = "- first item\n- second item\n\n```ts\nconst x = 1;\nconst y = 2;\n```\n";
    let spans = spec_md::assign_spans(body);
    assert_eq!(spans.len(), 3, "two items plus one code block: {spans:?}");
    assert_eq!(spans[0].text, "- first item");
    assert_eq!(spans[1].text, "- second item");
    assert!(spans[2].text.contains("const x = 1;"));
    assert!(spans[2].text.contains("const y = 2;"));
}

#[test]
fn claims_attribute_is_parsed_into_a_list() {
    let text = "# T\n\n## S {#s claims=\"c1a2,c3d4\"}\nbody\n";
    let (file, _problems) = spec_md::parse("docs/a.spec.md", text);
    assert_eq!(file.sections[1].claims, vec!["c1a2", "c3d4"]);
}

#[test]
fn normalization_strips_trailing_space_and_makes_one_trailing_newline() {
    let normalized = spec_md::normalized_body("line one    \r\nline two");
    assert_eq!(normalized, "line one\nline two\n");
}
