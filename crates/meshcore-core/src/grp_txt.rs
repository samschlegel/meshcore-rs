//! GrpTxt (group text) payload codec.
//!
//! Encodes and decodes the `PayloadType::GrpTxt` payload format used for
//! channel-based group messages.
//!
//! ## Wire format
//!
//! ```text
//! [channel_hash(1B)][cipher_mac(2B)][ciphertext...]
//! ```
//!
//! The plaintext inside the ciphertext is:
//! ```text
//! [timestamp(4B LE)][tx_type(1B)][message...]
//! ```
//!
//! The message is typically `"<sender>: <body>"`.

use crate::constants::{CIPHER_MAC_SIZE, MAX_PACKET_PAYLOAD, PUB_KEY_SIZE};
use crate::crypto::{encrypt_then_mac, mac_then_decrypt};
use crate::header::{PacketHeader, PayloadType, PayloadVersion, RouteType};
use crate::packet::Packet;

/// Minimum payload size: 1 (channel_hash) + 2 (MAC) + 16 (one AES block).
const MIN_GRP_TXT_PAYLOAD: usize = 1 + CIPHER_MAC_SIZE + 16;

/// Plaintext header size: 4 (timestamp) + 1 (tx_type).
const PLAINTEXT_HEADER: usize = 5;

/// Result of decoding a GrpTxt payload.
pub struct DecodedGrpTxt<'a> {
    /// Unix timestamp from the sender.
    pub timestamp: u32,
    /// The tx_type flags byte (0x00 = plain text).
    pub tx_type: u8,
    /// The message text (may contain `"sender: body"` format).
    pub message: &'a str,
}

/// Decode a GrpTxt payload using the channel's shared secret.
///
/// `payload` is the raw packet payload (starting with channel_hash byte).
/// `secret` is the 32-byte channel shared secret (SHA256 of channel name, first 16 bytes used as key).
/// `scratch` is a temporary buffer for decryption (must be >= payload length).
///
/// Returns `None` if the payload is too short, MAC verification fails, or the
/// decrypted content is not valid UTF-8.
pub fn decode_grp_txt<'a>(
    secret: &[u8; PUB_KEY_SIZE],
    payload: &[u8],
    scratch: &'a mut [u8],
) -> Option<DecodedGrpTxt<'a>> {
    if payload.len() < MIN_GRP_TXT_PAYLOAD {
        return None;
    }

    // Skip the channel_hash byte; the rest is [MAC(2B)][ciphertext...]
    let mac_and_cipher = &payload[1..];

    let decrypted_len = mac_then_decrypt(secret, scratch, mac_and_cipher)?;

    if decrypted_len < PLAINTEXT_HEADER {
        return None;
    }

    let timestamp = u32::from_le_bytes([scratch[0], scratch[1], scratch[2], scratch[3]]);
    let tx_type = scratch[4];

    // Find the actual message length (strip zero-padding from AES block alignment).
    let msg_bytes = &scratch[PLAINTEXT_HEADER..decrypted_len];
    let msg_len = msg_bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);

    let message = core::str::from_utf8(&msg_bytes[..msg_len]).ok()?;

    Some(DecodedGrpTxt {
        timestamp,
        tx_type,
        message,
    })
}

/// Check if a payload's channel_hash byte matches a given hash.
///
/// Useful for quickly filtering packets before attempting decryption.
pub fn matches_channel(payload: &[u8], channel_hash: u8) -> bool {
    payload.first().copied() == Some(channel_hash)
}

