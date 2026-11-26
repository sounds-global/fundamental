/// The NKey prefix byte of a user key: `20 << 3`, which base32-encodes to a leading `U`.
const USER_PREFIX: u8 = 20 << 3;

/// Encodes a raw Ed25519 public key as the NATS user public key a server's configuration names
/// (`{ nkey: "U…" }`): prefix byte and key, then CRC-16/XMODEM little-endian, unpadded base32.
#[must_use]
pub fn nkey_user_public_key(key: &[u8; 32]) -> String {
    let mut raw = Vec::with_capacity(1 + key.len() + 2);
    raw.push(USER_PREFIX);
    raw.extend_from_slice(key);
    let crc = crc16_xmodem(&raw);
    raw.extend_from_slice(&crc.to_le_bytes());
    data_encoding::BASE32_NOPAD.encode(&raw)
}

/// CRC-16/XMODEM: polynomial `0x1021`, initial value zero, unreflected.
fn crc16_xmodem(data: &[u8]) -> u16 {
    data.iter().fold(0_u16, |crc, byte| {
        (0..8).fold(crc ^ (u16::from(*byte) << 8), |crc, _| {
            if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x1021
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{crc16_xmodem, nkey_user_public_key};

    /// The user key the NATS documentation configures.
    const DOCUMENTED: &str = "UAPZQH4MNJCOVEJFERB3NFSIROQ5RE7CGBEPKAZSB6QB7IQHBKXHZPVP";

    #[test]
    fn a_documented_user_key_is_reproduced_from_its_raw_bytes() {
        let (_, raw) = nkeys::from_public_key(DOCUMENTED).expect("nkeys decodes it");
        assert_eq!(nkey_user_public_key(&raw), DOCUMENTED);
    }

    #[test]
    fn every_generated_user_key_is_reproduced_from_its_raw_bytes() {
        for _ in 0..16 {
            let generated = nkeys::KeyPair::new_user().public_key();
            let (_, raw) = nkeys::from_public_key(&generated).expect("nkeys decodes it");
            assert_eq!(nkey_user_public_key(&raw), generated);
        }
    }

    #[test]
    fn a_recorded_transit_key_encodes_to_a_user_key_the_reference_accepts() {
        // Recorded from OpenBao 2.6.2.
        let raw = fundamental_secrets::transit::ed25519_public_key_bytes(
            "mw0hx09eOCQl5Pb6dK4fj2UeqyIsUj66XUhreBeTAPA=",
        )
        .expect("recorded key");
        let encoded = nkey_user_public_key(&raw);
        let key = nkeys::KeyPair::from_public_key(&encoded).expect("the CRC and the point check");
        assert_eq!(key.key_pair_type(), nkeys::KeyPairType::User);
        assert_eq!(nkeys::from_public_key(&encoded).expect("decodes").1, raw);
    }

    #[test]
    fn the_checksum_is_crc16_xmodem() {
        // The standard check value for CRC-16/XMODEM.
        assert_eq!(crc16_xmodem(b"123456789"), 0x31c3);
    }
}
