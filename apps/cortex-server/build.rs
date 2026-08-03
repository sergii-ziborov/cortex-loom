use std::env;
use std::fs;
use std::path::PathBuf;

/// The UI bundle is embedded from `ui/dist`. A fresh clone without a UI build
/// must still compile, so an empty directory is created when it is missing;
/// the server then falls back to serving from disk at runtime.
fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets manifest dir"));
    let dist = manifest.join("../../ui/dist");
    let _ = fs::create_dir_all(&dist);
    println!("cargo:rerun-if-changed={}", dist.display());
}
