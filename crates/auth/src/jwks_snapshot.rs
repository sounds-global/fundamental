//! [`JwksSnapshot`] — one immutable view of a fetched key set.

use std::collections::{HashMap, HashSet};

use jsonwebtoken::jwk::Jwk;
use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use crate::AuthError;
use crate::claim_pins::ClaimPins;
use crate::jwks_key::JwksKey;
use crate::refused_jwk::RefusedJwk;

/// The longest `kid` or `kty` a refusal logs, in characters.
const LOGGED_FIELD_CHARS: usize = 64;
/// The longest parser message a `malformed` refusal logs, in characters.
const LOGGED_DETAIL_CHARS: usize = 160;
/// The `crv` names `jsonwebtoken` can read; any other is refused as an unsupported curve.
const READABLE_CURVES: [&str; 4] = ["P-256", "P-384", "P-521", "Ed25519"];

/// The keys of one JWKS document, each with its own validation policy, pinned to issuer and audience.
pub(crate) struct JwksSnapshot {
    keys: Keys,
}

/// A key without a `kid` exists only as a document's only key.
enum Keys {
    Unnamed(Box<JwksKey>),
    Named(HashMap<String, JwksKey>),
}

/// The document as published, each key still unparsed so one unreadable key is refused alone.
#[derive(Deserialize)]
struct Document {
    keys: Vec<Value>,
}

/// What a key publishes about itself, read before it is parsed.
struct Published {
    kid: Option<String>,
    kty: String,
}

impl JwksSnapshot {
    /// Parses a JWKS document read from `source` into its usable keys, logging each refused key.
    ///
    /// A `kid` shared by two accepted keys refuses both.
    ///
    /// # Errors
    /// [`AuthError::Jwks`] when the document does not parse, holds no key, or holds no key
    /// this verifier accepts.
    pub(crate) fn parse(body: &str, pins: &ClaimPins, source: &str) -> Result<Self, AuthError> {
        let document: Document = serde_json::from_str(body)
            .map_err(|e| AuthError::Jwks(format!("parsing JWKS document: {e}")))?;
        if document.keys.is_empty() {
            return Err(AuthError::Jwks("JWKS document has no keys".to_owned()));
        }

        let only_key = document.keys.len() == 1;
        let mut accepted = Vec::with_capacity(document.keys.len());
        let mut refused = 0_usize;
        for value in document.keys {
            let published = Published {
                kid: field(&value, "kid").map(str::to_owned),
                kty: field(&value, "kty").unwrap_or("none").to_owned(),
            };
            match read_key(value, &published, only_key, pins) {
                Ok(key) => accepted.push((published, key)),
                Err(reason) => {
                    refused += 1;
                    log_refusal(source, &published, &reason);
                }
            }
        }

        let repeated = repeated_kids(&accepted);
        let mut unnamed = None;
        let mut named = HashMap::with_capacity(accepted.len());
        for (published, key) in accepted {
            match published.kid {
                Some(ref kid) if repeated.contains(kid) => {
                    refused += 1;
                    log_refusal(source, &published, &RefusedJwk::DuplicateKid);
                }
                Some(kid) => {
                    named.insert(kid, key);
                }
                None => unnamed = Some(Box::new(key)),
            }
        }

        let keys = match unnamed {
            Some(key) => Keys::Unnamed(key),
            None if named.is_empty() => {
                return Err(AuthError::Jwks(format!(
                    "JWKS document has no key this verifier accepts ({refused} refused)"
                )));
            }
            None => Keys::Named(named),
        };
        Ok(Self { keys })
    }

    /// The key a token naming `kid` is verified with; an unnamed token gets the only key, if one.
    pub(crate) fn key_for(&self, kid: Option<&str>) -> Option<&JwksKey> {
        match (&self.keys, kid) {
            (Keys::Unnamed(key), None) => Some(key),
            (Keys::Unnamed(_), Some(_)) => None,
            (Keys::Named(keys), Some(kid)) => keys.get(kid),
            (Keys::Named(keys), None) if keys.len() == 1 => keys.values().next(),
            (Keys::Named(_), None) => None,
        }
    }

    pub(crate) fn len(&self) -> usize {
        match &self.keys {
            Keys::Unnamed(_) => 1,
            Keys::Named(keys) => keys.len(),
        }
    }

    /// This set reduced to the one key named `kid`, or `None` when it holds no such key.
    pub(crate) fn narrowed_to(self, kid: &str) -> Option<Self> {
        let Keys::Named(mut keys) = self.keys else {
            return None;
        };
        let key = keys.remove(kid)?;
        Some(Self {
            keys: Keys::Named(HashMap::from([(kid.to_owned(), key)])),
        })
    }
}

