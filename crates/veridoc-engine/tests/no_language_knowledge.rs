//! The stage boundary the specification's artifact stages make mechanically checkable: no
//! engine source file knows a programming language. The engine reads claims, referents, the
//! document tree, and the observable declarations out of the plugin's answers, so it has no
//! reason to name a file an author writes, and no reason to know a language's tooling.

use std::path::{Path, PathBuf};

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
fn no_engine_source_names_an_authored_file_extension() {
    for file in engine_sources() {
        let text = std::fs::read_to_string(&file).expect("source is readable");
        assert!(
            !text.contains(".ts"),
            "{} names a `.ts` file, which the engine never parses",
            file.display()
        );
    }
}
