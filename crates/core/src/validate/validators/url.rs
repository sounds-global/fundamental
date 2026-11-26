//! Basic URL validation.

use crate::validate::ValidationError;

pub fn validate_url(field: &str, url: &str) -> Result<(), ValidationError> {
    if !url.is_empty() && !is_valid_url(url) {
        return Err(ValidationError::invalid_url(field));
    }
    Ok(())
}

fn is_valid_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}
