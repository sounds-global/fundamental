use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Map, Value};
use tracing::instrument;

use crate::KeyError;
use crate::server::jws::{CompactJws, JwsHeader};
use crate::server::{AsyncSigner, SignatureAlgorithm};

/// Signs `payload` as a compact JWS (RFC 7515 §7.1) with `signer`.
///
/// The header carries `alg`, then `kid` and `typ` when given, in that order.
///
/// # Errors
/// Whatever the signer returns, and [`KeyError::Sign`] if the header cannot be serialised.
#[instrument(name = "keys.jws_sign_compact", skip_all, fields(alg = A::ID.jws_alg(), key_id = %signer.key_id()), err(Debug))]
pub async fn sign_compact<A: SignatureAlgorithm>(
    signer: &dyn AsyncSigner<A>,
    header: &JwsHeader<'_>,
    payload: &[u8],
) -> Result<CompactJws, KeyError> {
    let mut fields = Map::new();
    fields.insert("alg".to_owned(), Value::from(A::ID.jws_alg()));
    if let Some(kid) = header.kid {
        fields.insert("kid".to_owned(), Value::from(kid));
    }
    if let Some(typ) = header.typ {
        fields.insert("typ".to_owned(), Value::from(typ));
    }
    let header_json = serde_json::to_vec(&Value::Object(fields))
        .map_err(|error| KeyError::Sign(format!("JWS header: {error}")))?;

    let mut signing_input = URL_SAFE_NO_PAD.encode(header_json);
    signing_input.push('.');
    signing_input.push_str(&URL_SAFE_NO_PAD.encode(payload));

    let signature = signer.sign(signing_input.as_bytes()).await?;
    signing_input.push('.');
    signing_input.push_str(&URL_SAFE_NO_PAD.encode(A::signature_bytes(&signature)));
    Ok(CompactJws::new(signing_input))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::providers::SoftwareP256Signer;
    use crate::server::{Es256, P256Signature};

    #[tokio::test]
    async fn the_jws_alg_comes_from_the_type() {
        let signer = SoftwareP256Signer::generate("tenant").expect("generate");
        let jws = sign_compact(
            &signer,
            &JwsHeader::new(Some("k1"), Some("entity-statement+jwt")),
            b"{}",
        )
        .await
        .expect("signs");
        let header = jws.as_str().split('.').next().expect("header");
        let decoded: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).expect("base64")).expect("json");
        assert_eq!(decoded["alg"], "ES256");
        assert_eq!(decoded["kid"], "k1");
        assert_eq!(decoded["typ"], "entity-statement+jwt");
    }

    #[tokio::test]
    async fn a_compact_jws_through_dyn_signer_verifies() {
        let signer = SoftwareP256Signer::generate("tenant").expect("generate");
        let held: &dyn AsyncSigner<Es256> = &signer;
        let jws = sign_compact(held, &JwsHeader::default(), b"payload")
            .await
            .expect("signs");
        let parts: Vec<&str> = jws.as_str().split('.').collect();
        assert_eq!(parts.len(), 3);
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let signature =
            P256Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).expect("base64"))
                .expect("64 bytes");
        Es256::verify(&held.public_key(), signing_input.as_bytes(), &signature).expect("verifies");
        assert_eq!(
            URL_SAFE_NO_PAD.decode(parts[1]).expect("base64"),
            b"payload"
        );
    }
}
