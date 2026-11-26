/// What a passing baseline check compared.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Checked {
    /// How many benches were compared.
    pub compared: usize,
    /// The benches that got faster than their threshold, worth `fun bench --save`.
    pub improved: Vec<String>,
}
