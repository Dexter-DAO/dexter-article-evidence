//! Strict SEC1 compressed-point validation for stored WebAuthn P-256 keys.

/// Prefix-only checks admit malformed x-coordinates. Parsing the compressed
/// SEC1 encoding proves that the 33 bytes identify an actual P-256 curve point.
pub fn is_valid_compressed_p256(public_key: &[u8; 33]) -> bool {
    p256::PublicKey::from_sec1_bytes(public_key).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_prefix_only_garbage() {
        let mut malformed = [0u8; 33];
        malformed[0] = 0x02;
        malformed[1..].fill(0xff);
        assert!(!is_valid_compressed_p256(&malformed));
    }

    #[test]
    fn accepts_generator_encoding() {
        let mut generator = [0u8; 33];
        generator.copy_from_slice(&[
            0x03, 0x6b, 0x17, 0xd1, 0xf2, 0xe1, 0x2c, 0x42, 0x47, 0xf8, 0xbc, 0xe6, 0xe5, 0x63,
            0xa4, 0x40, 0xf2, 0x77, 0x03, 0x7d, 0x81, 0x2d, 0xeb, 0x33, 0xa0, 0xf4, 0xa1, 0x39,
            0x45, 0xd8, 0x98, 0xc2, 0x96,
        ]);
        assert!(is_valid_compressed_p256(&generator));
    }
}
