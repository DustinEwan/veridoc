//! The `.spec.md` grammar: `<doc-section>` opens a section, `<doc-claim>` names a claim inside a
//! sentence, and the problems bind turns into errors and warnings.

use veridoc_engine::spec_md::{self, SpecProblemKind};

#[test]
fn a_section_and_its_inline_references_parse_cleanly() {
    let text = "\
<doc-section name=\"scanner\">

## What the scanner refuses

The scanner reads one directory <doc-claim name=\"rejects-missing-config\"/>. It stops at the first root.
";
    let (file, problems) = spec_md::parse("docs/a.spec.md", text);
    assert_eq!(
        problems,
        vec![],
        "a section that names a claim carries no problem"
    );
    assert_eq!(file.sections.len(), 1);
    let scanner = &file.sections[0];
    assert_eq!(scanner.subject, "scanner");
    assert_eq!(scanner.line, 1);
    assert_eq!(scanner.references.len(), 1);
    assert_eq!(scanner.references[0].predicate, "rejects-missing-config");
    assert_eq!(
        scanner.references[0].line, 5,
        "the reference sits on line 5"
    );
    // The heading is prose, so it is a span like any other line.
    assert_eq!(
        scanner.spans.len(),
        3,
        "heading plus two sentences: {:?}",
        scanner.spans
    );
    assert_eq!(scanner.spans[0].text, "## What the scanner refuses");
    assert_eq!(
        scanner.spans[1].text,
        "The scanner reads one directory <doc-claim name=\"rejects-missing-config\"/>."
    );
    assert_eq!(scanner.spans[2].text, "It stops at the first root.");
}

#[test]
fn a_section_with_no_name_is_a_problem_naming_the_line() {
    let text = "<doc-section>\n\nbody\n";
    let (_file, problems) = spec_md::parse("docs/a.spec.md", text);
    assert!(
        problems
            .iter()
            .any(|p| p.line == 1 && p.kind == SpecProblemKind::MissingSubjectName),
        "expected the missing-name problem at line 1: {problems:?}"
    );
}

#[test]
fn a_section_that_names_no_claim_warns() {
    let text = "<doc-section name=\"working\">\n\nConventions for the tree.\n";
    let (file, problems) = spec_md::parse("docs/a.spec.md", text);
    assert!(file.sections[0].no_references);
    assert!(
        problems
            .iter()
            .any(|p| p.kind == SpecProblemKind::NoReferences && p.line == 1),
        "expected a NoReferences warning: {problems:?}"
    );
}

#[test]
fn a_repeated_section_name_is_not_a_problem() {
    // Coverage is what the build enforces, so two sections may be about the same subject.
    let text = "<doc-section name=\"scanner\">\n\na\n\n<doc-section name=\"scanner\">\n\nb\n";
    let (file, problems) = spec_md::parse("docs/a.spec.md", text);
    assert_eq!(file.sections.len(), 2);
    assert!(
        problems
            .iter()
            .all(|p| p.kind != SpecProblemKind::MissingSubjectName),
        "the name is present in both: {problems:?}"
    );
}

#[test]
fn an_unknown_element_is_a_problem_naming_the_tag() {
    let text = "<doc-section name=\"s\">\n\nSome prose <doc-thing name=\"x\"/>.\n";
    let (_file, problems) = spec_md::parse("docs/a.spec.md", text);
    assert!(
        matches!(
            &problems[0].kind,
            SpecProblemKind::UnknownElement(tag) if tag == "doc-thing"
        ),
        "expected an UnknownElement problem: {problems:?}"
    );
    assert_eq!(problems[0].line, 3);
}

#[test]
fn doc_each_and_doc_value_are_accepted_and_inert() {
    let text = "<doc-section name=\"s\">\n\n<doc-each over=\"files\">\n<doc-value of=\"files\"/>\n</doc-each>\n";
    let (file, problems) = spec_md::parse("docs/a.spec.md", text);
    assert!(
        problems
            .iter()
            .all(|p| p.kind != SpecProblemKind::UnknownElement("doc-each".to_string())),
        "the assembly step resolves these later: {problems:?}"
    );
    assert_eq!(file.sections[0].references.len(), 0);
}

#[test]
fn an_opener_must_sit_on_a_line_of_its_own() {
    let text = "<doc-section name=\"s\"> and prose after it\n\nbody\n";
    let (file, problems) = spec_md::parse("docs/a.spec.md", text);
    assert_eq!(file.sections.len(), 0, "no section opened");
    assert!(
        matches!(
            &problems[0].kind,
            SpecProblemKind::MisplacedElement(tag) if tag == "doc-section"
        ),
        "expected a MisplacedElement problem: {problems:?}"
    );
}

#[test]
fn a_code_sample_shows_the_grammar_without_using_it() {
    let text = "\
<doc-section name=\"grammar\">

This sample is not a section and names no claim:

```md
<doc-section name=\"sample\">
<doc-claim name=\"in-sample\"/>
```

The real reference is here <doc-claim name=\"real\"/>.
";
    let (file, problems) = spec_md::parse("docs/a.spec.md", text);
    assert_eq!(file.sections.len(), 1, "the fenced opener is prose");
    assert_eq!(problems, vec![]);
    assert_eq!(
        file.sections[0]
            .references
            .iter()
            .map(|r| r.predicate.as_str())
            .collect::<Vec<_>>(),
        vec!["real"]
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
fn two_references_in_one_section_are_kept_in_the_order_written() {
    let text = "<doc-section name=\"s\">\n\nOne <doc-claim name=\"b\"/> then <doc-claim name=\"a\"/>.\n\nAgain <doc-claim name=\"b\"/>.\n";
    let (file, _problems) = spec_md::parse("docs/a.spec.md", text);
    assert_eq!(
        file.sections[0]
            .references
            .iter()
            .map(|r| r.predicate.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "a", "b"]
    );
}

#[test]
fn normalization_strips_trailing_space_and_makes_one_trailing_newline() {
    let normalized = spec_md::normalized_body("line one    \r\nline two");
    assert_eq!(normalized, "line one\nline two\n");
}
