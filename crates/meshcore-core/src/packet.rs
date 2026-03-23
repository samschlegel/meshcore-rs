//! Wire packet type for the MeshCore protocol.
//!
//! The wire format is:
//! ```text
//! [header(1B)][transport_codes(4B, if route has transport)][path_len(1B)][path(0-64B)][payload(0-184B)]
//! ```
//!
//! The `path_len` byte encodes both the hash size and count:
//! - Bits 0-5: hash count (0-63)
//! - Bits 6-7: hash size encoding (0=1B, 1=2B, 2=3B, 3=invalid)

use crate::constants::{MAX_PACKET_PAYLOAD, MAX_PATH_SIZE, PH_ROUTE_MASK};
use crate::header::{
    HeaderError, PacketHeader, PayloadType, RouteType, DO_NOT_RETRANSMIT,
};
use heapless::Vec;

/// Mask for the hash count bits within the path_len byte.
const PATH_HASH_COUNT_MASK: u8 = 0x3F;

/// Bit shift for the hash size encoding within the path_len byte.
const PATH_HASH_SIZE_SHIFT: u8 = 6;

/// A MeshCore wire packet.
///
/// Contains the raw header byte, path data, payload, and optional transport
/// codes. The SNR field is populated by the radio receiver and is NOT
/// serialized to the wire.
#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Packet {
    /// Raw header byte (encodes route type, payload type, and version).
    pub header: u8,
    /// Encoded path length byte: bits 0-5 = hash count, bits 6-7 = hash size encoding.
    pub path_len: u8,
    /// Transport codes (two u16 values). Only valid when the route type has transport codes.
    pub transport_codes: [u16; 2],
    /// Path bytes (hash_count * hash_size bytes).
    pub path: Vec<u8, MAX_PATH_SIZE>,
    /// Payload bytes.
    pub payload: Vec<u8, MAX_PACKET_PAYLOAD>,
    /// Signal-to-noise ratio from the radio receiver. Not serialized to wire.
    pub snr: i8,
}

impl Packet {
    /// Create a new empty packet with all fields zeroed.
    pub fn new() -> Self {
        Self {
            header: 0,
            path_len: 0,
            transport_codes: [0, 0],
            path: Vec::new(),
            payload: Vec::new(),
            snr: 0,
        }
    }

    /// Parse the raw header byte into a [`PacketHeader`].
    pub fn parsed_header(&self) -> Result<PacketHeader, HeaderError> {
        PacketHeader::try_from(self.header)
    }

    /// Get the route type from the raw header byte.
    pub fn route_type(&self) -> Result<RouteType, HeaderError> {
        RouteType::try_from(self.header & PH_ROUTE_MASK)
    }

    /// Get the payload type from the raw header byte.
    pub fn payload_type(&self) -> Result<PayloadType, HeaderError> {
        let parsed = self.parsed_header()?;
        Ok(parsed.payload_type)
    }

    /// Whether this packet has transport codes (determined from the route type bits).
    ///
    /// Returns `false` if the route type bits are invalid.
    pub fn has_transport_codes(&self) -> bool {
        self.route_type()
            .map(|rt| rt.has_transport_codes())
            .unwrap_or(false)
    }

    // --- Path encoding helpers ---

    /// Get the hash size in bytes (1, 2, or 3) from the path_len encoding.
    ///
    /// The encoding is `(path_len >> 6) + 1`, so:
    /// - 0b00 = 1 byte per hash
    /// - 0b01 = 2 bytes per hash
    /// - 0b10 = 3 bytes per hash
    /// - 0b11 = 4 (invalid, but returned as-is; check [`is_valid_path_len`])
    pub fn path_hash_size(&self) -> u8 {
        (self.path_len >> PATH_HASH_SIZE_SHIFT) + 1
    }

    /// Get the hash count from the path_len encoding (bits 0-5).
    pub fn path_hash_count(&self) -> u8 {
        self.path_len & PATH_HASH_COUNT_MASK
    }

    /// Get the total path byte length on the wire: `hash_count * hash_size`.
    pub fn path_byte_len(&self) -> usize {
        (self.path_hash_count() as usize) * (self.path_hash_size() as usize)
    }

    /// Set the path hash count (bits 0-5), preserving the hash size bits (6-7).
    pub fn set_path_hash_count(&mut self, count: u8) {
        self.path_len = (self.path_len & !PATH_HASH_COUNT_MASK) | (count & PATH_HASH_COUNT_MASK);
    }

