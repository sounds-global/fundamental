//! Generates two page registrations, as two apps would: one from a tree that only exports
//! `Data` (the plain registration) and one from a tree with a `loader.rs` (the page pipeline).

use std::path::PathBuf;
use std::process::ExitCode;

use fundamental_build::PagesBuild;

fn main() -> ExitCode {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("set by cargo"));
    for tree in ["legacy", "pipeline"] {
        let out = out.join(tree);
        std::fs::create_dir_all(&out).expect("the tree's output directory");
        if let Err(refusal) = PagesBuild::new(manifest.join(tree), out).run() {
            eprintln!("error: {refusal}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
