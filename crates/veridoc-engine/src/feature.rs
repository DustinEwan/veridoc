//! The `.feature` grammar. Gherkin here is veridoc's own format, not the target language, so
//! parsing it in the engine does not cross the boundary `docs/spec.md` protects.
//!
//! ```gherkin
//! Feature: project scan
//!
//!   @id:config/rejects-missing-root
//!   Scenario: A missing project root is refused
//!     Given the empty-directory fixture
//!     When afz probe-plugin runs in that directory
//! ```
//!
//! A `Scenario:` line must carry a `@id:<slug>` tag, and the slug is the scenario id:
//! `docs/spec.md` refers to `scenario("config/rejects-missing-root")`, and slugifying the
//! scenario name does not produce that id. `Given` names a fixture, `When` names an action,
//! and a step of any other Gherkin kind — a `Then` above all, because the claim owns the
//! assertion — is a parse error naming the file and line.

/// A parsed `.feature` file.
#[derive(Debug, Clone)]
pub struct FeatureFile {
    pub path: String,
    pub scenarios: Vec<Scenario>,
}

/// One scenario: its id from the `@id:` tag, its name, and its two steps.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub id: String,
    pub name: String,
    pub line: u32,
    /// The `Given` step text; it must name a declared fixture.
    pub given: Option<Step>,
    /// The `When` step text; it must name a declared action.
    pub when: Option<Step>,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub text: String,
    pub line: u32,
}

/// A grammar violation naming the file and line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureProblem {
    pub file: String,
    pub line: u32,
    pub kind: FeatureProblemKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureProblemKind {
    /// A `Scenario:` with no `@id:<slug>` tag above it.
    MissingTag,
    /// A step that is not `Given` or `When`: `Then`, `And`, `But`, `*`.
    ForbiddenStep(String),
    /// A second `Given` in one scenario.
    SecondGiven,
    /// A second `When` in one scenario.
    SecondWhen,
    /// A `Scenario:` line with no name, or a step with no text.
    Empty(String),
    /// A line the grammar does not recognize at all.
    Unrecognized(String),
}

/// Parse one `.feature` file.
pub fn parse(path: &str, text: &str) -> (FeatureFile, Vec<FeatureProblem>) {
    let mut scenarios: Vec<Scenario> = Vec::new();
    let mut problems: Vec<FeatureProblem> = Vec::new();
    let mut pending_tag: Option<String> = None;
    let mut current: Option<Scenario> = None;

    for (i, raw) in text.split('\n').enumerate() {
        let line = raw.trim();
        let line_no = (i + 1) as u32;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('@') {
            // A tag line. Only `@id:<slug>` is meaningful; other tags are ignored.
            if let Some(slug) = rest_of_tag(line) {
                pending_tag = Some(slug);
            }
            continue;
        }
        if line.starts_with("Feature:") {
            flush(&mut current, &mut scenarios);
            continue;
        }
        if let Some(rest) = line.strip_prefix("Scenario:") {
            flush(&mut current, &mut scenarios);
            let name = rest.trim();
            if name.is_empty() {
                problems.push(problem(
                    path,
                    line_no,
                    FeatureProblemKind::Empty("scenario name".into()),
                ));
            }
            let id = match pending_tag.take() {
                Some(slug) => slug,
                None => {
                    problems.push(problem(path, line_no, FeatureProblemKind::MissingTag));
                    String::new()
                }
            };
            current = Some(Scenario {
                id,
                name: name.to_string(),
                line: line_no,
                given: None,
                when: None,
            });
            continue;
        }
        let step = if let Some(t) = strip_step(line, "Given") {
            ("Given", t)
        } else if let Some(t) = strip_step(line, "When") {
            ("When", t)
        } else if let Some(t) = first_word_step(line, &["Then", "And", "But", "*"]) {
            ("other", t)
        } else {
            problems.push(problem(
                path,
                line_no,
                FeatureProblemKind::Unrecognized(line.to_string()),
            ));
            continue;
        };
        let Some(scenario) = current.as_mut() else {
            problems.push(problem(
                path,
                line_no,
                FeatureProblemKind::Unrecognized(line.to_string()),
            ));
            continue;
        };
        let (kind, text) = step;
        match kind {
            "Given" => match &scenario.given {
                Some(_) => problems.push(problem(path, line_no, FeatureProblemKind::SecondGiven)),
                None if text.is_empty() => problems.push(problem(
                    path,
                    line_no,
                    FeatureProblemKind::Empty("Given step".into()),
                )),
                None => {
                    scenario.given = Some(Step {
                        text: text.to_string(),
                        line: line_no,
                    })
                }
            },
            "When" => match &scenario.when {
                Some(_) => problems.push(problem(path, line_no, FeatureProblemKind::SecondWhen)),
                None if text.is_empty() => problems.push(problem(
                    path,
                    line_no,
                    FeatureProblemKind::Empty("When step".into()),
                )),
                None => {
                    scenario.when = Some(Step {
                        text: text.to_string(),
                        line: line_no,
                    })
                }
            },
            _ => problems.push(problem(
                path,
                line_no,
                FeatureProblemKind::ForbiddenStep(line.to_string()),
            )),
        }
    }
    flush(&mut current, &mut scenarios);
    (
        FeatureFile {
            path: path.to_string(),
            scenarios,
        },
        problems,
    )
}

fn problem(file: &str, line: u32, kind: FeatureProblemKind) -> FeatureProblem {
    FeatureProblem {
        file: file.to_string(),
        line,
        kind,
    }
}

/// The slug of an `@id:<slug>` tag on a tag line, if it carries one.
fn rest_of_tag(line: &str) -> Option<String> {
    line.split_whitespace().find_map(|tag| {
        tag.strip_prefix("@id:")
            .map(|slug| slug.trim_end_matches(',').to_string())
    })
}

fn flush(current: &mut Option<Scenario>, scenarios: &mut Vec<Scenario>) {
    if let Some(scenario) = current.take() {
        // A scenario with no `When` acts on nothing and one with no `Given` builds no world;
        // bind reports each as the vocabulary error naming the missing thing.
        scenarios.push(scenario);
    }
}

fn strip_step<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(keyword)?;
    if rest.is_empty() || rest.starts_with(' ') {
        Some(rest.trim())
    } else {
        None
    }
}

fn first_word_step<'a>(line: &'a str, keywords: &[&'a str]) -> Option<&'a str> {
    for kw in keywords {
        if *kw == "*" {
            if let Some(rest) = line.strip_prefix('*') {
                return Some(rest.trim());
            }
            continue;
        }
        if let Some(rest) = strip_step(line, kw) {
            return Some(rest);
        }
    }
    None
}
