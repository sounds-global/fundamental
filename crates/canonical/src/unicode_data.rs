/// The Unicode data a registry-profile check ran against, recorded wherever acceptance is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnicodeData {
    /// The Unicode Standard version the tables implement.
    pub unicode: &'static str,
    /// Where the tables come from, precise enough to reproduce the check.
    pub source: &'static str,
}

impl UnicodeData {
    /// The data behind [`Profile::Registry`](crate::Profile): ICU4X 2.3.0 (ICU 78, Unicode 17.0).
    pub const REGISTRY: Self = Self {
        unicode: "17.0",
        source: "icu4x 2.3.0 compiled data (ICU 78)",
    };
}
