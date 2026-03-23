//! Cryptographic utilities for the MeshCore wire protocol.
//!
//! Provides AES-128 ECB encryption/decryption, HMAC-SHA256 truncated MAC,
//! encrypt-then-MAC authenticated encryption, and ECDH key exchange
//! (Ed25519-to-X25519 conversion for wire compatibility with MeshCore C).

use aes::cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;
use hmac::{Hmac, Mac};
use sha2::{Sha256, Sha512};

use crate::constants::{CIPHER_BLOCK_SIZE, CIPHER_KEY_SIZE, CIPHER_MAC_SIZE, PUB_KEY_SIZE};

type HmacSha256 = Hmac<Sha256>;

/// Encrypt plaintext using AES-128 ECB mode.
///
/// Key is derived from the first 16 bytes of `shared_secret`. The final block
/// is zero-padded to the 16-byte AES block boundary (not PKCS#7).
///
/// Returns the number of bytes written to `dest` (always a multiple of 16).
///
/// # Panics
///
/// Panics if `dest` is too small to hold the padded ciphertext.
pub fn aes_encrypt(shared_secret: &[u8; PUB_KEY_SIZE], dest: &mut [u8], src: &[u8]) -> usize {
    let key = GenericArray::from_slice(&shared_secret[..CIPHER_KEY_SIZE]);
    let cipher = Aes128::new(key);

    let num_blocks = src.len().div_ceil(CIPHER_BLOCK_SIZE);
    let out_len = num_blocks * CIPHER_BLOCK_SIZE;

    assert!(
        dest.len() >= out_len,
        "dest too small: need {out_len}, have {}",
        dest.len()
    );

    for i in 0..num_blocks {
        let src_start = i * CIPHER_BLOCK_SIZE;
        let mut block = GenericArray::default();

        // Copy source bytes; remaining bytes stay zero (zero-pad last block)
        let copy_len = core::cmp::min(CIPHER_BLOCK_SIZE, src.len() - src_start);
        block[..copy_len].copy_from_slice(&src[src_start..src_start + copy_len]);

        cipher.encrypt_block(&mut block);

        let dest_start = i * CIPHER_BLOCK_SIZE;
        dest[dest_start..dest_start + CIPHER_BLOCK_SIZE].copy_from_slice(&block);
    }

    out_len
}

/// Decrypt ciphertext using AES-128 ECB mode.
///
/// Key is derived from the first 16 bytes of `shared_secret`.
///
/// Returns the number of bytes written to `dest`.
///
/// # Panics
///
/// Panics if `src` length is not a multiple of 16 or `dest` is too small.
pub fn aes_decrypt(shared_secret: &[u8; PUB_KEY_SIZE], dest: &mut [u8], src: &[u8]) -> usize {
    let key = GenericArray::from_slice(&shared_secret[..CIPHER_KEY_SIZE]);
    let cipher = Aes128::new(key);

    let num_blocks = src.len() / CIPHER_BLOCK_SIZE;
    let out_len = num_blocks * CIPHER_BLOCK_SIZE;

    assert!(
        dest.len() >= out_len,
        "dest too small: need {out_len}, have {}",
        dest.len()
    );

    for i in 0..num_blocks {
        let start = i * CIPHER_BLOCK_SIZE;
        let mut block = GenericArray::clone_from_slice(&src[start..start + CIPHER_BLOCK_SIZE]);

        cipher.decrypt_block(&mut block);

        dest[start..start + CIPHER_BLOCK_SIZE].copy_from_slice(&block);
    }

    out_len
}

/// Compute a 2-byte truncated HMAC-SHA256 MAC.
///
/// The HMAC key is the full 32-byte `shared_secret` (not the 16-byte AES key).
/// Returns the first 2 bytes of the HMAC-SHA256 output.
pub fn compute_mac(shared_secret: &[u8; PUB_KEY_SIZE], data: &[u8]) -> [u8; CIPHER_MAC_SIZE] {
    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(shared_secret).expect("HMAC accepts any key length");
    mac.update(data);
    let result = mac.finalize().into_bytes();
    let mut truncated = [0u8; CIPHER_MAC_SIZE];
    truncated.copy_from_slice(&result[..CIPHER_MAC_SIZE]);
    truncated
}

/// Encrypt payload then prepend a 2-byte MAC (encrypt-then-MAC).
///
/// Output layout: `[MAC(2B)][ciphertext(padded to 16B boundary)]`
///
/// Returns the total number of bytes written to `dest`.
///
/// # Panics
///
/// Panics if `dest` is too small.
pub fn encrypt_then_mac(
    shared_secret: &[u8; PUB_KEY_SIZE],
    dest: &mut [u8],
    src: &[u8],
) -> usize {
    // Encrypt into dest after the MAC prefix
    let encrypted_len = aes_encrypt(shared_secret, &mut dest[CIPHER_MAC_SIZE..], src);

    // Compute MAC over the ciphertext
    let mac = compute_mac(
        shared_secret,
        &dest[CIPHER_MAC_SIZE..CIPHER_MAC_SIZE + encrypted_len],
    );

    // Write MAC at the front
    dest[..CIPHER_MAC_SIZE].copy_from_slice(&mac);

    CIPHER_MAC_SIZE + encrypted_len
}

