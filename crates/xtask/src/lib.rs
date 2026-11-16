//! Repository helpers: JSON Schema generation and the `docs/spec.md` contract check. Tooling
//! code returns `Result<T, String>` rather than defining an error enum, because its only
//! consumer is a human running `cargo run -p xtask`.

pub mod contract;
pub mod schema;

pub fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