fn read_key(
    value: Value,
    published: &Published,
    only_key: bool,
    pins: &ClaimPins,
) -> Result<JwksKey, RefusedJwk> {
    if field(&value, "crv").is_some_and(|curve| !READABLE_CURVES.contains(&curve)) {
        return Err(RefusedJwk::UnsupportedCurve);
    }
    if published.kid.is_none() && !only_key {
        return Err(RefusedJwk::Unnamed);
    }
    let jwk: Jwk =
        serde_json::from_value(value).map_err(|e| RefusedJwk::Malformed(e.to_string()))?;
    JwksKey::from_jwk(&jwk, pins)
}

/// The `kid`s more than one accepted key carries; refused keys never count.
fn repeated_kids(accepted: &[(Published, JwksKey)]) -> HashSet<String> {
    let mut seen = HashSet::with_capacity(accepted.len());
    accepted
        .iter()
        .filter_map(|(published, _)| published.kid.as_deref())
        .filter(|kid| !seen.insert(*kid))
        .map(str::to_owned)
        .collect()
}

fn log_refusal(source: &str, published: &Published, reason: &RefusedJwk) {
    warn!(
        source,
        kid = capped(
            published.kid.as_deref().unwrap_or("none"),
            LOGGED_FIELD_CHARS
        ),
        kty = capped(&published.kty, LOGGED_FIELD_CHARS),
        reason = reason.as_str(),
        detail = reason
            .detail()
            .map(|detail| capped(detail, LOGGED_DETAIL_CHARS)),
        "JWKS key refused"
    );
}

fn field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}

