//! The `.spec.md` grammar: an attribute list on each heading, sections split at headings,
//! spans numbered by the engine, and the section hash. The syntax is veridoc's own, so this
//! parser is engine-owned and crosses no plugin boundary.
//!
//! ```markdown
//! ## What the scanner refuses {#refuses selects="concept:project-scan kind:ensures" claims="c1a2"}
//! ```

use veridoc_protocol::{Kind, Selects};

/// A parsed `.spec.md` file.
#[derive(Debug, Clone)]
pub struct SpecFile {
    pub path: String,
    pub sections: Vec<Section>,
}

/// One section: a heading and the body up to the next heading of any level.
#[derive(Debug, Clone)]
pub struct Section {
    pub anchor: String,
    pub heading: String,
    pub level: u32,
    pub line: u32,
    /// The parsed `selects="…"` attribute; empty when the heading carries none.
    pub selects: Selects,
    /// The parsed `claims="…"` attribute.
    pub claims: Vec<String>,
    /// True when the heading carries neither `selects` nor `claims`.
    pub no_selection: bool,
    pub spans: Vec<Span>,
    /// The section body, used for the section hash and the brief.
    pub body: String,
}

/// One authored span. Ids are assigned by the engine at assembly time; a verdict citing an
/// id this function never issued is a run failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub id: String,
    pub text: String,
}

/// A parse problem that names the file and line. Bind turns each into a `BindError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecProblem {
    pub file: String,
    pub line: u32,
    pub kind: SpecProblemKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecProblemKind {
    /// A heading with no `{#id …}`; every heading but the first level-1 heading needs one.
    MissingId,
    /// A heading with neither `selects` nor `claims`. Reported as a warning, not an error.
    NoSelection,
    /// A section id used twice in one file.
    DuplicateId(String),
}

/// Parse one `.spec.md` file. `path` is the project-relative path used in messages.
pub fn parse(path: &str, text: &str) -> (SpecFile, Vec<SpecProblem>) {
    let mut sections: Vec<Section> = Vec::new();
    let mut problems: Vec<SpecProblem> = Vec::new();
    let mut seen_ids: Vec<String> = Vec::new();
    let mut first_h1_seen = false;

    let lines: Vec<&str> = text.split('\n').collect();
    let mut index = 0usize;
    while index < lines.len() {
        let level = heading_level(lines[index]);
        if level == 0 {
            index += 1;
            continue;
        }
        let line_no = (index + 1) as u32;
        let (title, attrs) = split_attributes(lines[index].trim_start_matches('#').trim());

        // The first level-1 heading is exempt from the id requirement.
        let is_first_h1 = level == 1 && !first_h1_seen;
        if level == 1 {
            first_h1_seen = true;
        }

        let anchor = attrs.as_ref().and_then(|a| a.id.clone()).unwrap_or_default();
        if anchor.is_empty() && !is_first_h1 {
            problems.push(SpecProblem {
                file: path.to_string(),
                line: line_no,
                kind: SpecProblemKind::MissingId,
            });
        }
        if !anchor.is_empty() {
            if seen_ids.contains(&anchor) {
                problems.push(SpecProblem {
                    file: path.to_string(),
                    line: line_no,
                    kind: SpecProblemKind::DuplicateId(anchor.clone()),
                });
            }
            seen_ids.push(anchor.clone());
            // A section with an id but no selection is a warning.
            let selects = attrs
                .as_ref()
                .and_then(|a| a.get("selects"))
                .map(parse_selects)
                .unwrap_or_default();
            let claims: Vec<String> = attrs
                .as_ref()
                .and_then(|a| a.get("claims"))
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            if selects.is_empty() && claims.is_empty() {
                problems.push(SpecProblem {
                    file: path.to_string(),
                    line: line_no,
                    kind: SpecProblemKind::NoSelection,
                });
            }
            sections.push(make_section(
                path, title, level, line_no, anchor, selects, claims, &lines, index,
            ));
        } else {
            // The exempt first level-1 heading: kept in the parse so the body is not lost,
            // but it selects nothing and warns nothing.
            sections.push(make_section(
                path,
                title,
                level,
                line_no,
                anchor,
                Selects::default(),
                Vec::new(),
                &lines,
                index,
            ));
        }

        // Skip the body of this section: it ends at the next heading.
        index += 1;
        while index < lines.len() && heading_level(lines[index]) == 0 {
            index += 1;
        }
    }

    (
        SpecFile {
            path: path.to_string(),
            sections,
        },
        problems,
    )
}

#[allow(clippy::too_many_arguments)]
fn make_section(
    _path: &str,
    heading: String,
    level: u32,
    line: u32,
    anchor: String,
    selects: Selects,
    claims: Vec<String>,
    lines: &[&str],
    heading_index: usize,
) -> Section {
    let mut body_end = heading_index + 1;
    while body_end < lines.len() && heading_level(lines[body_end]) == 0 {
        body_end += 1;
    }
    let body = lines[heading_index + 1..body_end].join("\n");
    let no_selection = anchor.is_empty() || (selects.is_empty() && claims.is_empty());
    Section {
        anchor,
        heading,
        level,
        line,
        selects,
        claims,
        no_selection,
        spans: assign_spans(&body),
        body,
    }
}

