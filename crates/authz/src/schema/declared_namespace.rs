//! [`declared_namespace`]: a namespace named in settings, resolved against the schema.

use fundamental_core::validate::{ValidationError, ValidationErrors};

use super::{NamespaceName, SchemaIr};

/// The namespace `raw` names, when `schema` declares it.
///
/// Resolved at boot, so a missing one refuses to start. Each problem is added to `problems`
/// under `field` (`namespace_invalid` or `namespace_undeclared`).
pub fn declared_namespace(
    schema: &SchemaIr,
    raw: &str,
    field: &str,
    problems: &mut ValidationErrors,
) -> Option<NamespaceName> {
    match NamespaceName::new(raw) {
        Ok(name) if schema.namespace(&name).is_some() => Some(name),
        Ok(_) => {
            problems.add(
                ValidationError::custom(
                    "namespace_undeclared",
                    format!("the schema declares no namespace `{raw}`"),
                )
                .with_field(field),
            );
            None
        }
        Err(_) => {
            problems.add(
                ValidationError::custom("namespace_invalid", "not a namespace name")
                    .with_field(field),
            );
            None
        }
    }
}
