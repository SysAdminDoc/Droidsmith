use std::path::PathBuf;

fn main() {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../src/lib/bindings.ts"));
    if let Err(error) = droidsmith_lib::export_typescript_bindings(&path) {
        eprintln!("failed to generate IPC bindings: {error}");
        std::process::exit(1);
    }
    println!("generated {}", path.display());
}
