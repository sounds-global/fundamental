//! [`is_false`]: the serde skip predicate for a flag that defaults to unset.

/// `true` when the flag is unset, so serialisation leaves it out and the schema hash is
/// unchanged for schemas that never set it.
#[allow(clippy::trivially_copy_pass_by_ref)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
pub(super) fn is_false(value: &bool) -> bool {
    !*value
}
