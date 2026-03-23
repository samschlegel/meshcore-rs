//! Packet header types for the MeshCore wire protocol.
//!
//! The header byte encodes route type, payload type, and payload version
//! in a single byte with the layout `0bVVPPPPRR`:
//! - Bits 0-1: Route type
//! - Bits 2-5: Payload type
//! - Bits 6-7: Payload version

use crate::constants::{PH_ROUTE_MASK, PH_TYPE_MASK, PH_TYPE_SHIFT, PH_VER_MASK, PH_VER_SHIFT};

/// Raw byte value indicating "do not retransmit" in the C implementation.
///
/// This is checked at the raw byte level *before* parsing into a [`PacketHeader`],
/// since `0xFF` happens to decode as `TransportDirect + RawCustom + V4`.
pub const DO_NOT_RETRANSMIT: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur when parsing header fields from raw bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderError {
    /// The route type bits contained an invalid value.
    InvalidRouteType(u8),
    /// The payload type bits contained an invalid value.
    InvalidPayloadType(u8),
    /// The payload version bits contained an invalid value.
    InvalidPayloadVersion(u8),
}

impl core::fmt::Display for HeaderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidRouteType(v) => write!(f, "invalid route type: {:#04x}", v),
            Self::InvalidPayloadType(v) => write!(f, "invalid payload type: {:#04x}", v),
            Self::InvalidPayloadVersion(v) => write!(f, "invalid payload version: {:#04x}", v),
        }
    }
}

// ---------------------------------------------------------------------------
// RouteType
// ---------------------------------------------------------------------------

/// Packet routing strategy.
///
/// Encoded in bits 0-1 of the header byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum RouteType {
    /// Flood with transport codes (4-byte transport prefix present).
    TransportFlood = 0x00,
    /// Simple flood routing.
    Flood = 0x01,
    /// Direct (unicast) routing.
    Direct = 0x02,
    /// Direct routing with transport codes (4-byte transport prefix present).
    TransportDirect = 0x03,
}

impl TryFrom<u8> for RouteType {
    type Error = HeaderError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::TransportFlood),
            0x01 => Ok(Self::Flood),
            0x02 => Ok(Self::Direct),
            0x03 => Ok(Self::TransportDirect),
            _ => Err(HeaderError::InvalidRouteType(value)),
        }
    }
}

impl RouteType {
    /// Returns `true` if this is a flood-based route (Flood or TransportFlood).
    #[inline]
    pub fn is_flood(self) -> bool {
        matches!(self, Self::Flood | Self::TransportFlood)
    }

    /// Returns `true` if this is a direct (unicast) route.
    #[inline]
    pub fn is_direct(self) -> bool {
        matches!(self, Self::Direct | Self::TransportDirect)
    }

    /// Returns `true` if the packet carries 4-byte transport codes.
    #[inline]
    pub fn has_transport_codes(self) -> bool {
        matches!(self, Self::TransportFlood | Self::TransportDirect)
    }
}

// ---------------------------------------------------------------------------
// PayloadType
// ---------------------------------------------------------------------------

/// Type of payload carried in the packet.
///
/// Encoded in bits 2-5 of the header byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum PayloadType {
    /// Identity/capability request.
    Request = 0x00,
    /// Identity/capability response.
    Response = 0x01,
    /// Text message (encrypted point-to-point).
    TxtMsg = 0x02,
    /// Acknowledgement.
    Ack = 0x03,
    /// Node advertisement.
    Advert = 0x04,
    /// Group text message.
    GrpTxt = 0x05,
    /// Group data message.
    GrpData = 0x06,
    /// Anonymous request.
    AnonReq = 0x07,
    /// Path information.
    Path = 0x08,
    /// Route trace.
    Trace = 0x09,
    /// Multipart message fragment.
    Multipart = 0x0A,
    /// Control message.
    Control = 0x0B,
    /// Raw custom payload (application-defined).
    RawCustom = 0x0F,
}

impl TryFrom<u8> for PayloadType {
    type Error = HeaderError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Request),
            0x01 => Ok(Self::Response),
            0x02 => Ok(Self::TxtMsg),
            0x03 => Ok(Self::Ack),
            0x04 => Ok(Self::Advert),
            0x05 => Ok(Self::GrpTxt),
            0x06 => Ok(Self::GrpData),
            0x07 => Ok(Self::AnonReq),
            0x08 => Ok(Self::Path),
            0x09 => Ok(Self::Trace),
            0x0A => Ok(Self::Multipart),
            0x0B => Ok(Self::Control),
            0x0F => Ok(Self::RawCustom),
            _ => Err(HeaderError::InvalidPayloadType(value)),
        }
    }
}

