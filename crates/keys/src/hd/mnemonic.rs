use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::KeyError;
use crate::hd::MasterSeed;
use crate::hd::bip39_wordlist::{index_of, word};
use crate::hd::hmac_sha512::hmac_sha512;

const PBKDF2_ROUNDS: u32 = 2048;

/// A BIP-39 mnemonic, the backup of a [`MasterSeed`]; zeroized on drop and `Debug` redacted.
///
/// Frozen: locked by the BIP-39 vectors. NFKD normalisation is skipped, as it is the identity
/// for ASCII; a non-ASCII passphrase would need it first.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct Mnemonic {
    phrase: String,
    entropy: Vec<u8>,
}

impl Mnemonic {
    /// Builds a mnemonic from 16, 20, 24, 28 or 32 bytes of entropy.
    ///
    /// # Errors
    /// [`KeyError::Malformed`] if `entropy` is not a BIP-39-valid length.
    pub fn from_entropy(entropy: &[u8]) -> Result<Self, KeyError> {
        if !matches!(entropy.len(), 16 | 20 | 24 | 28 | 32) {
            return Err(KeyError::Malformed(format!(
                "BIP-39 entropy must be 16..=32 bytes in 4-byte steps, got {}",
                entropy.len()
            )));
        }
        let indices = to_indices(entropy);
        let mut words = Vec::with_capacity(indices.len());
        for idx in indices {
            words.push(word(idx).ok_or(KeyError::MnemonicInvalid)?);
        }
        Ok(Self {
            phrase: words.join(" "),
            entropy: entropy.to_vec(),
        })
    }

    /// Generates a fresh 24-word mnemonic (256 bits of entropy) from the OS CSPRNG.
    ///
    /// # Errors
    /// [`KeyError::Entropy`] if the system random source is unavailable.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn generate() -> Result<Self, KeyError> {
        let mut entropy = [0u8; 32];
        getrandom::getrandom(&mut entropy).map_err(|_| KeyError::Entropy)?;
        let mnemonic = Self::from_entropy(&entropy);
        entropy.zeroize();
        mnemonic
    }

    /// Parses and validates a recovery phrase, canonicalised to single spaces.
    ///
    /// # Errors
    /// [`KeyError::MnemonicInvalid`] if the word count is wrong, a word is not in the
    /// list, or the checksum fails.
    pub fn parse(phrase: &str) -> Result<Self, KeyError> {
        let words: Vec<&str> = phrase.split_whitespace().collect();
        if !matches!(words.len(), 12 | 15 | 18 | 21 | 24) {
            return Err(KeyError::MnemonicInvalid);
        }
        let mut indices = Vec::with_capacity(words.len());
        for w in &words {
            indices.push(index_of(w).ok_or(KeyError::MnemonicInvalid)?);
        }
        let entropy = entropy_from_indices(&indices)?;
        Ok(Self {
            phrase: words.join(" "),
            entropy,
        })
    }

    /// The recovery phrase, for the backup screen; never log it.
    #[must_use]
    pub fn phrase(&self) -> &str {
        &self.phrase
    }

    /// The number of words in the phrase.
    #[must_use]
    pub fn word_count(&self) -> usize {
        self.entropy.len() * 3 / 4
    }

    /// Stretches the phrase, with an optional passphrase, into the [`MasterSeed`].
    #[must_use]
    pub fn to_seed(&self, passphrase: Option<&str>) -> MasterSeed {
        let mut salt = String::from("mnemonic");
        salt.push_str(passphrase.unwrap_or(""));
        let seed = pbkdf2_hmac_sha512_64(self.phrase.as_bytes(), salt.as_bytes());
        salt.zeroize();
        MasterSeed::from_bytes(seed)
    }
}

