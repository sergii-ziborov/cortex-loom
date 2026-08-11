use std::env;
use std::path::PathBuf;

use cortex_mcp::{CortexMcpState, ServerProfile};

fn main() {
    if let Err(error) = run() {
        eprintln!("cortex-mcp: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let database = env::var_os("CORTEX_LOOM_DB").map_or_else(default_database, PathBuf::from);
    let mut http = env::var("CORTEX_MCP_HTTP").ok();
    let mut profile = match env::var("CORTEX_MCP_PROFILE") {
        Ok(value) => ServerProfile::parse(&value)?,
        Err(_) => ServerProfile::default(),
    };
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--http" => {
                http = Some(
                    arguments
                        .next()
                        .ok_or_else(|| "--http requires an address".to_owned())?,
                );
            }
            "--profile" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--profile requires a value (full|context)".to_owned())?;
                profile = ServerProfile::parse(&value)?;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let state = CortexMcpState::open(database)?;
    match http {
        Some(address) => {
            let address = address
                .parse()
                .map_err(|error| format!("invalid --http address: {error}"))?;
            cortex_mcp::http::serve_http_with(state, address, profile)
                .map_err(|error| error.to_string())
        }
        None => cortex_mcp::serve_with(state, profile).map_err(|error| error.to_string()),
    }
}

fn default_database() -> PathBuf {
    PathBuf::from(".cortex-loom").join("cortex-loom.db")
}