// ---------------------------------------------------------------------------
// PayloadVersion
// ---------------------------------------------------------------------------

/// Payload format version.
///
/// Encoded in bits 6-7 of the header byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum PayloadVersion {
    /// Version 1 (current).
    V1 = 0x00,
    /// Version 2.
    V2 = 0x01,
    /// Version 3.
    V3 = 0x02,
    /// Version 4.
    V4 = 0x03,
}

impl TryFrom<u8> for PayloadVersion {
    type Error = HeaderError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::V1),
            0x01 => Ok(Self::V2),
            0x02 => Ok(Self::V3),
            0x03 => Ok(Self::V4),
            _ => Err(HeaderError::InvalidPayloadVersion(value)),
        }
    }
}

// ---------------------------------------------------------------------------
// PacketHeader
// ---------------------------------------------------------------------------

/// Parsed packet header byte.
///
/// The wire format is a single byte `0bVVPPPPRR`:
/// - Bits 0-1: [`RouteType`]
/// - Bits 2-5: [`PayloadType`]
/// - Bits 6-7: [`PayloadVersion`]
///
/// Before parsing, callers should check for [`DO_NOT_RETRANSMIT`] (`0xFF`)
/// at the raw byte level; it has special meaning in the C implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct PacketHeader {
    /// Routing strategy for this packet.
    pub route_type: RouteType,
    /// Type of payload carried.
    pub payload_type: PayloadType,
    /// Payload format version.
    pub version: PayloadVersion,
}

impl TryFrom<u8> for PacketHeader {
    type Error = HeaderError;

    fn try_from(byte: u8) -> Result<Self, Self::Error> {
        let route_type = RouteType::try_from(byte & PH_ROUTE_MASK)?;
        let payload_type = PayloadType::try_from((byte >> PH_TYPE_SHIFT) & PH_TYPE_MASK)?;
        let version = PayloadVersion::try_from((byte >> PH_VER_SHIFT) & PH_VER_MASK)?;
        Ok(Self {
            route_type,
            payload_type,
            version,
        })
    }
}

impl From<PacketHeader> for u8 {
    fn from(header: PacketHeader) -> u8 {
        (header.route_type as u8)
            | ((header.payload_type as u8) << PH_TYPE_SHIFT)
            | ((header.version as u8) << PH_VER_SHIFT)
    }
}

impl PacketHeader {
    /// Returns `true` if the raw header byte equals [`DO_NOT_RETRANSMIT`] (`0xFF`).
    ///
    /// This check should be performed on the raw byte *before* parsing, since
    /// `0xFF` decodes to valid enum variants (TransportDirect + RawCustom + V4).
    #[inline]
    pub fn is_do_not_retransmit(raw: u8) -> bool {
        raw == DO_NOT_RETRANSMIT
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- RouteType ----------------------------------------------------------

    #[test]
    fn route_type_round_trip() {
        for val in 0x00..=0x03u8 {
            let rt = RouteType::try_from(val).unwrap();
            assert_eq!(rt as u8, val);
        }
    }

    #[test]
    fn route_type_invalid() {
        assert!(RouteType::try_from(0x04).is_err());
        assert!(RouteType::try_from(0xFF).is_err());
    }

    #[test]
    fn route_type_is_flood() {
        assert!(RouteType::Flood.is_flood());
        assert!(RouteType::TransportFlood.is_flood());
        assert!(!RouteType::Direct.is_flood());
        assert!(!RouteType::TransportDirect.is_flood());
    }

    #[test]
    fn route_type_is_direct() {
        assert!(RouteType::Direct.is_direct());
        assert!(RouteType::TransportDirect.is_direct());
        assert!(!RouteType::Flood.is_direct());
        assert!(!RouteType::TransportFlood.is_direct());
    }

    #[test]
    fn route_type_has_transport_codes() {
        assert!(RouteType::TransportFlood.has_transport_codes());
        assert!(RouteType::TransportDirect.has_transport_codes());
        assert!(!RouteType::Flood.has_transport_codes());
        assert!(!RouteType::Direct.has_transport_codes());
    }

    // -- PayloadType --------------------------------------------------------

    #[test]
    fn payload_type_round_trip() {
        let valid = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0F,
        ];
        for &val in &valid {
            let pt = PayloadType::try_from(val).unwrap();
            assert_eq!(pt as u8, val);
        }
    }

