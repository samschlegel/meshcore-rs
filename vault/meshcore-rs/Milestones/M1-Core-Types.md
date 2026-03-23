# M1: Core Types

**Goal:** Implement foundational types in `meshcore-core` that all other crates depend on.

**Status:** Complete

## Deliverables
- [x] Packet struct with zero-copy accessors
- [x] PacketHeader parsing (RouteType, PayloadType, PayloadVersion enums)
- [x] Constants matching MeshCore C (MAX_PACKET_PAYLOAD, MAX_PATH_SIZE, etc.)
- [x] Identity / LocalIdentity types (Ed25519)
- [x] ECDH shared secret computation
- [x] AES-128 encryption/decryption helpers
- [x] 2-byte MAC (truncated HMAC-SHA256)

## Dependencies
None — this is the foundation crate.

## Acceptance Criteria
- [x] `cargo test -p meshcore-core` passes (49 tests)
- [x] Packet round-trip: serialize → deserialize preserves all fields
- [x] Crypto operations produce output compatible with MeshCore C
