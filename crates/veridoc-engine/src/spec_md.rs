//! The `.spec.md` grammar: markdown annotated with `doc-` elements. A section opens with a
//! `<doc-section name="…">` on its own line and ends at the next one, and a claim is referenced
//! by a `<doc-claim name="…"/>` that sits inside a sentence. Headings are prose: a heading means
//! nothing to this parser, so an author may use one inside a section freely. Spans are numbered
//! by the engine, and the section hash covers the body. The syntax is veridoc's own, so this
//! parser is engine-owned and crosses no plugin boundary.
//!
//! ```markdown
//! <doc-section name="scanner">
//!
//! ## What the scanner refuses
//!
//! The scanner reads its configuration from one directory <doc-claim name="rejects-missing-config"/>.
//! ```

/// A parsed `.spec.md` file.
#[derive(Debug, Clone)]
pub struct SpecFile {
    pub path: String,
    pub sections: Vec<Section>,
}

/// One section: the subject its prose is about, and the body up to the next `<doc-section>`.
#[derive(Debug, Clone)]
pub struct Section {
    /// The `name` of the opening element: the claim subject this section's prose covers. It may
    /// repeat across the project, because coverage is what the build enforces.
    pub subject: String,
    pub line: u32,
    /// The `<doc-claim>` references in the body, each with the line it sits on.
    pub references: Vec<Reference>,
    /// The `<doc-each>` elements in the body, each with the block it repeats.
    pub injections: Vec<Injection>,
    /// True when the section names no claim. Prose-only sections are legal, so this warns.
    pub no_references: bool,
    pub spans: Vec<Span>,
    /// The section body, used for the section hash and the brief.
    pub body: String,
}

/// One `<doc-claim name="…"/>`: the predicate half of a claim's name, written by the author. The
/// opening element supplies the subject half, and bind resolves the pair or stops the build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub predicate: String,
    pub line: u32,
}

/// One `<doc-each source="…">` and the one block after it that repeats. `bind` assembles the
/// section by replacing the element and its block with one copy of the block per value the source
/// names (`docs/decisions/0011-an-authored-placeholder-pulls-its-content-at-assembly.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Injection {
    /// The line the opening element sits on, which is the line every error about it names.
    pub line: u32,
    /// The `source` attribute: the collection to gather.
    pub source: String,
    /// The half-open byte range the element and its block occupy in [`Section::body`].
    pub start: usize,
    pub end: usize,
    /// The authored block itself, without the opening element: the text one copy renders.
    pub block: String,
    /// The `field` of every `<doc-value>` inside the block, in order. `None` is a bare
    /// `<doc-value/>`, which takes the collection's default field.
    pub fields: Vec<Option<String>>,
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
    /// A `<doc-section>` with no `name="…"`, so the section states no subject.
    MissingSubjectName,
    /// A `<doc-…>` element this grammar does not define.
    UnknownElement(String),
    /// An element that has to sit on a line of its own and does not.
    MisplacedElement(String),
    /// A `<doc-each>` with no `source="…"`, so the element asks for nothing.
    MissingSource,
    /// A `<doc-each>` with no block after it, so there is nothing to repeat.
    MissingBlock,
    /// A `<doc-value>` outside the block of a `<doc-each>`, where no value can land.
    ValueOutsideEach,
    /// A section that names no claim. Reported as a warning, not an error.
    NoReferences,
}

/// The elements this grammar defines. `doc-each` and `doc-value` pull their content when the
/// engine assembles the section body, which is where a value that the code decides lands.
const KNOWN_ELEMENTS: [&str; 4] = ["doc-section", "doc-claim", "doc-each", "doc-value"];