    /// Set both hash size and count in the path_len field.
    ///
    /// `size` is the hash size in bytes (1, 2, or 3). The encoding stored is `size - 1`.
    /// `count` is the number of hashes (0-63).
    pub fn set_path_hash_size_and_count(&mut self, size: u8, count: u8) {
        let size_enc = (size.saturating_sub(1)) & 0x03;
        self.path_len = (size_enc << PATH_HASH_SIZE_SHIFT) | (count & PATH_HASH_COUNT_MASK);
    }

    /// Check if the path_len encoding is valid.
    ///
    /// Returns `false` if the hash size encoding bits are `0b11` (which would
    /// mean 4 bytes per hash, not supported by the protocol).
    pub fn is_valid_path_len(&self) -> bool {
        (self.path_len >> PATH_HASH_SIZE_SHIFT) != 0x03
    }

    // --- Serialization ---

    /// Compute the total wire length of this packet.
    pub fn wire_len(&self) -> usize {
        let mut len = 1; // header
        if self.has_transport_codes() {
            len += 4; // two u16 values
        }
        len += 1; // path_len byte
        len += self.path_byte_len();
        len += self.payload.len();
        len
    }

    /// Serialize the packet to a byte buffer in wire format.
    ///
    /// Returns the number of bytes written. The caller must ensure `dest` is
    /// large enough (at least [`wire_len`](Self::wire_len) bytes).
    pub fn write_to(&self, dest: &mut [u8]) -> usize {
        let mut i = 0;

        dest[i] = self.header;
        i += 1;

        if self.has_transport_codes() {
            let tc0 = self.transport_codes[0].to_le_bytes();
            dest[i] = tc0[0];
            dest[i + 1] = tc0[1];
            i += 2;
            let tc1 = self.transport_codes[1].to_le_bytes();
            dest[i] = tc1[0];
            dest[i + 1] = tc1[1];
            i += 2;
        }

        dest[i] = self.path_len;
        i += 1;

        let pbl = self.path_byte_len();
        if pbl > 0 {
            dest[i..i + pbl].copy_from_slice(&self.path.as_slice()[..pbl]);
            i += pbl;
        }

        let payload_len = self.payload.len();
        if payload_len > 0 {
            dest[i..i + payload_len].copy_from_slice(self.payload.as_slice());
            i += payload_len;
        }

        i
    }

    /// Deserialize a packet from a byte buffer in wire format.
    ///
    /// Returns `true` on success, `false` if the input is malformed or too short.
    /// On failure, the packet state is partially modified.
    pub fn read_from(&mut self, src: &[u8]) -> bool {
        let len = src.len();
        if len < 2 {
            return false;
        }

        let mut i = 0;

        self.header = src[i];
        i += 1;

        if self.has_transport_codes() {
            if i + 4 > len {
                return false;
            }
            self.transport_codes[0] = u16::from_le_bytes([src[i], src[i + 1]]);
            i += 2;
            self.transport_codes[1] = u16::from_le_bytes([src[i], src[i + 1]]);
            i += 2;
        } else {
            self.transport_codes = [0, 0];
        }

        if i >= len {
            return false;
        }
        self.path_len = src[i];
        i += 1;

        if !self.is_valid_path_len() {
            return false;
        }

        let bl = self.path_byte_len();
        if i + bl > len {
            return false;
        }
        self.path.clear();
        if bl > 0 && self.path.extend_from_slice(&src[i..i + bl]).is_err() {
            return false;
        }
        i += bl;

        if i > len {
            return false;
        }

        let payload_len = len - i;
        if payload_len > MAX_PACKET_PAYLOAD {
            return false;
        }

        self.payload.clear();
        if payload_len > 0
            && self
                .payload
                .extend_from_slice(&src[i..i + payload_len])
                .is_err()
        {
            return false;
        }

        true
    }

    // --- Do-not-retransmit marker ---

    /// Mark this packet as "do not retransmit" by setting the header to `0xFF`.
    pub fn mark_do_not_retransmit(&mut self) {
        self.header = DO_NOT_RETRANSMIT;
    }

    /// Check if this packet is marked "do not retransmit" (header == `0xFF`).
    pub fn is_do_not_retransmit(&self) -> bool {
        self.header == DO_NOT_RETRANSMIT
    }

