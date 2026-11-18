//! `veridoc init`: the three files a project needs before its first run. Refuses to
//! overwrite an existing config, which is a usage error, not a reset.

use std::path::Path;

use veridoc_engine::config::{Config, CONFIG_DIR, CONFIG_FILE_NAME};

pub fn run(project: &Path) -> Result<i32, String> {
    let dir = project.join(CONFIG_DIR);
    let config_path = dir.join(CONFIG_FILE_NAME);
    if config_path.exists() {
        return Err(format!("{} already exists", config_path.display()));
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(
        &config_path,
        serde_json::to_string_pretty(&Config::default()).unwrap() + "\n",
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join(".gitignore"),
        "result.json\nlast-result-hash\nout/\nengine.log\nverdicts/\n",
    )
    .map_err(|e| e.to_string())?;
    let index = project.join("index.doc.ts");
    if !index.exists() {
        std::fs::write(
            &index,
            "import { tree } from \"@veridoc/tree\";\n\nexport default tree({ concepts: [], documents: [] });\n",
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(0)
}