/// Parse one `.spec.md` file. `path` is the project-relative path used in messages.
pub fn parse(path: &str, text: &str) -> (SpecFile, Vec<SpecProblem>) {
    let mut sections: Vec<Section> = Vec::new();
    let mut problems: Vec<SpecProblem> = Vec::new();
    let lines: Vec<&str> = text.split('\n').collect();
    let in_fence = fence_mask(&lines);
    let openers: Vec<usize> = opener_lines(&lines, &in_fence);
    for (position, opener) in openers.iter().enumerate() {
        let line_no = (*opener + 1) as u32;
        let body_start = opener + 1;
        let body_end = openers.get(position + 1).copied().unwrap_or(lines.len());
        let body_lines = &lines[body_start..body_end];
        let body = body_lines.join("\n");
        let starts = line_starts(body_lines);

        let subject = attr(&element_attrs(lines[*opener]), "name");
        if subject.is_empty() {
            problems.push(SpecProblem {
                file: path.to_string(),
                line: line_no,
                kind: SpecProblemKind::MissingSubjectName,
            });
        }

        // `doc-each` is scanned first, because a `<doc-value>` is only legal inside the block an
        // opener repeats, and the reference pass below needs that set to tell a placeholder from
        // a stray element.
        let mut injections: Vec<Injection> = Vec::new();
        let mut value_lines: Vec<usize> = Vec::new();
        let mut cursor = 0;
        while cursor < body_lines.len() {
            if in_fence[body_start + cursor] {
                cursor += 1;
                continue;
            }
            let opener_line = body_lines[cursor].trim();
            if !opener_line.starts_with("<doc-each") || !opener_line.ends_with('>') {
                cursor += 1;
                continue;
            }
            let where_line = line_no + cursor as u32 + 1;
            let source = attr(&element_attrs(body_lines[cursor]), "source");
            if source.is_empty() {
                problems.push(SpecProblem {
                    file: path.to_string(),
                    line: where_line,
                    kind: SpecProblemKind::MissingSource,
                });
            }
            let Some((first, last)) = repeat_block(body_lines, cursor) else {
                problems.push(SpecProblem {
                    file: path.to_string(),
                    line: where_line,
                    kind: SpecProblemKind::MissingBlock,
                });
                cursor += 1;
                continue;
            };
            // The block is copied verbatim per value, so a `<doc-value>` on any of its lines is
            // live, fence markers included: an exit-code table is a fenced block, and the values
            // of a table are exactly what the code decides.
            let mut fields = Vec::new();
            for (index, line) in (first..=last).map(|index| (index, body_lines[index])) {
                value_lines.push(body_start + index);
                for (tag, attributes) in elements(line) {
                    if tag == "doc-value" {
                        fields.push(attr_opt(&attributes, "field"));
                    }
                }
            }
            injections.push(Injection {
                line: where_line,
                source,
                start: starts[cursor],
                end: starts[last] + body_lines[last].len(),
                block: body_lines[first..=last].join("\n"),
                fields,
            });
            cursor = last + 1;
        }

        let mut references: Vec<Reference> = Vec::new();
        for (offset, line) in body_lines.iter().enumerate() {
            if in_fence[body_start + offset] {
                continue;
            }
            let where_line = line_no + offset as u32 + 1;
            for (tag, attributes) in elements(line) {
                match tag.as_str() {
                    "doc-claim" => references.push(Reference {
                        predicate: attr(&attributes, "name"),
                        line: where_line,
                    }),
                    "doc-value" => {
                        if !value_lines.contains(&(body_start + offset)) {
                            problems.push(SpecProblem {
                                file: path.to_string(),
                                line: where_line,
                                kind: SpecProblemKind::ValueOutsideEach,
                            });
                        }
                    }
                    // An opener that is not alone on its line is reported by the misplaced scan
                    // below, which names the element and the rule it broke.
                    "doc-each" | "doc-section" => {}
                    other => problems.push(SpecProblem {
                        file: path.to_string(),
                        line: where_line,
                        kind: SpecProblemKind::UnknownElement(other.to_string()),
                    }),
                }
            }
        }

        let no_references = references.is_empty();
        if no_references {
            problems.push(SpecProblem {
                file: path.to_string(),
                line: line_no,
                kind: SpecProblemKind::NoReferences,
            });
        }
        sections.push(Section {
            subject,
            line: line_no,
            references,
            injections,
            no_references,
            spans: assign_spans(&body),
            body,
        });
    }

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let element = if trimmed.starts_with("<doc-section") {
            "doc-section"
        } else if trimmed.starts_with("<doc-each") {
            "doc-each"
        } else {
            continue;
        };
        if !in_fence[index] && !trimmed.ends_with('>') {
            problems.push(SpecProblem {
                file: path.to_string(),
                line: (index + 1) as u32,
                kind: SpecProblemKind::MisplacedElement(element.to_string()),
            });
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

/// Lines inside a fenced code block, fence markers included. The grammar reads no element on
/// those lines, so a document that shows this grammar in a sample neither opens a section nor
/// names a claim.
fn fence_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut in_fence = false;
    for (index, line) in lines.iter().enumerate() {
        if is_fence_line(line) {
            in_fence = !in_fence;
            mask[index] = true;
            continue;
        }
        mask[index] = in_fence;
    }
    mask
}

/// The lines that open a section: a `<doc-section …>` alone on its line, outside a fenced block.
fn opener_lines(lines: &[&str], in_fence: &[bool]) -> Vec<usize> {
    let mut openers = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if in_fence[index] {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("<doc-section") && trimmed.ends_with('>') {
            openers.push(index);
        }
    }
    openers
}

fn is_fence_line(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

/// The attributes of the single element written on a line, as `key` and `value` pairs.
fn element_attrs(line: &str) -> Vec<(String, String)> {
    elements(line)
        .into_iter()
        .next()
        .map(|(_, attributes)| attributes)
        .unwrap_or_default()
}

/// The value of one attribute, or the empty string when the element does not carry it.
fn attr(attributes: &[(String, String)], key: &str) -> String {
    attr_opt(attributes, key).unwrap_or_default()
}

/// The value of one attribute, or `None` when the element does not carry it.
fn attr_opt(attributes: &[(String, String)], key: &str) -> Option<String> {
    attributes
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
}

/// The byte offset of each line inside `lines.join("\n")`, so an element can name the exact range
/// assembly replaces.
fn line_starts(lines: &[&str]) -> Vec<usize> {
    let mut starts = Vec::with_capacity(lines.len());
    let mut next = 0;
    for line in lines {
        starts.push(next);
        next += line.len() + 1;
    }
    starts
}

/// The block the opener at `opener` repeats: one blank line after the opener is skipped, then
/// either one fenced block, fence markers included, or the run of lines up to the next blank line.
/// `None` when the section ends before a block starts, or a fence opens and never closes, because
/// then there is no block to repeat and an author must be told rather than shown nothing.
fn repeat_block(body_lines: &[&str], opener: usize) -> Option<(usize, usize)> {
    let mut first = opener + 1;
    if body_lines
        .get(first)
        .is_some_and(|line| line.trim().is_empty())
    {
        first += 1;
    }
    if first >= body_lines.len() {
        return None;
    }
    if is_fence_line(body_lines[first]) {
        let mut last = first + 1;
        while last < body_lines.len() && !is_fence_line(body_lines[last]) {
            last += 1;
        }
        return if last < body_lines.len() {
            Some((first, last))
        } else {
            None
        };
    }
    let mut last = first;
    while last + 1 < body_lines.len()
        && !body_lines[last + 1].trim().is_empty()
        && !is_fence_line(body_lines[last + 1])
    {
        last += 1;
    }
    Some((first, last))
}

/// Every `<doc-…>` element on one line, as its tag name and its parsed attributes.
fn elements(line: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("<doc-") {
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        let inner = &rest[start + 1..start + end];
        let tag_end = inner
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(inner.len());
        let tag = inner[..tag_end].trim_end_matches('/').to_string();
        let attributes = parse_attrs(inner[tag_end..].trim_end_matches('/').trim());
        if KNOWN_ELEMENTS.contains(&tag.as_str()) {
            out.push((tag, attributes));
        } else {
            out.push((tag, Vec::new()));
        }
        rest = &rest[start + end + 1..];
    }
    out
}

/// Parse `name="scanner" kind="ensures"` into pairs. A quoted value may contain spaces.
fn parse_attrs(inner: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for token in tokenize(inner) {
        if let Some(eq) = token.find('=') {
            pairs.push((token[..eq].to_string(), token[eq + 1..].to_string()));
        }
    }
    pairs
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

/// Split a section body into spans. A paragraph is split into sentences at `[.!?]` followed by
/// whitespace; a list item is one span; a fenced code block is one span. An element written
/// inside a sentence stays in that sentence's text, because the annotation is part of the line
/// the author wrote and the hash covers it.
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

/// Replace every `<doc-value …>` in a repeating block with the value the caller supplies for its
/// `field`. A bare `<doc-value/>` asks for the empty field name, which is the caller's signal to
/// use the collection's own name. Assembly copies the block once per value, so the block is the
/// only place a value can land (`docs/decisions/0011-an-authored-placeholder-pulls-its-content-at-assembly.md`).
pub fn substitute(block: &str, value_of: &dyn Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(block.len());
    let mut rest = block;
    while let Some(start) = rest.find("<doc-value") {
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        out.push_str(&rest[..start]);
        let inner = rest[start + 1..start + end].trim_end_matches('/');
        out.push_str(&value_of(&attr(&parse_attrs(inner), "field")));
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out
}
