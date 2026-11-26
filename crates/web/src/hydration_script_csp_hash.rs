use fundamental_server::{CspHash, ServerResult};

/// The base64 SHA-256 of [`hydration_bootstrap`](crate::hydration_bootstrap); update it with every bootstrap edit.
const HYDRATION_SCRIPT_SHA256: &str = "peL/GXVy6RM8fkxYRW9876iA5aRuep3tHCI1D7JH8t4=";

/// The `'sha256-…'` source that admits the island bootstrap under a strict Content Security Policy.
///
/// # Errors
/// [`ServerError::SecurityHeaderInvalid`](fundamental_server::ServerError::SecurityHeaderInvalid)
/// (`API_018`) if the digest above is not well-formed base64.
pub fn hydration_script_csp_hash() -> ServerResult<CspHash> {
    CspHash::sha256_base64(HYDRATION_SCRIPT_SHA256)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recorded_digest_is_a_well_formed_hash_source() {
        let hash = hydration_script_csp_hash().expect("well formed");
        assert_eq!(hash.source(), format!("'sha256-{HYDRATION_SCRIPT_SHA256}'"));
    }
}