impl core::fmt::Debug for Mnemonic {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

fn bit_at(bytes: &[u8], i: usize) -> bool {
    (bytes[i / 8] >> (7 - (i % 8))) & 1 == 1
}

fn to_indices(entropy: &[u8]) -> Vec<u16> {
    let hash = Sha256::digest(entropy);
    let ent_bits = entropy.len() * 8;
    let cs_bits = ent_bits / 32;
    let n_words = (ent_bits + cs_bits) / 11;
    (0..n_words)
        .map(|w| {
            (0..11).fold(0u16, |idx, b| {
                let pos = w * 11 + b;
                let bit = if pos < ent_bits {
                    bit_at(entropy, pos)
                } else {
                    bit_at(&hash, pos - ent_bits)
                };
                (idx << 1) | u16::from(bit)
            })
        })
        .collect()
}

fn entropy_from_indices(indices: &[u16]) -> Result<Vec<u8>, KeyError> {
    let total_bits = indices.len() * 11;
    let cs_bits = total_bits / 33;
    let ent_bits = total_bits - cs_bits;
    let mut entropy = vec![0u8; ent_bits / 8];
    for (w, &idx) in indices.iter().enumerate() {
        for b in 0..11 {
            let pos = w * 11 + b;
            if pos < ent_bits && (idx >> (10 - b)) & 1 == 1 {
                entropy[pos / 8] |= 1 << (7 - (pos % 8));
            }
        }
    }
    let hash = Sha256::digest(&entropy);
    for i in 0..cs_bits {
        let got = {
            let w = (ent_bits + i) / 11;
            let b = (ent_bits + i) % 11;
            (indices[w] >> (10 - b)) & 1 == 1
        };
        if got != bit_at(&hash, i) {
            return Err(KeyError::MnemonicInvalid);
        }
    }
    Ok(entropy)
}

/// PBKDF2-HMAC-SHA512 producing one 64-byte block, the BIP-39 seed case.
fn pbkdf2_hmac_sha512_64(password: &[u8], salt: &[u8]) -> [u8; 64] {
    // U1 = PRF(password, salt ‖ INT_32_BE(1)); T = U1 ⊕ U2 ⊕ … ⊕ U_c.
    let mut u = hmac_sha512(password, &[salt, &1u32.to_be_bytes()]);
    let mut t = u;
    for _ in 1..PBKDF2_ROUNDS {
        u = hmac_sha512(password, &[&u]);
        for (ti, ui) in t.iter_mut().zip(u.iter()) {
            *ti ^= ui;
        }
    }
    u.zeroize();
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    fn hex_lower(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    /// Official BIP-39 (Trezor) English vectors with the passphrase "TREZOR".
    #[test]
    fn matches_official_bip39_vectors() {
        let m = Mnemonic::from_entropy(&[0u8; 32]).expect("entropy");
        assert!(m.phrase().starts_with("abandon abandon abandon"));
        assert!(m.phrase().ends_with(" art"));
        assert_eq!(m.word_count(), 24);
        assert_eq!(
            hex_lower(m.to_seed(Some("TREZOR")).expose()),
            "bda85446c68413707090a52022edd26a1c9462295029f2e60cd7c4f2bbd3097170af7a4d73245cafa9c3cca8d561a7c3de6f5d4a10be8ed2a5e608d68f92fcc8"
        );

        let m = Mnemonic::from_entropy(&[0x7fu8; 32]).expect("entropy");
        assert!(m.phrase().starts_with("legal winner thank year"));
        assert_eq!(
            hex_lower(m.to_seed(Some("TREZOR")).expose()),
            "bc09fca1804f7e69da93c2f2028eb238c227f2e9dda30cd63699232578480a4021b146ad717fbb7e451ce9eb835f43620bf5c514db0f8add49f5d121449d3e87"
        );

        let m = Mnemonic::from_entropy(&[0xffu8; 32]).expect("entropy");
        assert_eq!(
            hex_lower(m.to_seed(Some("TREZOR")).expose()),
            "dd48c104698c30cfe2b6142103248622fb7bb0ff692eebb00089b32d22484e1613912f0a5b694407be899ffd31ed3992c456cdf60f5d4564b8ba3f05a69890ad"
        );
    }

    #[test]
    fn parse_round_trips_a_generated_phrase() {
        let m = Mnemonic::from_entropy(&[0x24u8; 32]).expect("entropy");
        let parsed = Mnemonic::parse(m.phrase()).expect("valid phrase parses");
        assert_eq!(parsed, m);
        assert_eq!(parsed.to_seed(None).expose(), m.to_seed(None).expose());
    }

    #[test]
    fn parse_normalises_whitespace() {
        let m = Mnemonic::from_entropy(&[0u8; 32]).expect("entropy");
        let spaced = format!("  {}  ", m.phrase().replace(' ', "   "));
        let parsed = Mnemonic::parse(&spaced).expect("odd spacing still parses");
        assert_eq!(parsed.phrase(), m.phrase());
    }

    #[test]
    fn parse_rejects_a_bad_checksum() {
        // The last word swapped for another valid word breaks the checksum.
        let m = Mnemonic::from_entropy(&[0u8; 32]).expect("entropy"); // "... art"
        let mut words: Vec<&str> = m.phrase().split(' ').collect();
        words.pop();
        words.push("zoo"); // a valid word, wrong checksum
        let err = Mnemonic::parse(&words.join(" ")).expect_err("checksum must fail");
        assert!(matches!(err, KeyError::MnemonicInvalid));
        assert_eq!(err.error_code(), "KEYS_HD_001");
        assert_eq!(err.http_status_code(), 400);
    }

    #[test]
    fn parse_rejects_unknown_words_and_wrong_length() {
        assert!(matches!(
            Mnemonic::parse("clearly not eight bip39 words here"),
            Err(KeyError::MnemonicInvalid)
        ));
        let m = Mnemonic::from_entropy(&[0u8; 32]).expect("entropy");
        let mut words: Vec<&str> = m.phrase().split(' ').collect();
        words[0] = "notaword";
        assert!(matches!(
            Mnemonic::parse(&words.join(" ")),
            Err(KeyError::MnemonicInvalid)
        ));
    }

    #[test]
    fn from_entropy_rejects_a_bad_length() {
        let err = Mnemonic::from_entropy(&[0u8; 30]).expect_err("bad length");
        assert!(matches!(err, KeyError::Malformed(_)));
    }

    #[test]
    fn debug_redacts_the_phrase() {
        let m = Mnemonic::from_entropy(&[0u8; 32]).expect("entropy");
        assert_eq!(format!("{m:?}"), "[REDACTED]");
    }

    #[test]
    fn passphrase_changes_the_seed() {
        let m = Mnemonic::from_entropy(&[0x24u8; 32]).expect("entropy");
        assert_ne!(m.to_seed(None).expose(), m.to_seed(Some("extra")).expose());
    }
}