    /// Get the SNR as a float value (snr / 4.0).
    pub fn snr_f32(&self) -> f32 {
        self.snr as f32 / 4.0
    }
}

impl Default for Packet {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::{PacketHeader, PayloadType, PayloadVersion, RouteType};

    #[test]
    fn new_creates_valid_empty_packet() {
        let pkt = Packet::new();
        assert_eq!(pkt.header, 0);
        assert_eq!(pkt.path_len, 0);
        assert_eq!(pkt.transport_codes, [0, 0]);
        assert!(pkt.path.is_empty());
        assert!(pkt.payload.is_empty());
        assert_eq!(pkt.snr, 0);
        assert!(pkt.is_valid_path_len());
        assert_eq!(pkt.path_hash_size(), 1);
        assert_eq!(pkt.path_hash_count(), 0);
        assert_eq!(pkt.path_byte_len(), 0);
    }

    #[test]
    fn path_encoding_size1_count5() {
        let mut pkt = Packet::new();
        pkt.set_path_hash_size_and_count(1, 5);
        assert_eq!(pkt.path_len, 0x05);
        assert_eq!(pkt.path_hash_size(), 1);
        assert_eq!(pkt.path_hash_count(), 5);
        assert_eq!(pkt.path_byte_len(), 5);
        assert!(pkt.is_valid_path_len());
    }

    #[test]
    fn path_encoding_size2_count10() {
        let mut pkt = Packet::new();
        pkt.set_path_hash_size_and_count(2, 10);
        assert_eq!(pkt.path_len, 0x4A);
        assert_eq!(pkt.path_hash_size(), 2);
        assert_eq!(pkt.path_hash_count(), 10);
        assert_eq!(pkt.path_byte_len(), 20);
        assert!(pkt.is_valid_path_len());
    }

    #[test]
    fn path_encoding_invalid_size_bits() {
        let mut pkt = Packet::new();
        pkt.path_len = 0xC0 | 5; // 0b11_000101
        assert!(!pkt.is_valid_path_len());
    }

    #[test]
    fn round_trip_flood_no_transport() {
        let header_byte: u8 = PacketHeader {
            route_type: RouteType::Flood,
            payload_type: PayloadType::TxtMsg,
            version: PayloadVersion::V1,
        }
        .into();

        let mut pkt = Packet::new();
        pkt.header = header_byte;
        pkt.set_path_hash_size_and_count(1, 2);
        pkt.path.extend_from_slice(&[0xAA, 0xBB]).unwrap();
        pkt.payload
            .extend_from_slice(b"Hello, mesh!")
            .unwrap();

        let mut buf = [0u8; 255];
        let written = pkt.write_to(&mut buf);
        assert_eq!(written, pkt.wire_len());

        let mut pkt2 = Packet::new();
        assert!(pkt2.read_from(&buf[..written]));

        assert_eq!(pkt2.header, pkt.header);
        assert_eq!(pkt2.path_len, pkt.path_len);
        assert_eq!(pkt2.transport_codes, [0, 0]);
        assert_eq!(pkt2.path.as_slice(), pkt.path.as_slice());
        assert_eq!(pkt2.payload.as_slice(), pkt.payload.as_slice());
    }

