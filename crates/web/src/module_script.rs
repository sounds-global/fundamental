/// The module script a page runs and the modules it preloads, each with its `integrity`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleScript {
    src: String,
    integrity: String,
    preload: Vec<(String, String)>,
}

impl ModuleScript {
    /// The module at `src` with its `integrity` (`sha256-<base64>`), preloading each of `preload`.
    #[must_use]
    pub fn new(
        src: impl Into<String>,
        integrity: impl Into<String>,
        preload: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        Self {
            src: src.into(),
            integrity: integrity.into(),
            preload: preload.into_iter().collect(),
        }
    }

    /// The entry module's URL.
    #[must_use]
    pub fn src(&self) -> &str {
        &self.src
    }

    /// The entry module's `integrity` (`sha256-<base64>`).
    #[must_use]
    pub fn integrity(&self) -> &str {
        &self.integrity
    }

    /// Each module preloaded beside it: its URL and its `integrity`.
    pub fn preload(&self) -> impl Iterator<Item = (&str, &str)> {
        self.preload
            .iter()
            .map(|(href, integrity)| (href.as_str(), integrity.as_str()))
    }
}
