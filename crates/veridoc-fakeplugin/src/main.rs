use std::io::{BufWriter, Write};

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    // `<emitter> --out <dir>` is the conformance emitter contract: render the fixture project
    // into <dir>. `--emit-conformance <dir>` is the same mode spelled for hand use.
    let flag = ["--out", "--emit-conformance"]
        .iter()
        .find_map(|f| argv.iter().position(|a| a == f));
    if let Some(pos) = flag {
        let Some(dir) = argv.get(pos + 1) else {
            eprintln!("usage: veridoc-fakeplugin --out <dir>");
            std::process::exit(2);
        };
        match veridoc_fakeplugin::emit_conformance(std::path::Path::new(dir)) {
            Ok(()) => println!("{}", dir),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut writer: BufWriter<std::io::StdoutLock<'_>> = BufWriter::new(stdout.lock());
    veridoc_fakeplugin::serve(stdin.lock(), &mut writer);
    let _ = writer.flush();
}
