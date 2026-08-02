use std::env;
use std::path::PathBuf;

use cortex_mcp::CortexMcpState;

fn main() {
    if let Err(error) = run() {
        eprintln!("cortex-mcp: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let database = env::var_os("CORTEX_LOOM_DB").map_or_else(default_database, PathBuf::from);
    let state = CortexMcpState::open(database)?;
    cortex_mcp::serve(state).map_err(|error| error.to_string())
}

fn default_database() -> PathBuf {
    PathBuf::from(".cortex-loom").join("cortex-loom.db")
}