/// Verify MAC then decrypt (MAC-then-decrypt).
///
/// Expects input layout: `[MAC(2B)][ciphertext]`.
/// Returns `Some(decrypted_len)` on success, or `None` if MAC verification fails.
pub fn mac_then_decrypt(
    shared_secret: &[u8; PUB_KEY_SIZE],
    dest: &mut [u8],
    src: &[u8],
) -> Option<usize> {
    if src.len() < CIPHER_MAC_SIZE {
        return None;
    }

    let received_mac: [u8; CIPHER_MAC_SIZE] = [src[0], src[1]];
    let ciphertext = &src[CIPHER_MAC_SIZE..];

    let computed_mac = compute_mac(shared_secret, ciphertext);

    if received_mac != computed_mac {
        return None;
    }

    let decrypted_len = aes_decrypt(shared_secret, dest, ciphertext);
    Some(decrypted_len)
}

/// Compute an ECDH shared secret from Ed25519 keys.
///
/// Converts Ed25519 keys to X25519 (Montgomery) form and performs
/// Diffie-Hellman, matching the behavior of MeshCore C's `ed25519_key_exchange`.
///
/// - `local_seed`: Ed25519 signing key seed (32 bytes)
/// - `remote_public`: Ed25519 verifying key (32 bytes, compressed Edwards Y)
///
/// Returns the 32-byte X25519 shared secret.
pub fn ecdh_shared_secret(
    local_seed: &[u8; 32],
    remote_public: &[u8; PUB_KEY_SIZE],
) -> [u8; PUB_KEY_SIZE] {
    use sha2::Digest;

    // Convert Ed25519 seed to X25519 static secret:
    // SHA-512(seed), take first 32 bytes, clamp per X25519 spec.
    let hash = Sha512::digest(local_seed);
    let mut x25519_secret_bytes = [0u8; 32];
    x25519_secret_bytes.copy_from_slice(&hash[..32]);
    x25519_secret_bytes[0] &= 248;
    x25519_secret_bytes[31] &= 127;
    x25519_secret_bytes[31] |= 64;

    // Convert Ed25519 public key (compressed Edwards Y) to X25519 (Montgomery U).
    // Uses curve25519-dalek's CompressedEdwardsY -> decompress -> to_montgomery.
    let compressed = curve25519_dalek::edwards::CompressedEdwardsY(*remote_public);
    let edwards_point = match compressed.decompress() {
        Some(pt) => pt,
        None => return [0u8; PUB_KEY_SIZE], // invalid point
    };
    let montgomery = edwards_point.to_montgomery();

    let x25519_pub = x25519_dalek::PublicKey::from(montgomery.to_bytes());
    let secret = x25519_dalek::StaticSecret::from(x25519_secret_bytes);
    let shared = secret.diffie_hellman(&x25519_pub);
    shared.to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_round_trip_exact_block() {
        let secret = [0xABu8; PUB_KEY_SIZE];
        let plaintext = [1u8; 16]; // exactly one block
        let mut ciphertext = [0u8; 16];
        let mut decrypted = [0u8; 16];

        let enc_len = aes_encrypt(&secret, &mut ciphertext, &plaintext);
        assert_eq!(enc_len, 16);
        assert_ne!(&ciphertext[..], &plaintext[..]);

        let dec_len = aes_decrypt(&secret, &mut decrypted, &ciphertext);
        assert_eq!(dec_len, 16);
        assert_eq!(&decrypted[..], &plaintext[..]);
    }

    #[test]
    fn aes_round_trip_partial_block() {
        let secret = [0x42u8; PUB_KEY_SIZE];
        let plaintext = b"hello world"; // 11 bytes, not a multiple of 16
        let mut ciphertext = [0u8; 16];
        let mut decrypted = [0u8; 16];

        let enc_len = aes_encrypt(&secret, &mut ciphertext, plaintext);
        assert_eq!(enc_len, 16);

        let dec_len = aes_decrypt(&secret, &mut decrypted, &ciphertext);
        assert_eq!(dec_len, 16);
        // First 11 bytes match; remaining 5 are zero padding
        assert_eq!(&decrypted[..11], &plaintext[..]);
        assert_eq!(&decrypted[11..16], &[0u8; 5]);
    }

    #[test]
    fn aes_round_trip_multi_block() {
        let secret = [0x77u8; PUB_KEY_SIZE];
        let plaintext = [0xFFu8; 33]; // 3 blocks (33 bytes -> 48 padded)
        let mut ciphertext = [0u8; 48];
        let mut decrypted = [0u8; 48];

        let enc_len = aes_encrypt(&secret, &mut ciphertext, &plaintext);
        assert_eq!(enc_len, 48);

        let dec_len = aes_decrypt(&secret, &mut decrypted, &ciphertext);
        assert_eq!(dec_len, 48);
        assert_eq!(&decrypted[..33], &plaintext[..]);
        assert_eq!(&decrypted[33..48], &[0u8; 15]);
    }

    #[test]
    fn mac_produces_two_bytes() {
        let secret = [0x01u8; PUB_KEY_SIZE];
        let data = b"test data";
        let mac = compute_mac(&secret, data);
        assert_eq!(mac.len(), CIPHER_MAC_SIZE);
    }

    #[test]
    fn mac_is_deterministic() {
        let secret = [0x55u8; PUB_KEY_SIZE];
        let data = b"deterministic";
        let mac1 = compute_mac(&secret, data);
        let mac2 = compute_mac(&secret, data);
        assert_eq!(mac1, mac2);
    }

    #[test]
    fn mac_differs_with_different_keys() {
        let secret1 = [0x01u8; PUB_KEY_SIZE];
        let secret2 = [0x02u8; PUB_KEY_SIZE];
        let data = b"same data";
        let mac1 = compute_mac(&secret1, data);
        let mac2 = compute_mac(&secret2, data);
        assert_ne!(mac1, mac2);
    }

    #[test]
    fn encrypt_then_mac_round_trip() {
        let secret = [0xCDu8; PUB_KEY_SIZE];
        let plaintext = b"authenticated encryption";
        let mut encrypted = [0u8; 256];
        let mut decrypted = [0u8; 256];

        let enc_len = encrypt_then_mac(&secret, &mut encrypted, plaintext);
        assert_eq!(enc_len, CIPHER_MAC_SIZE + 32); // 24 bytes -> 2 blocks = 32 + 2

        let dec_len = mac_then_decrypt(&secret, &mut decrypted, &encrypted[..enc_len]);
        assert!(dec_len.is_some());
        let dec_len = dec_len.unwrap();
        assert_eq!(&decrypted[..plaintext.len()], &plaintext[..]);
        assert_eq!(dec_len, 32); // padded to block boundary
    }

    #[test]
    fn mac_verification_failure() {
        let secret = [0xAAu8; PUB_KEY_SIZE];
        let plaintext = b"tamper test";
        let mut encrypted = [0u8; 256];
        let mut decrypted = [0u8; 256];

        let enc_len = encrypt_then_mac(&secret, &mut encrypted, plaintext);

        // Tamper with the ciphertext (after the MAC)
        encrypted[CIPHER_MAC_SIZE] ^= 0xFF;

        let result = mac_then_decrypt(&secret, &mut decrypted, &encrypted[..enc_len]);
        assert!(result.is_none());
    }

    #[test]
    fn mac_verification_failure_tampered_mac() {
        let secret = [0xBBu8; PUB_KEY_SIZE];
        let plaintext = b"tamper mac";
        let mut encrypted = [0u8; 256];
        let mut decrypted = [0u8; 256];

        let enc_len = encrypt_then_mac(&secret, &mut encrypted, plaintext);

        // Tamper with the MAC itself
        encrypted[0] ^= 0xFF;

        let result = mac_then_decrypt(&secret, &mut decrypted, &encrypted[..enc_len]);
        assert!(result.is_none());
    }

    #[test]
    fn ecdh_two_keypairs_same_shared_secret() {
        // Generate two Ed25519 keypairs from known seeds and verify
        // that A's DH(a, B) == B's DH(b, A).
        use ed25519_dalek::SigningKey;

        let seed_a = [1u8; 32];
        let seed_b = [2u8; 32];

        let key_a = SigningKey::from_bytes(&seed_a);
        let key_b = SigningKey::from_bytes(&seed_b);

        let pub_a = key_a.verifying_key().to_bytes();
        let pub_b = key_b.verifying_key().to_bytes();

        let shared_ab = ecdh_shared_secret(&seed_a, &pub_b);
        let shared_ba = ecdh_shared_secret(&seed_b, &pub_a);

        assert_eq!(shared_ab, shared_ba);
        // Ensure it's not all zeros (degenerate case)
        assert_ne!(shared_ab, [0u8; 32]);
    }

    #[test]
    fn aes_empty_input() {
        let secret = [0x11u8; PUB_KEY_SIZE];
        let mut ciphertext = [0u8; 16];

        let enc_len = aes_encrypt(&secret, &mut ciphertext, &[]);
        assert_eq!(enc_len, 0);
    }
}
