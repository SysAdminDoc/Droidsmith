//! Local MCP stdio adapter for Droidsmith's headless CLI.

fn main() {
    if let Err(error) = droidsmith_lib::mcp::run_stdio() {
        eprintln!("[droidsmith-mcp] {error}");
        std::process::exit(1);
    }
}
