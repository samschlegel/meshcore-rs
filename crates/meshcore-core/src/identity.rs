//! Ed25519 identity types for MeshCore.
//!
//! Provides [`PublicKey`], [`PathHash`], and [`LocalIdentity`] for signing,
//! verification, and routing path identification. Wire-compatible with the
//! MeshCore C implementation.
//!
//! Reference: `Identity.h/cpp` in the C codebase.

use crate::constants::{PUB_KEY_SIZE, SIGNATURE_SIZE};
use ed25519_dalek::{Signer, Verifier};

/// A single-byte hash derived from the first byte of a public key.
///
/// Used in routing paths to compactly identify nodes. Matches the C
/// implementation's use of `pub_key[0]` as a path hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct PathHash(pub u8);

/// An Ed25519 public key (32 bytes).
///
/// Wraps the raw key bytes and provides verification and path-hash derivation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PublicKey(pub [u8; PUB_KEY_SIZE]);

impl PublicKey {
    /// Returns the path hash for this public key (first byte).
    ///
    /// Matches the C implementation: `pub_key[0]`.
    pub fn path_hash(&self) -> PathHash {
        PathHash(self.0[0])
    }

    /// Checks whether the given hash bytes match the prefix of this public key.
    pub fn is_hash_match(&self, hash: &[u8]) -> bool {
        if hash.is_empty() || hash.len() > self.0.len() {
            return false;
        }
        self.0[..hash.len()] == *hash
    }

    /// Verify an Ed25519 signature against this public key.
    ///
    /// Returns `true` if the signature is valid for the given message.
    pub fn verify(&self, message: &[u8], signature: &[u8; SIGNATURE_SIZE]) -> bool {
        let Ok(verifying_key) = ed25519_dalek::VerifyingKey::from_bytes(&self.0) else {
            return false;
        };
        let sig = ed25519_dalek::Signature::from_bytes(signature);
        verifying_key.verify(message, &sig).is_ok()
    }
}

impl core::fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "PublicKey({:02x}{:02x}..{:02x}{:02x})",
            self.0[0], self.0[1], self.0[30], self.0[31],
        )
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for PublicKey {
    fn format(&self, f: defmt::Formatter<'_>) {
        defmt::write!(
            f,
            "PublicKey({:02x}{:02x}..{:02x}{:02x})",
            self.0[0],
            self.0[1],
            self.0[30],
            self.0[31],
        );
    }
}

impl From<[u8; PUB_KEY_SIZE]> for PublicKey {
    fn from(bytes: [u8; PUB_KEY_SIZE]) -> Self {
        Self(bytes)
    }
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// A local node identity containing the Ed25519 signing key.
///
/// The signing key holds both the secret seed and the derived public key.
pub struct LocalIdentity {
    signing_key: ed25519_dalek::SigningKey,
}

impl LocalIdentity {
    /// Create a `LocalIdentity` from a 32-byte seed.
    ///
    /// The seed is expanded internally by `ed25519-dalek` to produce the
    /// full signing key.
    pub fn from_bytes(secret: &[u8; 32]) -> Self {
        Self {
            signing_key: ed25519_dalek::SigningKey::from_bytes(secret),
        }
    }

    /// Extract the public key from this identity.
    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.signing_key.verifying_key().to_bytes())
    }

    /// Convenience: returns the path hash for this identity's public key.
    pub fn path_hash(&self) -> PathHash {
        self.public_key().path_hash()
    }

    /// Sign a message using Ed25519, returning the 64-byte signature.
    pub fn sign(&self, message: &[u8]) -> [u8; SIGNATURE_SIZE] {
        self.signing_key.sign(message).to_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_seed() -> [u8; 32] {
        [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
            0x1d, 0x1e, 0x1f, 0x20,
        ]
    }

    #[test]
    fn sign_and_verify() {
        let identity = LocalIdentity::from_bytes(&test_seed());
        let message = b"hello meshcore";
        let signature = identity.sign(message);
        let pubkey = identity.public_key();

        assert!(pubkey.verify(message, &signature));
    }

    #[test]
    fn path_hash_is_first_byte() {
        let identity = LocalIdentity::from_bytes(&test_seed());
        let pubkey = identity.public_key();

        assert_eq!(pubkey.path_hash(), PathHash(pubkey.0[0]));
        assert_eq!(identity.path_hash(), pubkey.path_hash());
    }

    #[test]
    fn is_hash_match_works() {
        let identity = LocalIdentity::from_bytes(&test_seed());
        let pubkey = identity.public_key();

        // Single byte match
        assert!(pubkey.is_hash_match(&[pubkey.0[0]]));
        // Multi-byte prefix match
        assert!(pubkey.is_hash_match(&pubkey.0[..4]));
        // Full key match
        assert!(pubkey.is_hash_match(&pubkey.0));
        // Wrong byte
        let wrong = pubkey.0[0].wrapping_add(1);
        assert!(!pubkey.is_hash_match(&[wrong]));
        // Empty slice
        assert!(!pubkey.is_hash_match(&[]));
    }

    #[test]
    fn invalid_signature_fails() {
        let identity = LocalIdentity::from_bytes(&test_seed());
        let message = b"hello meshcore";
        let mut signature = identity.sign(message);
        signature[0] ^= 0xff;

        assert!(!identity.public_key().verify(message, &signature));
    }

    #[test]
    fn wrong_message_fails() {
        let identity = LocalIdentity::from_bytes(&test_seed());
        let signature = identity.sign(b"hello meshcore");

        assert!(!identity.public_key().verify(b"wrong message", &signature));
    }

    #[test]
    fn public_key_from_bytes() {
        let bytes = [0xaa; PUB_KEY_SIZE];
        let pubkey = PublicKey::from(bytes);
        assert_eq!(pubkey.0, bytes);
        assert_eq!(pubkey.as_ref(), &bytes);
    }

    #[test]
    fn debug_format() {
        use core::fmt::Write;

        let identity = LocalIdentity::from_bytes(&test_seed());
        let pubkey = identity.public_key();

        let mut buf = heapless::String::<64>::new();
        write!(buf, "{:?}", pubkey).unwrap();
        assert!(buf.starts_with("PublicKey("));
        assert!(buf.ends_with(")"));
    }
}
