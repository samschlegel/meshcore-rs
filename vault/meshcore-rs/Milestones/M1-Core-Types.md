# M1: Core Types

**Goal:** Implement foundational types in `meshcore-core` that all other crates depend on.

## Deliverables
- [ ] Packet struct with zero-copy accessors
- [ ] PacketHeader parsing (RouteType, PayloadType, PayloadVersion enums)
- [ ] Constants matching MeshCore C (MAX_PACKET_PAYLOAD, MAX_PATH_SIZE, etc.)
- [ ] Identity / LocalIdentity types (Ed25519)
- [ ] ECDH shared secret computation
- [ ] AES-128 encryption/decryption helpers
- [ ] 2-byte MAC (truncated HMAC-SHA256)

## Dependencies
None — this is the foundation crate.

## Acceptance Criteria
- `cargo test -p meshcore-core` passes
- Packet round-trip: serialize → deserialize preserves all fields
- Crypto operations produce output compatible with MeshCore C
