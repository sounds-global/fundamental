use super::sourced::Sourced;

/// The loader `loader`, declared in `file` (relative to the pages directory).
pub fn at<L>(file: &'static str, loader: L) -> Sourced<L> {
    Sourced { file, loader }
}
