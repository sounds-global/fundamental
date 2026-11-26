/// One sequence rewrite a profile applies after the UTS #39 skeleton, repeated to a fixed point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fold {
    /// What is written.
    pub from: &'static str,
    /// What a reader takes it for.
    pub to: &'static str,
}

impl Fold {
    /// A fold of `from` to `to`.
    #[must_use]
    pub const fn new(from: &'static str, to: &'static str) -> Self {
        Self { from, to }
    }
}

#[cfg(test)]
mod tests {
    use super::Fold;

    #[test]
    fn a_fold_carries_both_halves() {
        let fold = Fold::new("rn", "m");
        assert_eq!(fold.from, "rn");
        assert_eq!(fold.to, "m");
    }
}
