//! Throwaway key pairs for this crate's unit tests. None of them protects anything.

use jsonwebtoken::jwk::Jwk;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey};
use p256::ecdsa::SigningKey;
use p256::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
use serde_json::Value;

/// The private half of an RSA-2048 key pair.
pub(crate) const RSA_PRIVATE_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQCERFftNljFBTtS
XoGsnp/KwycvUmRY8ojg8LRLFtsNNI+CVDwdMkF6o0pr+AYXJlBOcux023yWfuYm
d/RVbjZITaBAatTlrPZC99exAAysRCvG7dxhRs6Pzpz2kCyiTYhgfMMrmBdAyfV8
w5Q+L/XDRx5nW4g5UGWR+1O5IhTz4leJn1xXfnm1dtWb3Do85G4GJuAVlka0ZvyO
JFSlMwVDrn4G/iX1nnck7RvnGJuHkXqUnwrcUNb2FJmLKby7v/ruhiS9iyDHpo5P
a1d0hL07jkg682GKq1ZAbnlZOhzBwbjm7DzIt2+hxPmO+3ll2EjW2SqgnFQEDKs5
CjOv2mkpAgMBAAECggEAAZelC1kNJooSlcJHg2GBgPpqfVpblOqOlU6wjPrgdj88
vw/ujVdujGBs4/HfeOXgL6GYkSuXS2SJtsrY6TJjeulcVYozt/QmHtZHBX1/qY9E
i6ePcKhekkKlhx5jOyz9yFOYMe0p0oDb8L2WCk2fgtNXXV0vvajnICPLdT4gzHLX
tq/0a4MnIKoLE5g46lfhtoMMQrIcujFJKAtlWX+lx5kUkKyEg6Y4RVsgRWaqqpGT
6KG6LYiZIzTvukC1M2Ax3rqfbAr3ImSjYYZ5wSIAJtX6cvY8vH0wAygo4o9o2rkt
xNw51pbhedd2A6d0H/4meHIlhTrcJgu58T3+6YFs5QKBgQC4dhW2f3zFV6L0dPFk
XtfVynLFarIqxjMhq6roX2JEJG0WjnEPreXT24XIPr3z1HZcpUYE1fqX55ttkj32
KR3yA/VYttm1IWdhpJ8rQnCTPY7SSbZe5KZtzXMpND1aw0zG1uihwvy4yLiqpBoo
L2Q/ib7kwQMXeAozk4lLa1IQ1wKBgQC3kD0KRnZNklW/holU32myYwp8FtDUICpV
GXQaiu03eIccRo6l9l4jliwSRRbSILHWKThQkPRdnoKaOlIoiHJCXuMpQUt7boNi
eYalq9TNP9JEpMRonkG+e1yjTCbNoaZtq27Gj2Ri5wU3hNjz8o0xqljnqFEVUVKc
+ELiltYV/wKBgQCxoJyWCbwXuKVl6/rL4XzMFdBhhLYe9I8mXE4OyFMzNTSLJQ2A
wL9xPJJeWvTGCREVc92yKG/Quan1YH8kPUHl3KP1EtcgUhNgdosDuE35q6yNmYfx
ZxNQ5yd/RRvT+/9B9o87vi1or3kMcUvy6HgLq1RUqfyVK/X+5pKuOqFjLwKBgHcT
QRV2Se3eHHPGgTWmS7llV/rRDhxc1Tko7tD9CA6wx+yGtKhiHgU1uFcNrFUY0mPb
j2Vr8qj8VzpW1M7oXD5DsVYkviNOuDncHYVA5yVR3kObIrG+32UCP0DK+GzyDLUd
vRlfET67H/441kfBMuhh2ZRE8nxCqbWbsj+smynfAoGAAgMyHWR7SZ0DsmJy1hFS
sWb3szfeT+iBhKYZRbnzr3h7jKR+4hMXpWLpu2QvIwVhFNnn3adIwxXgdAQkZu24
z/8exsJmF4OnDokW0hJaCEsWv5g9hDsTrcASB4aOJS1jGK9AKIGtq+bRbY6uLfcf
S2llDmlR/8Eh1fLTg4TnpNI=
-----END PRIVATE KEY-----
";

/// The public half of [`RSA_PRIVATE_PEM`].
pub(crate) const RSA_PUBLIC_PEM: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAhERX7TZYxQU7Ul6BrJ6f
ysMnL1JkWPKI4PC0SxbbDTSPglQ8HTJBeqNKa/gGFyZQTnLsdNt8ln7mJnf0VW42
SE2gQGrU5az2QvfXsQAMrEQrxu3cYUbOj86c9pAsok2IYHzDK5gXQMn1fMOUPi/1
w0ceZ1uIOVBlkftTuSIU8+JXiZ9cV355tXbVm9w6PORuBibgFZZGtGb8jiRUpTMF
Q65+Bv4l9Z53JO0b5xibh5F6lJ8K3FDW9hSZiym8u7/67oYkvYsgx6aOT2tXdIS9
O45IOvNhiqtWQG55WTocwcG45uw8yLdvocT5jvt5ZdhI1tkqoJxUBAyrOQozr9pp
KQIDAQAB
-----END PUBLIC KEY-----
";

/// A P-256 key pair, derived from a fixed scalar so every run signs with the same key.
pub(crate) struct Es256Key {
    pub(crate) encoding: EncodingKey,
    pub(crate) decoding: DecodingKey,
}

/// The P-256 key pair whose private scalar is `seed` repeated 32 times.
pub(crate) fn es256_key(seed: u8) -> Es256Key {
    let key = SigningKey::from_slice(&[seed; 32]).expect("valid scalar");
    let private = key.to_pkcs8_pem(LineEnding::LF).expect("pkcs8");
    let public = key
        .verifying_key()
        .to_public_key_pem(LineEnding::LF)
        .expect("spki");
    Es256Key {
        encoding: EncodingKey::from_ec_pem(private.as_bytes()).expect("encoding key"),
        decoding: DecodingKey::from_ec_pem(public.as_bytes()).expect("decoding key"),
    }
}

/// The signing key of [`RSA_PRIVATE_PEM`].
pub(crate) fn rsa_key() -> EncodingKey {
    EncodingKey::from_rsa_pem(RSA_PRIVATE_PEM.as_bytes()).expect("rsa key")
}

/// The public JWK of `encoding`, declaring `algorithm` and, when given, `kid`.
pub(crate) fn public_jwk(encoding: &EncodingKey, algorithm: Algorithm, kid: Option<&str>) -> Value {
    let mut jwk =
        serde_json::to_value(Jwk::from_encoding_key(encoding, algorithm).expect("public jwk"))
            .expect("serialises");
    if let (Value::Object(fields), Some(kid)) = (&mut jwk, kid) {
        fields.insert("kid".to_owned(), kid.into());
    }
    jwk
}
