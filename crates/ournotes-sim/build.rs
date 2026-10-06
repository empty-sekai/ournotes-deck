//! The SHA-256 of the crate's sources as the environment variable `OURNOTES_SIM_SOURCE_SHA256` of the crate
//! (`ournotes_sim::SOURCE_SHA256`).

use std::path::Path;

use sha2::{Digest, Sha256};

/// The files under `dir` (relative to `root`), as `/`-separated paths relative to `root`.
fn files(root: &Path, dir: &str, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(root.join(dir)).expect("source directory") {
        let entry = entry.expect("source directory entry");
        let name = entry.file_name().into_string().expect("UTF-8 source file name");
        let path = format!("{dir}/{name}");
        if entry.file_type().expect("source file type").is_dir() { files(root, &path, out) } else { out.push(path) }
    }
}

fn main() {
    let root = std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let root = Path::new(&root);
    let mut paths = vec!["Cargo.toml".to_string(), "build.rs".to_string()];
    files(root, "src", &mut paths);
    paths.sort();
    // each file as its path and its contents, both prefixed with their length
    let mut hash = Sha256::new();
    for path in &paths {
        let bytes = std::fs::read(root.join(path)).expect("source file");
        hash.update((path.len() as u64).to_le_bytes());
        hash.update(path.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    let hex: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    println!("cargo:rustc-env=OURNOTES_SIM_SOURCE_SHA256={hex}");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
}
