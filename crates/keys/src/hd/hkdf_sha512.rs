use crate::hd::hmac_sha512::hmac_sha512;

const HASH_LEN: usize = 64;

/// RFC 5869 HKDF-Expand over HMAC-SHA512; `prk` is already uniform, so there is no extract step.
///
/// `okm.len()` must be `<= 255 * 64`. Frozen: it derives the vault content keys.
pub(crate) fn hkdf_expand(prk: &[u8], info: &[u8], okm: &mut [u8]) {
    debug_assert!(
        okm.len() <= 255 * HASH_LEN,
        "HKDF-Expand output length exceeds 255*HashLen"
    );
    let mut previous: Vec<u8> = Vec::new();
    let mut counter: u8 = 1;
    let mut filled = 0;
    while filled < okm.len() {
        // T(i) = HMAC-SHA512(prk, T(i-1) ‖ info ‖ i).
        let block = hmac_sha512(prk, &[&previous, info, &[counter]]);
        let take = (okm.len() - filled).min(HASH_LEN);
        okm[filled..filled + take].copy_from_slice(&block[..take]);
        filled += take;
        previous = block.to_vec();
        counter = counter.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One block is `HMAC(prk, info ‖ 0x01)` truncated.
    #[test]
    fn single_block_matches_hmac_of_info_and_counter() {
        let prk = [0x0b_u8; 32];
        let info = b"fundamental/vault/content-master-v1";
        let mut okm = [0u8; 32];
        hkdf_expand(&prk, info, &mut okm);

        let expected = hmac_sha512(&prk, &[info, &[0x01]]);
        assert_eq!(okm, expected[..32]);
    }

    #[test]
    fn second_block_chains_on_the_first() {
        let prk = [0x2a_u8; 32];
        let info = b"ctx";
        let mut okm = [0u8; 96]; // forces two blocks (64 + 32)
        hkdf_expand(&prk, info, &mut okm);

        let t1 = hmac_sha512(&prk, &[info, &[0x01]]);
        let t2 = hmac_sha512(&prk, &[&t1, info, &[0x02]]);
        assert_eq!(&okm[..64], &t1[..]);
        assert_eq!(&okm[64..96], &t2[..32]);
    }

    #[test]
    fn is_deterministic() {
        let prk = [0x99_u8; 32];
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        hkdf_expand(&prk, b"info", &mut a);
        hkdf_expand(&prk, b"info", &mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn distinct_info_yields_distinct_output() {
        let prk = [0x77_u8; 32];
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        hkdf_expand(&prk, b"info-a", &mut a);
        hkdf_expand(&prk, b"info-b", &mut b);
        assert_ne!(a, b);
    }
}