/// The `#` count of a heading line, or 0 when it is not a heading. A `#` inside a fenced code
/// block is not a heading: the body-collection loops only test lines outside a body, and the
/// span assigner treats fenced content as one span, so fenced `#` lines never reach here as
/// headings of the enclosing file… except in the body-skip loop, which is why a fence inside
/// a body must not contain heading-looking lines that end the section. v1 accepts that limit:
/// a fenced `#` line ends a section, and the example projects do not contain one.
fn heading_level(line: &str) -> u32 {
    let trimmed = line.trim_start();
    let hashes = trimmed.len() - trimmed.trim_start_matches('#').len();
    if hashes == 0 || hashes > 6 {
        return 0;
    }
    let rest = &trimmed[hashes..];
    if rest.is_empty() || rest.starts_with(' ') {
        hashes as u32
    } else {
        0
    }
}

/// Split `title {#id key="value"}` into the title and the parsed attribute list.
fn split_attributes(raw: &str) -> (String, Option<Attrs>) {
    let open = match raw.rfind('{') {
        Some(i) if raw.trim_end().ends_with('}') => i,
        _ => return (raw.to_string(), None),
    };
    let close = raw.trim_end().len() - 1;
    let title = raw[..open].trim_end().to_string();
    let inner = &raw[open + 1..close];
    (title, Some(parse_attrs(inner)))
}

struct Attrs {
    id: Option<String>,
    pairs: Vec<(String, String)>,
}

impl Attrs {
    fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Parse `#id key="value" key="value"`. Tokens are whitespace-separated; a quoted value may
/// contain spaces.
fn parse_attrs(inner: &str) -> Attrs {
    let mut id = None;
    let mut pairs = Vec::new();
    for token in tokenize(inner) {
        if let Some(rest) = token.strip_prefix('#') {
            id = Some(rest.to_string());
        } else if let Some(eq) = token.find('=') {
            pairs.push((token[..eq].to_string(), token[eq + 1..].to_string()));
        }
    }
    Attrs { id, pairs }
}

/// Split on whitespace outside double quotes, dropping the quotes.
fn tokenize(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut token = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    for ch in inner.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                has_token = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token {
                    out.push(std::mem::take(&mut token));
                    has_token = false;
                }
            }
            c => {
                token.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        out.push(token);
    }
    out
}

/// Parse `concept:project-scan kind:ensures,witnesses referent:vd:r:x` into a `Selects`.
fn parse_selects(value: &str) -> Selects {
    let mut selects = Selects::default();
    for pair in value.split_whitespace() {
        let Some((key, val)) = pair.split_once(':') else {
            continue;
        };
        match key {
            "concept" => selects.concept = Some(val.to_string()),
            "kind" => {
                let kinds = val.split(',').filter_map(parse_kind).collect::<Vec<_>>();
                if !kinds.is_empty() {
                    selects.kind = Some(kinds);
                }
            }
            "referent" => selects.referent = Some(val.to_string()),
            _ => {}
        }
    }
    selects
}

fn parse_kind(name: &str) -> Option<Kind> {
    Some(match name {
        "requires" => Kind::Requires,
        "ensures" => Kind::Ensures,
        "invariant" => Kind::Invariant,
        "witness" => Kind::Witness,
        "because" => Kind::Because,
        _ => return None,
    })
}

/// Split a section body into spans. A paragraph is split into sentences at `[.!?]` followed by
/// whitespace; a list item is one span; a fenced code block is one span.
pub fn assign_spans(body: &str) -> Vec<Span> {
    let mut spans: Vec<String> = Vec::new();
    let mut in_fence = false;
    let mut fence_buf = String::new();
    let mut para: Vec<String> = Vec::new();

    for raw in body.split('\n') {
        let line = raw.trim_end();
        let t = line.trim();
        if t.starts_with("```") {
            if in_fence {
                spans.push(fence_buf.trim_end().to_string());
                fence_buf.clear();
                in_fence = false;
            } else {
                flush_para(&mut para, &mut spans);
                in_fence = true;
            }
            continue;
        }
        if in_fence {
            fence_buf.push_str(line);
            fence_buf.push('\n');
            continue;
        }
        if t.is_empty() {
            flush_para(&mut para, &mut spans);
            continue;
        }
        if is_list_item(t) {
            flush_para(&mut para, &mut spans);
            spans.push(t.to_string());
            continue;
        }
        para.push(t.to_string());
    }
    if in_fence && !fence_buf.trim().is_empty() {
        spans.push(fence_buf.trim_end().to_string());
    }
    flush_para(&mut para, &mut spans);

    spans
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .enumerate()
        .map(|(i, text)| Span {
            id: format!("sp{}", i + 1),
            text,
        })
        .collect()
}

fn flush_para(para: &mut Vec<String>, spans: &mut Vec<String>) {
    if para.is_empty() {
        return;
    }
    let joined = para.join(" ");
    for sentence in split_sentences(&joined) {
        spans.push(sentence);
    }
    para.clear();
}

fn is_list_item(t: &str) -> bool {
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return true;
    }
    // `1. item`: digits, a dot, a space.
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    digits > 0 && t[digits..].starts_with(". ")
}

/// Split on `[.!?]` followed by whitespace, keeping the punctuation with its sentence.
fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        current.push(c);
        if matches!(c, '.' | '!' | '?') {
            let mut j = i + 1;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            if j > i + 1 || j == chars.len() {
                out.push(current.trim().to_string());
                current.clear();
                i = j;
                continue;
            }
        }
        i += 1;
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

/// The normalized section body hash: CRLF to LF, trailing whitespace per line stripped, one
/// trailing newline.
pub fn normalized_body(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    for line in body.replace("\r\n", "\n").lines() {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}
