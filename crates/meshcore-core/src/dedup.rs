//! Packet deduplication using truncated SHA-256 hashes.
//!
//! Matches the MeshCore C `SimpleMeshTables` implementation: a fixed-size
//! ring buffer of 8-byte packet hashes. When the buffer is full, the oldest
//! entry is overwritten (FIFO eviction, no TTL).
//!
//! The packet hash is `SHA-256(payload_type || payload)`, truncated to 8 bytes.
//! For TRACE packets, `path_len` is also included to distinguish return paths.

use sha2::{Digest, Sha256};

use crate::constants::MAX_HASH_SIZE;
use crate::header::PayloadType;
use crate::packet::Packet;

/// A truncated packet hash (8 bytes of SHA-256).
type PacketHash = [u8; MAX_HASH_SIZE];

const ZERO_HASH: PacketHash = [0u8; MAX_HASH_SIZE];

/// Compute the deduplication hash for a packet.
///
/// Hash = SHA-256(payload_type_byte || payload), truncated to 8 bytes.
/// For TRACE packets, path_len is included after the type byte.
pub fn packet_hash(packet: &Packet) -> PacketHash {
    let mut sha = Sha256::new();

    let payload_type_byte = (packet.header >> 2) & 0x0F;
    sha.update([payload_type_byte]);

    // TRACE packets include path_len to distinguish forward/return paths
    if payload_type_byte == PayloadType::Trace as u8 {
        sha.update([packet.path_len]);
    }

    sha.update(packet.payload.as_slice());

    let digest = sha.finalize();
    let mut hash = [0u8; MAX_HASH_SIZE];
    hash.copy_from_slice(&digest[..MAX_HASH_SIZE]);
    hash
}

/// Packet deduplication table — ring buffer of truncated SHA-256 hashes.
///
/// `N` is the capacity (number of entries). The C implementation uses 128.
///
/// # Usage
///
/// ```ignore
/// let mut dedup = PacketDedup::<128>::new();
///
/// // Returns false on first encounter, true on duplicates
/// assert!(!dedup.has_seen(&packet));  // first time → inserts + returns false
/// assert!(dedup.has_seen(&packet));   // duplicate → returns true
/// ```
pub struct PacketDedup<const N: usize> {
    hashes: [PacketHash; N],
    next_idx: usize,
    direct_dups: u32,
    flood_dups: u32,
}

impl<const N: usize> Default for PacketDedup<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> PacketDedup<N> {
    /// Create an empty dedup table.
    pub const fn new() -> Self {
        Self {
            hashes: [ZERO_HASH; N],
            next_idx: 0,
            direct_dups: 0,
            flood_dups: 0,
        }
    }

    /// Check if a packet has been seen before. If not, insert it.
    ///
    /// Returns `true` if the packet is a duplicate (already in the table).
    /// Returns `false` if this is the first time (inserts it into the table).
    pub fn has_seen(&mut self, packet: &Packet) -> bool {
        let hash = packet_hash(packet);

        // Linear scan — matches C implementation
        for stored in self.hashes.iter() {
            if *stored == hash {
                // Track stats
                let is_direct = packet
                    .route_type()
                    .map(|rt| rt.is_direct())
                    .unwrap_or(false);
                if is_direct {
                    self.direct_dups += 1;
                } else {
                    self.flood_dups += 1;
                }
                return true;
            }
        }

        // Not seen — insert at next position (ring buffer)
        self.hashes[self.next_idx] = hash;
        self.next_idx = (self.next_idx + 1) % N;
        false
    }

    /// Remove a packet's hash from the table (if present).
    ///
    /// Used when a packet needs to be re-processed (e.g., after a routing change).
    pub fn clear_entry(&mut self, packet: &Packet) {
        let hash = packet_hash(packet);
        for stored in self.hashes.iter_mut() {
            if *stored == hash {
                *stored = ZERO_HASH;
                break;
            }
        }
    }

    /// Number of duplicate direct packets detected.
    pub fn direct_dups(&self) -> u32 {
        self.direct_dups
    }

    /// Number of duplicate flood packets detected.
    pub fn flood_dups(&self) -> u32 {
        self.flood_dups
    }

