//! [`WasmArtifacts`] — the files a [`WasmBuild`](super::WasmBuild) produces.

use std::path::PathBuf;

/// The browser bundle a wasm build emits into its output directory.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WasmArtifacts {
    /// The processed WebAssembly module (`<name>_bg.wasm`).
    pub wasm: PathBuf,
    /// The JavaScript glue module that loads the wasm (`<name>.js`).
    pub js: PathBuf,
}