/// Encode a GrpTxt payload and return a ready-to-send `Packet`.
///
/// - `secret`: 32-byte channel shared secret
/// - `channel_hash`: first byte of SHA256 of the secret (pre-computed)
/// - `timestamp`: unix timestamp
/// - `message`: the full message string (e.g. `"bot: pong"`)
///
/// Returns `None` if the message is too large to fit in a packet.
pub fn encode_grp_txt(
    secret: &[u8; PUB_KEY_SIZE],
    channel_hash: u8,
    timestamp: u32,
    message: &[u8],
) -> Option<Packet> {
    // Build plaintext: [timestamp(4B LE)][tx_type(1B)][message...]
    let plaintext_len = PLAINTEXT_HEADER + message.len();
    if plaintext_len > MAX_PACKET_PAYLOAD {
        return None;
    }

    let mut plaintext = [0u8; MAX_PACKET_PAYLOAD];
    plaintext[..4].copy_from_slice(&timestamp.to_le_bytes());
    plaintext[4] = 0x00; // tx_type: plain text
    plaintext[5..5 + message.len()].copy_from_slice(message);

    // Encrypt: output is [MAC(2B)][ciphertext...]
    let mut encrypted = [0u8; MAX_PACKET_PAYLOAD];
    let enc_len = encrypt_then_mac(secret, &mut encrypted, &plaintext[..plaintext_len]);

    // Total payload: [channel_hash(1B)][MAC+ciphertext]
    let payload_len = 1 + enc_len;
    if payload_len > MAX_PACKET_PAYLOAD {
        return None;
    }

    let header_byte: u8 = PacketHeader {
        route_type: RouteType::Flood,
        payload_type: PayloadType::GrpTxt,
        version: PayloadVersion::V1,
    }
    .into();

    let mut pkt = Packet::new();
    pkt.header = header_byte;
    pkt.set_path_hash_size_and_count(1, 0);
    pkt.payload.push(channel_hash).ok()?;
    pkt.payload.extend_from_slice(&encrypted[..enc_len]).ok()?;

    Some(pkt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// Derive the channel secret and hash from a channel name, matching the firmware logic.
    fn channel_key(name: &[u8]) -> ([u8; 32], u8) {
        let mut secret = [0u8; 32];
        let hash = Sha256::digest(name);
        secret[..16].copy_from_slice(&hash[..16]);

        let channel_hash = {
            let h2 = Sha256::digest(&secret[..16]);
            h2[0]
        };

        (secret, channel_hash)
    }

    #[test]
    fn roundtrip_basic() {
        let (secret, ch_hash) = channel_key(b"#meshcore-rs");
        let message = b"bot: hello world";
        let timestamp = 1700000000u32;

        let pkt = encode_grp_txt(&secret, ch_hash, timestamp, message).unwrap();

        assert_eq!(pkt.payload[0], ch_hash);
        assert!(pkt.payload.len() > MIN_GRP_TXT_PAYLOAD);

        let mut scratch = [0u8; 256];
        let decoded =
            decode_grp_txt(&secret, pkt.payload.as_slice(), &mut scratch).unwrap();

        assert_eq!(decoded.timestamp, timestamp);
        assert_eq!(decoded.tx_type, 0x00);
        assert_eq!(decoded.message, "bot: hello world");
    }

    #[test]
    fn roundtrip_empty_message() {
        let (secret, ch_hash) = channel_key(b"#test");
        let pkt = encode_grp_txt(&secret, ch_hash, 42, b"").unwrap();

        let mut scratch = [0u8; 256];
        let decoded =
            decode_grp_txt(&secret, pkt.payload.as_slice(), &mut scratch).unwrap();

        assert_eq!(decoded.timestamp, 42);
        assert_eq!(decoded.message, "");
    }

    #[test]
    fn wrong_key_fails_mac() {
        let (secret, ch_hash) = channel_key(b"#meshcore-rs");
        let pkt = encode_grp_txt(&secret, ch_hash, 100, b"secret msg").unwrap();

        let (wrong_secret, _) = channel_key(b"#wrong-channel");
        let mut scratch = [0u8; 256];
        let result = decode_grp_txt(&wrong_secret, pkt.payload.as_slice(), &mut scratch);

        assert!(result.is_none());
    }

    #[test]
    fn too_short_payload_fails() {
        let secret = [0u8; 32];
        let short_payload = [0u8; 3]; // way too short
        let mut scratch = [0u8; 256];

        assert!(decode_grp_txt(&secret, &short_payload, &mut scratch).is_none());
    }

    #[test]
    fn matches_channel_works() {
        assert!(matches_channel(&[0xAB, 0x01, 0x02], 0xAB));
        assert!(!matches_channel(&[0xAB, 0x01, 0x02], 0xCD));
        assert!(!matches_channel(&[], 0xAB));
    }

    #[test]
    fn packet_header_is_grp_txt_flood() {
        let (secret, ch_hash) = channel_key(b"#test");
        let pkt = encode_grp_txt(&secret, ch_hash, 0, b"hi").unwrap();
        let hdr = pkt.parsed_header().unwrap();
        assert_eq!(hdr.payload_type, PayloadType::GrpTxt);
        assert_eq!(hdr.route_type, RouteType::Flood);
        assert_eq!(hdr.version, PayloadVersion::V1);
    }

    #[test]
    fn wire_roundtrip() {
        // Encode → serialize → deserialize → decode
        let (secret, ch_hash) = channel_key(b"#meshcore-rs");
        let pkt = encode_grp_txt(&secret, ch_hash, 1234567890, b"meshcore-rs: test").unwrap();

        let mut wire = [0u8; 255];
        let wire_len = pkt.write_to(&mut wire);

        let mut pkt2 = Packet::new();
        assert!(pkt2.read_from(&wire[..wire_len]));

        let mut scratch = [0u8; 256];
        let decoded =
            decode_grp_txt(&secret, pkt2.payload.as_slice(), &mut scratch).unwrap();
        assert_eq!(decoded.message, "meshcore-rs: test");
        assert_eq!(decoded.timestamp, 1234567890);
    }
}
