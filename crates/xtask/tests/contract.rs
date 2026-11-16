//! The `VeridocPlugin` block in `docs/spec.md` is the contract. This test fails, naming the
//! offending method, capability, or version literal, when the spec text drifts from the Rust
//! wire vocabulary in `veridoc-protocol`.

#[test]
fn contract_spec_matches_rust_vocabulary() {
    let spec = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec.md");
    xtask::contract::check_spec(&spec).expect("docs/spec.md contract");
}
