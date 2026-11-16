//! The stage boundary `docs/spec.md:501` makes mechanically checkable: no engine source
//! file knows a programming language. The only `.ts` spellings allowed are the two file
//! names the engine treats as opaque labels — the index it never parses and the extension
//! it names in one error message — and no engine file mentions a TypeScript tool.

use std::path::{Path, PathBuf};
const ALLOWED_TS_LABELS: &[&str] = &["index.doc.ts", ".obs.ts"];

fn engine_sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("engine src is readable") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[test]
fn no_engine_source_mentions_a_typescript_tool() {
    for file in engine_sources() {
        let text = std::fs::read_to_string(&file).expect("source is readable");
        let lower = text.to_lowercase();
        for banned in ["ts-morph", "typescript", "tree-sitter", "swc", "babel"] {
            assert!(
                !lower.contains(banned),
                "{} mentions {banned}",
                file.display()
            );
        }
    }
}

#[test]
fn the_only_ts_spellings_are_opaque_file_labels() {
    for file in engine_sources() {
        let text = std::fs::read_to_string(&file).expect("source is readable");
        let mut rest = text.replace(ALLOWED_TS_LABELS[0], "");
        rest = rest.replace(ALLOWED_TS_LABELS[1], "");
        assert!(
            !rest.contains(".ts"),
            "{} mentions `.ts` outside the allowed labels",
            file.display()
        );
    }
}
