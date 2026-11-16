//! The `.feature` grammar: the `@id:` tag is the scenario id, one `Given` and one `When`, and
//! every other Gherkin construct is a problem naming the file and line.

use veridoc_engine::feature::{self, FeatureProblemKind};

const GOOD: &str = "Feature: project scan\n\n  @id:config/rejects-missing-root\n  Scenario: A missing project root is refused\n    Given the empty-directory fixture\n    When the run-probe action runs\n";

#[test]
fn a_conforming_scenario_parses_with_its_tag_id() {
    let (file, problems) = feature::parse("features/cli.feature", GOOD);
    assert!(problems.is_empty(), "unexpected problems: {problems:?}");
    assert_eq!(file.scenarios.len(), 1);
    let s = &file.scenarios[0];
    assert_eq!(s.id, "config/rejects-missing-root");
    assert_eq!(s.name, "A missing project root is refused");
    assert!(s.given.is_some());
    assert!(s.when.is_some());
}

#[test]
fn a_scenario_without_a_tag_is_a_problem() {
    let text = "Feature: f\n\n  Scenario: no tag\n    Given x\n    When y\n";
    let (_file, problems) = feature::parse("features/c.feature", text);
    assert!(
        problems
            .iter()
            .any(|p| p.kind == FeatureProblemKind::MissingTag && p.line == 3),
        "expected MissingTag at line 3: {problems:?}"
    );
}

#[test]
fn a_then_step_is_rejected_because_the_claim_owns_the_assertion() {
    let text = "Feature: f\n\n  @id:s\n  Scenario: has a then\n    Given x\n    When y\n    Then z holds\n";
    let (_file, problems) = feature::parse("features/c.feature", text);
    assert!(
        problems
            .iter()
            .any(|p| matches!(&p.kind, FeatureProblemKind::ForbiddenStep(t) if t == "Then z holds")),
        "expected a ForbiddenStep for the Then: {problems:?}"
    );
}

#[test]
fn a_second_given_and_a_second_when_are_problems() {
    let text = "Feature: f\n\n  @id:s\n  Scenario: two of each\n    Given a\n    Given b\n    When c\n    When d\n";
    let (_file, problems) = feature::parse("features/c.feature", text);
    assert!(
        problems.iter().any(|p| p.kind == FeatureProblemKind::SecondGiven),
        "expected SecondGiven: {problems:?}"
    );
    assert!(
        problems.iter().any(|p| p.kind == FeatureProblemKind::SecondWhen),
        "expected SecondWhen: {problems:?}"
    );
}