    #[test]
    fn round_trip_transport_flood() {
        let header_byte: u8 = PacketHeader {
            route_type: RouteType::TransportFlood,
            payload_type: PayloadType::Advert,
            version: PayloadVersion::V1,
        }
        .into();

        let mut pkt = Packet::new();
        pkt.header = header_byte;
        pkt.transport_codes = [0x1234, 0x5678];
        pkt.set_path_hash_size_and_count(1, 0);
        pkt.payload
            .extend_from_slice(&[1, 2, 3, 4, 5])
            .unwrap();

        let mut buf = [0u8; 255];
        let written = pkt.write_to(&mut buf);
        assert_eq!(written, pkt.wire_len());

        let mut pkt2 = Packet::new();
        assert!(pkt2.read_from(&buf[..written]));

        assert_eq!(pkt2.header, pkt.header);
        assert_eq!(pkt2.transport_codes, [0x1234, 0x5678]);
        assert_eq!(pkt2.payload.as_slice(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn round_trip_direct_with_path() {
        let header_byte: u8 = PacketHeader {
            route_type: RouteType::Direct,
            payload_type: PayloadType::Response,
            version: PayloadVersion::V1,
        }
        .into();

        let mut pkt = Packet::new();
        pkt.header = header_byte;
        pkt.set_path_hash_size_and_count(2, 3);
        pkt.path
            .extend_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06])
            .unwrap();
        pkt.payload
            .extend_from_slice(b"response data")
            .unwrap();

        let mut buf = [0u8; 255];
        let written = pkt.write_to(&mut buf);
        assert_eq!(written, pkt.wire_len());

        let mut pkt2 = Packet::new();
        assert!(pkt2.read_from(&buf[..written]));

        assert_eq!(pkt2.header, pkt.header);
        assert_eq!(pkt2.path_len, pkt.path_len);
        assert_eq!(pkt2.path_hash_size(), 2);
        assert_eq!(pkt2.path_hash_count(), 3);
        assert_eq!(pkt2.path.as_slice(), &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        assert_eq!(pkt2.payload.as_slice(), b"response data");
    }

    #[test]
    fn read_from_rejects_too_short() {
        let mut pkt = Packet::new();
        assert!(!pkt.read_from(&[]));
        assert!(!pkt.read_from(&[0x09]));
    }

    #[test]
    fn read_from_rejects_invalid_path_len() {
        let data = [0x09, 0xC5]; // header=0x09, path_len=0b11_000101
        let mut pkt = Packet::new();
        assert!(!pkt.read_from(&data));
    }

    #[test]
    fn do_not_retransmit_marker() {
        let mut pkt = Packet::new();
        assert!(!pkt.is_do_not_retransmit());

        pkt.mark_do_not_retransmit();
        assert!(pkt.is_do_not_retransmit());
        assert_eq!(pkt.header, 0xFF);
    }

    #[test]
    fn wire_len_matches_write_to() {
        let configs: Vec<(u8, bool, u8, u8, usize), 8> = {
            let mut v = Vec::new();
            let _ = v.push((0x09, false, 1, 0, 0));
            let _ = v.push((0x09, false, 1, 3, 10));
            let _ = v.push((0x10, true, 1, 0, 5));
            let _ = v.push((0x0F, true, 2, 4, 20));
            v
        };

        for &(header, _has_tc, hash_size, hash_count, payload_len) in configs.iter() {
            let mut pkt = Packet::new();
            pkt.header = header;
            pkt.set_path_hash_size_and_count(hash_size, hash_count);

            let path_bytes = (hash_count as usize) * (hash_size as usize);
            for _ in 0..path_bytes {
                let _ = pkt.path.push(0xAA);
            }
            for _ in 0..payload_len {
                let _ = pkt.payload.push(0xBB);
            }

            if pkt.has_transport_codes() {
                pkt.transport_codes = [0x1111, 0x2222];
            }

            let mut buf = [0u8; 255];
            let written = pkt.write_to(&mut buf);
            assert_eq!(
                written,
                pkt.wire_len(),
                "wire_len mismatch for header={:#04x}",
                header
            );
        }
    }

    #[test]
    fn snr_f32_conversion() {
        let mut pkt = Packet::new();
        pkt.snr = 20;
        assert!((pkt.snr_f32() - 5.0).abs() < f32::EPSILON);

        pkt.snr = -8;
        assert!((pkt.snr_f32() - (-2.0)).abs() < f32::EPSILON);

        pkt.snr = 0;
        assert!((pkt.snr_f32() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn set_path_hash_count_preserves_size() {
        let mut pkt = Packet::new();
        pkt.set_path_hash_size_and_count(2, 5);
        assert_eq!(pkt.path_hash_size(), 2);
        assert_eq!(pkt.path_hash_count(), 5);

        pkt.set_path_hash_count(10);
        assert_eq!(pkt.path_hash_size(), 2); // preserved
        assert_eq!(pkt.path_hash_count(), 10);
    }

    #[test]
    fn transport_codes_little_endian() {
        let mut pkt = Packet::new();
        pkt.header = 0x00; // TransportFlood
        pkt.transport_codes = [0x0102, 0x0304];
        pkt.set_path_hash_size_and_count(1, 0);

        let mut buf = [0u8; 255];
        let written = pkt.write_to(&mut buf);

        assert_eq!(buf[1], 0x02);
        assert_eq!(buf[2], 0x01);
        assert_eq!(buf[3], 0x04);
        assert_eq!(buf[4], 0x03);

        let mut pkt2 = Packet::new();
        assert!(pkt2.read_from(&buf[..written]));
        assert_eq!(pkt2.transport_codes, [0x0102, 0x0304]);
    }
}