    #[test]
    fn payload_type_reserved_values_are_invalid() {
        assert_eq!(
            PayloadType::try_from(0x0C),
            Err(HeaderError::InvalidPayloadType(0x0C))
        );
        assert_eq!(
            PayloadType::try_from(0x0D),
            Err(HeaderError::InvalidPayloadType(0x0D))
        );
        assert_eq!(
            PayloadType::try_from(0x0E),
            Err(HeaderError::InvalidPayloadType(0x0E))
        );
    }

    #[test]
    fn payload_type_out_of_range() {
        assert!(PayloadType::try_from(0x10).is_err());
    }

    // -- PayloadVersion -----------------------------------------------------

    #[test]
    fn payload_version_round_trip() {
        for val in 0x00..=0x03u8 {
            let pv = PayloadVersion::try_from(val).unwrap();
            assert_eq!(pv as u8, val);
        }
    }

    #[test]
    fn payload_version_invalid() {
        assert!(PayloadVersion::try_from(0x04).is_err());
    }

    // -- PacketHeader -------------------------------------------------------

    #[test]
    fn header_round_trip_all_combinations() {
        let route_vals = [0x00u8, 0x01, 0x02, 0x03];
        let payload_vals = [
            0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0F,
        ];
        let version_vals = [0x00u8, 0x01, 0x02, 0x03];

        for &r in &route_vals {
            for &p in &payload_vals {
                for &v in &version_vals {
                    let byte = r | (p << PH_TYPE_SHIFT) | (v << PH_VER_SHIFT);
                    let header = PacketHeader::try_from(byte).unwrap();
                    let encoded: u8 = header.into();
                    assert_eq!(
                        encoded, byte,
                        "round-trip failed for route={r:#x} payload={p:#x} ver={v:#x}"
                    );
                }
            }
        }
    }

    #[test]
    fn header_bit_layout_matches_c() {
        // Flood + TxtMsg + V1 = 0b00_0010_01 = 0x09
        let byte = 0x09u8;
        let header = PacketHeader::try_from(byte).unwrap();
        assert_eq!(header.route_type, RouteType::Flood);
        assert_eq!(header.payload_type, PayloadType::TxtMsg);
        assert_eq!(header.version, PayloadVersion::V1);
        assert_eq!(u8::from(header), 0x09);
    }

    #[test]
    fn header_direct_ack_v1() {
        // Direct + Ack + V1 = 0b00_0011_10 = 0x0E
        let byte = 0x0Eu8;
        let header = PacketHeader::try_from(byte).unwrap();
        assert_eq!(header.route_type, RouteType::Direct);
        assert_eq!(header.payload_type, PayloadType::Ack);
        assert_eq!(header.version, PayloadVersion::V1);
    }

    #[test]
    fn header_transport_flood_advert_v1() {
        // TransportFlood + Advert + V1 = 0b00_0100_00 = 0x10
        let byte = 0x10u8;
        let header = PacketHeader::try_from(byte).unwrap();
        assert_eq!(header.route_type, RouteType::TransportFlood);
        assert_eq!(header.payload_type, PayloadType::Advert);
        assert_eq!(header.version, PayloadVersion::V1);
    }

    #[test]
    fn header_invalid_payload_type_in_byte() {
        // Route=Flood(0x01), PayloadType=0x0C (reserved), Ver=V1(0x00)
        let byte = 0x01 | (0x0C << 2);
        assert_eq!(
            PacketHeader::try_from(byte),
            Err(HeaderError::InvalidPayloadType(0x0C))
        );
    }

    #[test]
    fn do_not_retransmit_constant() {
        assert!(PacketHeader::is_do_not_retransmit(0xFF));
        assert!(!PacketHeader::is_do_not_retransmit(0x00));
        assert!(!PacketHeader::is_do_not_retransmit(0xFE));
    }

    #[test]
    fn do_not_retransmit_still_parses() {
        // 0xFF decodes to TransportDirect + RawCustom + V4
        let header = PacketHeader::try_from(0xFF).unwrap();
        assert_eq!(header.route_type, RouteType::TransportDirect);
        assert_eq!(header.payload_type, PayloadType::RawCustom);
        assert_eq!(header.version, PayloadVersion::V4);
    }
}
