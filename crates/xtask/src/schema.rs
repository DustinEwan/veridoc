//! Emits the committed JSON Schema in `docs/schema/<version>/` from the same types the engine
//! compiles against. `cargo run -p xtask -- schema` is the single entry point, so the
//! output path lives in one place. One directory per protocol revision: a published set is
//! frozen, a new revision adds a directory, and nothing edits the old one.

use schemars::schema_for;
use std::path::Path;

/// Write every protocol schema into the revision directory under `dir`. Returns the paths
/// written, so callers compare layouts, not just contents.
pub fn write_schemas(dir: &Path) -> Result<Vec<String>, String> {
    let dir = dir.join(veridoc_protocol::schema_dir(veridoc_protocol::PROTOCOL_VERSION));
    let dir: &Path = &dir;
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let mut written = Vec::new();

    emit(dir, &mut written, "request", &schema_for!(veridoc_protocol::Request))?;
    emit(dir, &mut written, "response", &schema_for!(veridoc_protocol::Response))?;
    emit(dir, &mut written, "handshake", &schema_for!(veridoc_protocol::Handshake))?;
    emit(dir, &mut written, "capability", &schema_for!(veridoc_protocol::Capability))?;
    emit(dir, &mut written, "kind", &schema_for!(veridoc_protocol::Kind))?;
    emit(dir, &mut written, "ref-token", &schema_for!(veridoc_protocol::RefToken))?;
    emit(dir, &mut written, "channel", &schema_for!(veridoc_protocol::Channel))?;
    emit(dir, &mut written, "raw-claim", &schema_for!(veridoc_protocol::RawClaim))?;
    emit(dir, &mut written, "asserts", &schema_for!(veridoc_protocol::Asserts))?;
    emit(dir, &mut written, "assert-leaf", &schema_for!(veridoc_protocol::AssertLeaf))?;
    emit(dir, &mut written, "check", &schema_for!(veridoc_protocol::Check))?;
    emit(
        dir,
        &mut written,
        "satisfies-spec",
        &schema_for!(veridoc_protocol::SatisfiesSpec),
    )?;
    emit(dir, &mut written, "artefact", &schema_for!(veridoc_protocol::Artefact))?;
    emit(dir, &mut written, "citation", &schema_for!(veridoc_protocol::Citation))?;
    emit(dir, &mut written, "selects", &schema_for!(veridoc_protocol::Selects))?;
    emit(
        dir,
        &mut written,
        "document-decl",
        &schema_for!(veridoc_protocol::DocumentDecl),
    )?;
    emit(
        dir,
        &mut written,
        "observable-decl",
        &schema_for!(veridoc_protocol::ObservableDecl),
    )?;
    emit(
        dir,
        &mut written,
        "action-decl",
        &schema_for!(veridoc_protocol::ActionDecl),
    )?;
    emit(dir, &mut written, "skipped", &schema_for!(veridoc_protocol::Skipped))?;
    emit(
        dir,
        &mut written,
        "collect-req",
        &schema_for!(veridoc_protocol::CollectReq),
    )?;
    emit(
        dir,
        &mut written,
        "collect-result",
        &schema_for!(veridoc_protocol::CollectResult),
    )?;
    emit(
        dir,
        &mut written,
        "resolve-req",
        &schema_for!(veridoc_protocol::ResolveReq),
    )?;
    emit(
        dir,
        &mut written,
        "resolve-result",
        &schema_for!(veridoc_protocol::ResolveResult),
    )?;
    emit(
        dir,
        &mut written,
        "derivable-req",
        &schema_for!(veridoc_protocol::DerivableReq),
    )?;
    emit(
        dir,
        &mut written,
        "derivable-result",
        &schema_for!(veridoc_protocol::DerivableResult),
    )?;
    emit(
        dir,
        &mut written,
        "bivalence-req",
        &schema_for!(veridoc_protocol::BivalenceReq),
    )?;
    emit(
        dir,
        &mut written,
        "bivalence-result",
        &schema_for!(veridoc_protocol::BivalenceResult),
    )?;
    emit(
        dir,
        &mut written,
        "bivalence-method",
        &schema_for!(veridoc_protocol::BivalenceMethod),
    )?;
    emit(
        dir,
        &mut written,
        "run-request",
        &schema_for!(veridoc_protocol::RunRequest),
    )?;
    emit(
        dir,
        &mut written,
        "run-response",
        &schema_for!(veridoc_protocol::RunResponse),
    )?;
    emit(
        dir,
        &mut written,
        "run-failure-kind",
        &schema_for!(veridoc_protocol::RunFailureKind),
    )?;
    emit(
        dir,
        &mut written,
        "cite-req",
        &schema_for!(veridoc_protocol::CiteReq),
    )?;
    emit(dir, &mut written, "cite-result", &schema_for!(veridoc_protocol::CiteResult))?;
    emit(
        dir,
        &mut written,
        "surface-result",
        &schema_for!(veridoc_protocol::SurfaceResult),
    )?;
    emit(dir, &mut written, "s1-verdict", &schema_for!(veridoc_protocol::S1Verdict))?;
    emit(dir, &mut written, "s2-verdict", &schema_for!(veridoc_protocol::S2Verdict))?;
    emit(dir, &mut written, "s3-verdict", &schema_for!(veridoc_protocol::S3Verdict))?;
    // The result document is the engine's output, not a plugin message, but it is published
    // the same way: one file in `docs/schema/`, generated from the type that writes it.
    emit(
        dir,
        &mut written,
        "result-document",
        &schema_for!(veridoc_engine::result::ResultDocument),
    )?;
    Ok(written)
}

fn emit<S: serde::Serialize>(
    dir: &Path,
    written: &mut Vec<String>,
    name: &str,
    schema: &S,
) -> Result<(), String> {
    let path = dir.join(format!("{name}.json"));
    let text =
        serde_json::to_string_pretty(schema).map_err(|e| format!("serialize {name}: {e}"))?;
    std::fs::write(&path, text + "\n").map_err(|e| format!("write {}: {e}", path.display()))?;
    written.push(path.display().to_string());
    Ok(())
}
