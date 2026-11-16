//! The brief: the only thing the semantic stage reads. It is plain data — markdown text
//! plus an id table — with no reference to the claim graph, because `docs/decisions/0003`
//! says a stage that reaches back for a referent has no boundary. The signature appears in
//! words and the outcome travels with the claim (`docs/spec.md:495-496`); a hash appears
//! nowhere in what the model reads.

use veridoc_protocol::{Asserts, Check};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    S1,
    S2,
    S3,
}

impl Task {
    pub fn name(self) -> &'static str {
        match self {
            Task::S1 => "s1",
            Task::S2 => "s2",
            Task::S3 => "s3",
        }
    }
}

/// One authored span with the id the engine issued for it.
#[derive(Debug, Clone, PartialEq)]
pub struct BriefSpan {
    pub id: String,
    pub text: String,
}

/// One selected claim, rendered for a reader who sees no code: the subject and signature are
/// words, and the outcome the deterministic stage reached travels with the sentence.
#[derive(Debug, Clone, PartialEq)]
pub struct BriefClaim {
    pub id: String,
    pub kind: String,
    /// `supported (scenario config/rejects-missing-root)` or `judge channel`.
    pub outcome: String,
    pub text: String,
    pub subject: String,
    pub signature: String,
    pub asserts: Vec<String>,
}

/// One document node for S3: the pair of fields the duplicate test compares.
#[derive(Debug, Clone, PartialEq)]
pub struct BriefNode {
    pub path: String,
    pub audience: String,
    pub question: String,
}

/// Everything one judge task reads. `issued_ids` is the id table the verdict is re-verified
/// against before it is kept or replayed.
#[derive(Debug, Clone, PartialEq)]
pub struct Brief {
    pub task: Task,
    /// `docs/architecture.spec.md#refuses` for S1 and S2; the index for S3.
    pub section: String,
    pub audience: Option<String>,
    pub question: Option<String>,
    pub spans: Vec<BriefSpan>,
    pub claims: Vec<BriefClaim>,
    pub nodes: Vec<BriefNode>,
}

impl Brief {
    /// The markdown the judge reads.
    pub fn render(&self) -> String {
        let mut out = String::new();
        match self.task {
            Task::S3 => out.push_str("## Index\n\n"),
            _ => out.push_str(&format!("## Section {}\n\n", self.section)),
        }
        if let Some(audience) = &self.audience {
            out.push_str(&format!("Audience: {audience}\n"));
        }
        if let Some(question) = &self.question {
            out.push_str(&format!("Question: {question}\n"));
        }
        if !self.spans.is_empty() {
            out.push_str("\n### Authored prose\n");
            for span in &self.spans {
                out.push_str(&format!("{}: {}\n", span.id, span.text));
            }
        }
        if !self.claims.is_empty() {
            out.push_str("\n### Selected claims\n");
            for claim in &self.claims {
                out.push_str(&format!(
                    "{}  kind: {}  outcome: {}\n",
                    claim.id, claim.kind, claim.outcome
                ));
                out.push_str(&format!("  text: {}\n", claim.text));
                out.push_str(&format!("  subject: {}\n", claim.subject));
                out.push_str(&format!("  signature in words: {}\n", claim.signature));
                if !claim.asserts.is_empty() {
                    out.push_str("  asserts:\n");
                    for line in &claim.asserts {
                        out.push_str(&format!("    {line}\n"));
                    }
                }
            }
        }
        if !self.nodes.is_empty() {
            out.push_str("\n### Document nodes\n");
            for node in &self.nodes {
                out.push_str(&format!(
                    "{}  audience: {}  question: {}\n",
                    node.path, node.audience, node.question
                ));
            }
        }
        out
    }

    /// Every id this brief issued: span ids, claim ids, and node paths. A verdict citing
    /// anything outside this list is a run failure, not a discard.
    pub fn issued_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.spans.iter().map(|s| s.id.clone()).collect();
        ids.extend(self.claims.iter().map(|c| c.id.clone()));
        ids.extend(self.nodes.iter().map(|n| n.path.clone()));
        ids
    }
}

/// Render an assertion tree as indented lines for the brief. A `satisfies` leaf carries the
/// `(custom logic)` label so the Custom logic metric is readable from the brief alone.
pub fn render_asserts(tree: &Asserts) -> Vec<String> {
    let mut out = Vec::new();
    render_node(tree, 0, &mut out);
    out
}

fn render_node(node: &Asserts, depth: usize, out: &mut Vec<String>) {
    let pad = "  ".repeat(depth);
    match node {
        Asserts::All { all } => {
            out.push(format!("{pad}all of"));
            for child in all {
                render_node(child, depth + 1, out);
            }
        }
        Asserts::Any { any } => {
            out.push(format!("{pad}any of"));
            for child in any {
                render_node(child, depth + 1, out);
            }
        }
        Asserts::Not { not } => {
            out.push(format!("{pad}not"));
            render_node(not, depth + 1, out);
        }
        Asserts::Leaf(leaf) => {
            let check = match &leaf.r#is {
                Check::Equals(value) => format!("equals {}", render_value(value)),
                Check::Contains(value) => format!("contains {}", render_value(value)),
                Check::Satisfies(spec) => {
                    out.push(format!("{pad}{} satisfies {}", leaf.obs, spec.source));
                    out.push(format!("{pad}  (custom logic)"));
                    return;
                }
            };
            out.push(format!("{pad}{} {check}", leaf.obs));
        }
    }
}

fn render_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => format!("\"{text}\""),
        other => other.to_string(),
    }
}
