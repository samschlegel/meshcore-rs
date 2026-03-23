// Protocol constants matching the MeshCore C implementation.
// Reference: MeshCore.h, Packet.h

// Key and signature sizes
pub const PUB_KEY_SIZE: usize = 32;
pub const PRV_KEY_SIZE: usize = 64;
pub const SEED_SIZE: usize = 32;
pub const SIGNATURE_SIZE: usize = 64;

// Cipher constants
pub const CIPHER_KEY_SIZE: usize = 16;
pub const CIPHER_BLOCK_SIZE: usize = 16;
pub const CIPHER_MAC_SIZE: usize = 2;

// Path and hash sizes
pub const PATH_HASH_SIZE: usize = 1;
pub const MAX_HASH_SIZE: usize = 8;

// Packet sizes
pub const MAX_PACKET_PAYLOAD: usize = 184;
pub const MAX_PATH_SIZE: usize = 64;
pub const MAX_TRANS_UNIT: usize = 255;

// Advertisement
pub const MAX_ADVERT_DATA_SIZE: usize = 32;

// Header bit layout
pub const PH_ROUTE_MASK: u8 = 0x03;
pub const PH_TYPE_SHIFT: u8 = 2;
pub const PH_TYPE_MASK: u8 = 0x0F;
pub const PH_VER_SHIFT: u8 = 6;
pub const PH_VER_MASK: u8 = 0x03;
