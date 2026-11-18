//! The committed schema must be regenerable from the Rust types, because the TypeScript
//! plugin is written against these files and a stale file means a silent protocol drift.

use veridoc_protocol::{schema_dir, PROTOCOL_VERSION};
use xtask::schema::write_schemas;

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_contract_type_has_a_schema() {
    let dir = std::env::temp_dir().join(format!("veridoc-xtask-schema-{}", std::process::id()));
    let files = write_schemas(&dir).expect("schemas write");
    let set = dir.join(schema_dir(PROTOCOL_VERSION));
    assert!(
        files.len() >= 30,
        "expected every contract type, got {}",
        files.len()
    );
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).expect("schema is json");
        assert_eq!(
            value["$schema"].as_str(),
            Some("https://json-schema.org/draft/2020-12/schema"),
            "{f}"
        );
    }

    // `RefToken` is internally tagged on `kind`, so the wire shape matches the builder call.
    let ref_token: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(set.join("ref-token.json")).unwrap())
            .unwrap();
    let tags: Vec<&str> = ref_token["oneOf"]
        .as_array()
        .expect("ref-token is a tagged union")
        .iter()
        .filter_map(|v| v["properties"]["kind"]["const"].as_str())
        .collect();
    assert_eq!(
        tags,
        ["symbol", "concept"],
        "RefToken variants changed shape"
    );

    let capability: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(set.join("capability.json")).unwrap())
            .unwrap();
    assert_eq!(
        capability["enum"]
            .as_array()
            .expect("capability enum")
            .len(),
        7,
        "Capability must carry the seven wire names"
    );

    // `static-analysis` keeps the spec's kebab spelling, not serde's camelCase default.
    let method: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(set.join("bivalence-method.json")).unwrap())
            .unwrap();
    let spellings: Vec<&str> = method["enum"]
        .as_array()
        .expect("bivalence-method enum")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        spellings,
        ["structural", "static-analysis", "witnesses"],
        "BivalenceMethod spellings changed"
    );

    // The result document is generated from the type that writes it.
    assert!(
        set.join("result-document.json").exists(),
        "result-document.json missing"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn committed_schema_matches_generated_schema() {
    let dir =
        std::env::temp_dir().join(format!("veridoc-xtask-schema-fresh-{}", std::process::id()));
    let files = write_schemas(&dir).expect("schemas write");
    let committed = repo_root().join("docs/schema");
    let mut stale = Vec::new();
    // Compare by path under `docs/schema/`, so the versioned directory layout is what is
    // gated, not just the file contents.
    let mut fresh_names: Vec<String> = Vec::new();
    for path in &files {
        let path = std::path::Path::new(path);
        let relative = path.strip_prefix(&dir).unwrap();
        let fresh = std::fs::read_to_string(path).unwrap();
        fresh_names.push(relative.display().to_string());
        match std::fs::read_to_string(committed.join(relative)) {
            Ok(have) if have == fresh => {}
            Ok(_) => stale.push(relative.display().to_string()),
            Err(_) => stale.push(format!("{} (missing)", relative.display())),
        }
    }
    // A committed file the generator no longer writes is drift in the other direction:
    // someone edited docs/schema by hand, or a revision directory was added by hand.
    let committed_set = committed.join(schema_dir(PROTOCOL_VERSION));
    for entry in std::fs::read_dir(&committed_set).expect("committed schema dir") {
        let name = entry
            .unwrap()
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        if !fresh_names
            .iter()
            .any(|f| f.ends_with(&format!("{}{name}", std::path::MAIN_SEPARATOR)))
        {
            stale.push(format!("{name} (not generated)"));
        }
    }
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        stale.is_empty(),
        "docs/schema is stale, run `cargo run -p xtask -- schema`: {stale:?}"
    );
}

/// The design fixes the wire format: JSON field names are camelCase. serde's container-level
/// `rename_all` renames variant names but leaves struct-variant fields alone, so a field like
/// `deadline_ms` reaches the wire unchanged and the generated TypeScript type disagrees with
/// the Rust parser that accepts it.
#[test]
fn every_wire_field_name_is_camel_case() {
    let dir =
        std::env::temp_dir().join(format!("veridoc-xtask-schema-camel-{}", std::process::id()));
    let files = write_schemas(&dir).expect("schemas write");
    let mut offenders = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).expect("schema is json");
        let name = std::path::Path::new(file)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        walk_properties(&value, &name, &mut offenders);
    }
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        offenders.is_empty(),
        "snake_case wire field names: {offenders:?}"
    );
}

fn walk_properties(node: &serde_json::Value, schema: &str, offenders: &mut Vec<String>) {
    match node {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if key == "properties" {
                    if let Some(fields) = value.as_object() {
                        for field in fields.keys() {
                            if field.contains('_') {
                                offenders.push(format!("{schema}: {field}"));
                            }
                        }
                    }
                }
                walk_properties(value, schema, offenders);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                walk_properties(item, schema, offenders);
            }
        }
        _ => {}
    }
}