/// `value` cut to `chars` characters.
fn capped(value: &str, chars: usize) -> &str {
    value
        .char_indices()
        .nth(chars)
        .map_or(value, |(end, _)| &value[..end])
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use fundamental_observability::testing::CapturedLogs;
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    use serde_json::json;
    use tracing::subscriber::with_default;
    use tracing_subscriber::layer::SubscriberExt;

    use super::*;
    use crate::InvalidToken;
    use crate::test_keys::{es256_key, public_jwk, rsa_key};

    const FUTURE: i64 = 4_102_444_800;
    const SOURCE: &str = "https://iam.example.com/jwks";

    #[derive(Debug, Deserialize)]
    struct Claims {
        sub: String,
    }

    struct Signer {
        encoding: EncodingKey,
        algorithm: Algorithm,
        jwk: Value,
    }

    impl Signer {
        fn rsa(kid: &str) -> Self {
            Self::new(rsa_key(), Algorithm::RS256, kid)
        }

        fn ec(kid: &str, seed: u8) -> Self {
            Self::new(es256_key(seed).encoding, Algorithm::ES256, kid)
        }

        fn new(encoding: EncodingKey, algorithm: Algorithm, kid: &str) -> Self {
            let jwk = public_jwk(&encoding, algorithm, Some(kid));
            Self {
                encoding,
                algorithm,
                jwk,
            }
        }

        fn unnamed(mut self) -> Self {
            if let Value::Object(fields) = &mut self.jwk {
                fields.remove("kid");
            }
            self
        }

        fn mint(&self, kid: Option<&str>) -> String {
            let mut header = Header::new(self.algorithm);
            header.kid = kid.map(str::to_owned);
            encode(
                &header,
                &json!({ "sub": "alice", "exp": FUTURE }),
                &self.encoding,
            )
            .expect("encode")
        }

        fn token(&self) -> String {
            self.mint(self.jwk["kid"].as_str())
        }
    }

    fn document(keys: &[&Value]) -> String {
        json!({ "keys": keys }).to_string()
    }

    fn parse(body: &str) -> Result<JwksSnapshot, AuthError> {
        JwksSnapshot::parse(body, &ClaimPins::default(), SOURCE)
    }

    fn parse_logged(body: &str) -> (Result<JwksSnapshot, AuthError>, String) {
        let captured = CapturedLogs::default();
        let subscriber = tracing_subscriber::registry().with(captured.layer());
        let parsed = with_default(subscriber, || parse(body));
        (parsed, captured.text())
    }

    fn verify(
        snapshot: &JwksSnapshot,
        token: &str,
        kid: Option<&str>,
    ) -> Result<String, AuthError> {
        snapshot
            .key_for(kid)
            .ok_or(AuthError::Invalid(InvalidToken::UnknownKey))?
            .decode::<Claims>(token)
            .map(|data| data.claims.sub)
    }

    fn refusal_message(parsed: Result<JwksSnapshot, AuthError>) -> String {
        match parsed {
            Err(AuthError::Jwks(message)) => message,
            Err(other) => panic!("expected AuthError::Jwks, got {other:?}"),
            Ok(_) => panic!("expected the document to be refused"),
        }
    }

    fn oct(kid: &str) -> Value {
        json!({ "kty": "oct", "kid": kid, "alg": "HS256", "k": "c2VjcmV0" })
    }

    fn segment(value: &Value) -> String {
        URL_SAFE_NO_PAD.encode(value.to_string())
    }

    #[test]
    fn a_mixed_rsa_and_ec_jwks_verifies_both() {
        let rsa = Signer::rsa("rsa");
        let ec = Signer::ec("ec", 7);
        let snapshot = parse(&document(&[&rsa.jwk, &ec.jwk])).expect("parse");

        assert_eq!(snapshot.len(), 2);
        assert_eq!(
            verify(&snapshot, &rsa.token(), Some("rsa")).expect("rsa"),
            "alice"
        );
        assert_eq!(
            verify(&snapshot, &ec.token(), Some("ec")).expect("ec"),
            "alice"
        );
    }

    #[test]
    fn an_rs256_token_does_not_verify_under_the_ec_key() {
        let rsa = Signer::rsa("rsa");
        let ec = Signer::ec("ec", 7);
        let snapshot = parse(&document(&[&rsa.jwk, &ec.jwk])).expect("parse");

        for (token, kid) in [(rsa.mint(Some("ec")), "ec"), (ec.mint(Some("rsa")), "rsa")] {
            assert!(
                matches!(
                    verify(&snapshot, &token, Some(kid)),
                    Err(AuthError::Invalid(InvalidToken::AlgorithmNotAllowed))
                ),
                "a token for the other family is refused under `{kid}`"
            );
        }
    }

    #[test]
    fn a_p256_key_does_not_verify_es384() {
        // Published without `alg`, so only the curve binds the key to ES256.
        let mut ec = Signer::ec("ec", 7);
        if let Value::Object(fields) = &mut ec.jwk {
            fields.remove("alg");
        }
        let snapshot = parse(&document(&[&ec.jwk])).expect("parse");
        assert_eq!(
            snapshot.key_for(Some("ec")).expect("key").algorithms(),
            [Algorithm::ES256]
        );

        let header = segment(&json!({ "alg": "ES384", "typ": "JWT", "kid": "ec" }));
        let claims = segment(&json!({ "sub": "alice", "exp": FUTURE }));
        let token = format!("{header}.{claims}.c2ln");
        assert!(matches!(
            verify(&snapshot, &token, Some("ec")),
            Err(AuthError::Invalid(InvalidToken::AlgorithmNotAllowed))
        ));
    }

    #[test]
    fn an_oct_key_in_a_jwks_is_refused() {
        let ec = Signer::ec("ec", 7);
        let (parsed, written) = parse_logged(&document(&[&oct("shared"), &ec.jwk]));
        let snapshot = parsed.expect("the EC key still loads");

        assert!(snapshot.key_for(Some("shared")).is_none());
        assert_eq!(snapshot.len(), 1);
        assert_eq!(written.matches("JWKS key refused").count(), 1, "{written}");
        for field in [
            "reason=\"symmetric\"",
            "kid=\"shared\"",
            "kty=\"oct\"",
            "source=\"https://iam.example.com/jwks\"",
        ] {
            assert!(written.contains(field), "{field} in {written}");
        }
    }

    #[test]
    fn a_jwks_of_only_oct_keys_fails_to_load() {
        let (parsed, written) = parse_logged(&document(&[&oct("a"), &oct("b")]));
        let message = refusal_message(parsed);
        assert!(
            message.contains("no key this verifier accepts (2 refused)"),
            "{message}"
        );
        assert_eq!(written.matches("JWKS key refused").count(), 2, "{written}");
    }

    #[test]
    fn a_duplicate_kid_is_refused() {
        let rsa = Signer::rsa("same");
        let ec = Signer::ec("same", 7);
        let other = Signer::ec("other", 8);

        let (parsed, written) = parse_logged(&document(&[&rsa.jwk, &ec.jwk, &other.jwk]));
        let snapshot = parsed.expect("the unambiguous key still loads");
        assert!(
            snapshot.key_for(Some("same")).is_none(),
            "neither key sharing a kid verifies anything"
        );
        assert_eq!(
            verify(&snapshot, &other.token(), Some("other")).expect("other"),
            "alice"
        );
        assert_eq!(
            written.matches("reason=\"duplicate_kid\"").count(),
            2,
            "{written}"
        );

        let message = refusal_message(parse(&document(&[&rsa.jwk, &ec.jwk])));
        assert!(message.contains("(2 refused)"), "{message}");
    }

    #[test]
    fn a_refused_key_sharing_a_kid_leaves_the_usable_one() {
        let signing = Signer::ec("k1", 7);
        let mut encryption = Signer::rsa("k1").jwk;
        encryption["use"] = json!("enc");
        let snapshot = parse(&document(&[&encryption, &signing.jwk])).expect("parse");
        assert_eq!(
            verify(&snapshot, &signing.token(), Some("k1")).expect("k1"),
            "alice"
        );
    }

    #[test]
    fn a_missing_kid_is_accepted_on_the_only_key() {
        let ec = Signer::ec("ec", 7).unnamed();
        let snapshot = parse(&document(&[&ec.jwk])).expect("parse");

        assert_eq!(
            verify(&snapshot, &ec.mint(None), None).expect("verifies"),
            "alice"
        );
        assert!(
            snapshot.key_for(Some("anything")).is_none(),
            "a token naming a kid never selects the unnamed key"
        );
    }

    #[test]
    fn a_missing_kid_beside_other_keys_is_refused() {
        let first = Signer::ec("first", 7).unnamed();
        let second = Signer::ec("second", 8).unnamed();
        let named = Signer::rsa("rsa");

        let (parsed, written) = parse_logged(&document(&[&first.jwk, &named.jwk]));
        let snapshot = parsed.expect("parse");
        assert_eq!(snapshot.len(), 1, "only the named key is held");
        assert!(snapshot.key_for(Some("rsa")).is_some());
        assert!(written.contains("reason=\"unnamed\""), "{written}");

        let message = refusal_message(parse(&document(&[&first.jwk, &second.jwk])));
        assert!(message.contains("(2 refused)"), "{message}");
    }

    #[test]
    fn a_token_naming_no_kid_uses_the_only_key_and_nothing_else() {
        let ec = Signer::ec("ec", 7);
        let rsa = Signer::rsa("rsa");
        let one = parse(&document(&[&ec.jwk])).expect("parse");
        assert_eq!(
            verify(&one, &ec.mint(None), None).expect("verifies"),
            "alice"
        );
        let two = parse(&document(&[&ec.jwk, &rsa.jwk])).expect("parse");
        assert!(two.key_for(None).is_none());
    }

    #[test]
    fn a_key_this_crate_cannot_read_is_refused_alone() {
        let ec = Signer::ec("ec", 7);
        let x25519 = json!({ "kty": "OKP", "crv": "X25519", "kid": "enc", "x": "AQ" });
        let no_modulus = json!({ "kty": "RSA", "kid": "broken", "e": "AQAB" });
        let (parsed, written) = parse_logged(&document(&[&x25519, &no_modulus, &ec.jwk]));
        let snapshot = parsed.expect("the EC key still loads");
        assert_eq!(snapshot.len(), 1);
        assert!(snapshot.key_for(Some("ec")).is_some());
        assert!(
            written.contains("reason=\"unsupported_curve\""),
            "{written}"
        );
        assert!(written.contains("reason=\"malformed\""), "{written}");
        assert!(
            written.contains("detail="),
            "the parser's reason is logged: {written}"
        );
    }

    #[test]
    fn narrowing_keeps_only_the_named_key() {
        let ec = Signer::ec("ec", 7);
        let rsa = Signer::rsa("rsa");
        let narrowed = parse(&document(&[&ec.jwk, &rsa.jwk]))
            .expect("parse")
            .narrowed_to("rsa")
            .expect("rsa is held");
        assert_eq!(narrowed.len(), 1);
        assert!(narrowed.key_for(Some("ec")).is_none());
        assert!(
            parse(&document(&[&ec.jwk]))
                .expect("parse")
                .narrowed_to("rsa")
                .is_none()
        );
    }

    #[test]
    fn a_long_kid_is_logged_capped() {
        let long = "k".repeat(500);
        let ec = Signer::ec("ec", 7);
        let (_, written) = parse_logged(&document(&[&oct(&long), &ec.jwk]));
        assert!(
            written.contains(&"k".repeat(LOGGED_FIELD_CHARS)),
            "{written}"
        );
        assert!(
            !written.contains(&"k".repeat(LOGGED_FIELD_CHARS + 1)),
            "{written}"
        );
    }

    #[test]
    fn parse_rejects_an_empty_or_malformed_jwks() {
        assert!(refusal_message(parse(r#"{"keys":[]}"#)).contains("no keys"));
        assert!(refusal_message(parse("not json")).contains("parsing JWKS document"));
    }
}
