use crate::generate_dek;
use base64::{Engine, engine::general_purpose::STANDARD};

/// Generates a DEK and returns it as a standard base64-encoded string.
pub fn generate_dek_base64() -> String {
    STANDARD.encode(generate_dek())
}