    /// Reset duplicate counters.
    pub fn reset_stats(&mut self) {
        self.direct_dups = 0;
        self.flood_dups = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::{PacketHeader, PayloadVersion, RouteType};

    fn make_grp_txt(payload: &[u8]) -> Packet {
        let header_byte: u8 = PacketHeader {
            route_type: RouteType::Flood,
            payload_type: PayloadType::GrpTxt,
            version: PayloadVersion::V1,
        }
        .into();

        let mut pkt = Packet::new();
        pkt.header = header_byte;
        pkt.set_path_hash_size_and_count(1, 0);
        pkt.payload.extend_from_slice(payload).unwrap();
        pkt
    }

    #[test]
    fn first_packet_not_duplicate() {
        let mut dedup = PacketDedup::<16>::new();
        let pkt = make_grp_txt(b"hello");
        assert!(!dedup.has_seen(&pkt));
    }

    #[test]
    fn same_packet_is_duplicate() {
        let mut dedup = PacketDedup::<16>::new();
        let pkt = make_grp_txt(b"hello");
        assert!(!dedup.has_seen(&pkt));
        assert!(dedup.has_seen(&pkt));
        assert!(dedup.has_seen(&pkt));
    }

    #[test]
    fn different_payloads_not_duplicate() {
        let mut dedup = PacketDedup::<16>::new();
        let pkt1 = make_grp_txt(b"hello");
        let pkt2 = make_grp_txt(b"world");
        assert!(!dedup.has_seen(&pkt1));
        assert!(!dedup.has_seen(&pkt2));
    }

    #[test]
    fn different_types_not_duplicate() {
        let mut dedup = PacketDedup::<16>::new();

        let pkt1 = make_grp_txt(b"same payload");

        let mut pkt2 = Packet::new();
        let header_byte: u8 = PacketHeader {
            route_type: RouteType::Flood,
            payload_type: PayloadType::Advert,
            version: PayloadVersion::V1,
        }
        .into();
        pkt2.header = header_byte;
        pkt2.set_path_hash_size_and_count(1, 0);
        pkt2.payload.extend_from_slice(b"same payload").unwrap();

        assert!(!dedup.has_seen(&pkt1));
        assert!(!dedup.has_seen(&pkt2));
    }

    #[test]
    fn eviction_after_capacity() {
        let mut dedup = PacketDedup::<4>::new();

        // Fill the table with 4 unique packets
        let pkts: [Packet; 4] = core::array::from_fn(|i| {
            make_grp_txt(&[i as u8; 8])
        });

        for pkt in &pkts {
            assert!(!dedup.has_seen(pkt));
        }

        // All should be detected as duplicates
        for pkt in &pkts {
            assert!(dedup.has_seen(pkt));
        }

        // Add a 5th — this evicts the oldest (pkts[0])
        let pkt5 = make_grp_txt(b"new packet");
        assert!(!dedup.has_seen(&pkt5));

        // pkts[0] was evicted, so it's "new" again
        assert!(!dedup.has_seen(&pkts[0]));

        // pkt5 is still remembered
        assert!(dedup.has_seen(&pkt5));
    }

    #[test]
    fn clear_entry_removes_hash() {
        let mut dedup = PacketDedup::<16>::new();
        let pkt = make_grp_txt(b"clearable");

        assert!(!dedup.has_seen(&pkt));
        assert!(dedup.has_seen(&pkt)); // dup

        dedup.clear_entry(&pkt);
        assert!(!dedup.has_seen(&pkt)); // no longer a dup
    }

    #[test]
    fn stats_tracking() {
        let mut dedup = PacketDedup::<16>::new();
        let pkt = make_grp_txt(b"flood stats");

        dedup.has_seen(&pkt);
        dedup.has_seen(&pkt); // dup
        dedup.has_seen(&pkt); // dup

        assert_eq!(dedup.flood_dups(), 2);
        assert_eq!(dedup.direct_dups(), 0);

        dedup.reset_stats();
        assert_eq!(dedup.flood_dups(), 0);
    }

    #[test]
    fn wire_roundtrip_still_deduplicates() {
        let mut dedup = PacketDedup::<16>::new();
        let pkt = make_grp_txt(b"over the wire");

        assert!(!dedup.has_seen(&pkt));

        // Serialize and deserialize (simulating radio TX/RX)
        let mut buf = [0u8; 255];
        let len = pkt.write_to(&mut buf);
        let mut pkt2 = Packet::new();
        assert!(pkt2.read_from(&buf[..len]));

        // Should still be detected as a duplicate
        assert!(dedup.has_seen(&pkt2));
    }

    #[test]
    fn same_payload_different_path_is_duplicate() {
        // Path is NOT included in the hash (matching C behavior),
        // so same payload with different paths = duplicate.
        let mut dedup = PacketDedup::<16>::new();

        let mut pkt1 = make_grp_txt(b"pathtest");
        pkt1.set_path_hash_size_and_count(1, 2);
        pkt1.path.extend_from_slice(&[0xAA, 0xBB]).unwrap();

        let mut pkt2 = make_grp_txt(b"pathtest");
        pkt2.set_path_hash_size_and_count(1, 3);
        pkt2.path.extend_from_slice(&[0xCC, 0xDD, 0xEE]).unwrap();

        assert!(!dedup.has_seen(&pkt1));
        assert!(dedup.has_seen(&pkt2)); // same payload → dup
    }
}
