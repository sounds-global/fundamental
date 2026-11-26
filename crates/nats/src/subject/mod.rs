//! NATS subjects: validated hierarchical names and wildcard patterns.

#[allow(clippy::module_inception)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
mod subject;
mod subject_builder;
mod subject_pattern;
mod subject_prefix;
mod validation;

pub use subject::Subject;
pub use subject_builder::SubjectBuilder;
pub use subject_pattern::SubjectPattern;
pub use subject_prefix::SubjectPrefix;
