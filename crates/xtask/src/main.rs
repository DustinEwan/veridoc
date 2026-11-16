fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("schema") => {
            let base = xtask::repo_root().join("docs/schema");
            for path in xtask::schema::write_schemas(&base)? {
                println!("wrote {path}");
            }
            Ok(())
        }
        Some("contract") => xtask::contract::check_spec(&xtask::repo_root().join("docs/spec.md")),
        other => Err(format!(
            "usage: cargo run -p xtask -- <schema|contract> (got {other:?})"
        )),
    }
}
