# meshcore-rs

## Project Overview

Rust reimplementation of [MeshCore](https://github.com/rmenke/MeshCore), a LoRa mesh networking protocol. The goal is wire-compatible with the C implementation while being idiomatic Rust — traits over virtual classes, composable modules over rigid role hierarchies, exhaustive enums over magic constants.

**Reference C implementation:** `A:\code\MeshCore`
**Target platforms:** ESP32 (esp-hal), nRF52840 (embassy-nrf)
**Radio:** SX1262 via embedded-hal SPI traits

## Goals

1. Wire-compatible with MeshCore C (same packet format, crypto, routing)
2. Support Repeater, Companion, and Room Server as **composable capabilities**, not rigid roles
3. Serial interfaces (USB, BT, WiFi) are independent of node role
4. First milestone: USB serial only
5. `#![no_std]` core crates; optional `std` feature for host testing
6. Zero-copy where possible; static allocation for embedded (no alloc by default)

## Architecture Principles

- **Layered traits:** Radio → Dispatcher → Mesh → Application behaviors
- **Composable roles:** A node is assembled from independent capabilities (Forwarding, ContactStore, SerialProtocol, RoomManagement). "Repeater" = forwarding + neighbor tracking. "Companion" = contact store + serial protocol. "Room Server" = room management + contact store. These can overlap freely.
- **Generics over dynamic dispatch:** `Dispatcher<R: Radio, C: Clock>` enables monomorphization (no vtable overhead on embedded)
- **Enums with exhaustive match** for packet types, route types, payload types
- **Newtype patterns** for domain concepts (PathHash, PublicKey, PacketHeader, etc.)
- All architecture decisions must have an ADR in `docs/decisions/` — see [ADR process](#adr-process)

## Workspace Structure

```
meshcore-rs/
  Cargo.toml                    # workspace root
  CLAUDE.md
  docs/decisions/               # ADRs (MADR format)
  crates/
    meshcore-core/              # Packet, Identity, crypto, constants (no_std)
    meshcore-radio/             # Radio trait + SX1262 driver (no_std)
    meshcore-dispatch/          # Dispatcher: queue, scheduling, duty cycle (no_std)
    meshcore-mesh/              # Mesh layer: routing, encryption, packet handling (no_std)
    meshcore-serial/            # Serial interface traits + USB impl (no_std)
    meshcore-app/               # Composable application behaviors (no_std)
  boards/
    esp32/                      # ESP32 binary crate
    nrf52840/                   # nRF52840 binary crate
  tests/                        # Integration tests, host-mode simulation
```

## Reference: MeshCore C Architecture

### Key Source Files
| C File | Rust Equivalent | Purpose |
|--------|----------------|---------|
| `src/Packet.h/cpp` | `meshcore-core` | Wire packet format, constants, serialization |
| `src/Identity.h/cpp` | `meshcore-core` | Ed25519 identity, ECDH key exchange |
| `src/Dispatcher.h/cpp` | `meshcore-dispatch` | Packet queuing, transmission scheduling |
| `src/Mesh.h/cpp` | `meshcore-mesh` | Routing logic, packet handling |
| `src/helpers/BaseChatMesh.h` | `meshcore-app` | Higher-level chat/contact features |
| `src/helpers/BaseSerialInterface.h` | `meshcore-serial` | Serial interface abstraction |

### Packet Format (v1)
```
[header(1B)][transport_codes(4B optional)][path_len(1B)][path(0-64B)][payload(0-184B)]
```

**Header byte:** `0bVVPPPPRR`
- Bits 0-1: Route type (FLOOD=0x01, DIRECT=0x02, TRANSPORT_FLOOD=0x00, TRANSPORT_DIRECT=0x03)
- Bits 2-5: Payload type (REQUEST=0x00, RESPONSE=0x01, TXT_MSG=0x02, ACK=0x03, ADVERT=0x04, GRP_TXT=0x05, GRP_DATA=0x06, ANON_REQ=0x07, PATH=0x08, TRACE=0x09, MULTIPART=0x0A, CONTROL=0x0B, RAW_CUSTOM=0x0F)
- Bits 6-7: Payload version (currently v1)

### Constants
```
MAX_PACKET_PAYLOAD = 184 bytes
MAX_PATH_SIZE      = 64 bytes
MAX_TRANS_UNIT     = 255 bytes
PATH_HASH_SIZE     = 1 byte
PUB_KEY_SIZE       = 32 bytes (Ed25519)
SIGNATURE_SIZE     = 64 bytes
CIPHER_MAC_SIZE    = 2 bytes
```

### Protocol Docs
- `A:\code\MeshCore\docs\packet_format.md` — wire packet specification
- `A:\code\MeshCore\docs\payloads.md` — payload type details
- `A:\code\MeshCore\docs\kiss_modem_protocol.md` — KISS protocol specification
- `A:\code\MeshCore\docs\companion_protocol.md` — BLE companion protocol

## Workflow

### ADR Process
- All architecture decisions recorded in `docs/decisions/` using [MADR](https://adr.github.io/madr/) format
- File naming: `NNNN-short-title.md` (e.g., `0001-workspace-structure.md`)
- Template: `docs/decisions/template.md`
- ADRs are **required** before: adding a new crate, choosing a key dependency, changing module boundaries, deviating from C implementation behavior

### Task Tracking
- Obsidian vault at `vault/meshcore-rs/` for task and milestone tracking
- Tasks use Obsidian-native checkbox format: `- [ ] Task description #tag`
- Manage via CLI: `obsidian tasks todo`, `obsidian task done path=X line=N`
- Vault name for CLI commands: `meshcore-rs`

### Agent Workflow
- Use `/agent-team` skill for iterative multi-step implementation work
- Create skills in `.claude/skills/<name>/SKILL.md` to teach future agents reusable procedures
- Track work progress in the Obsidian vault

### Verification Checklist
Run after every implementation change:
```bash
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace
# Cross-compile (when board crates exist):
# cargo build --target xtensa-esp32s3-none-elf   # ESP32
# cargo build --target thumbv7em-none-eabihf      # nRF52840
```
- No `unsafe` without a documented `// SAFETY:` invariant
- New architecture decisions must have a corresponding ADR

## Coding Standards

- Rust 2021 edition
- `#![no_std]` for all library crates (with optional `std` feature for host testing)
- `#![deny(unsafe_code)]` by default; `unsafe` only where hardware requires it, with `// SAFETY:` comments
- Newtype patterns for domain concepts (`PathHash`, `PublicKey`, `PacketHeader`, etc.)
- Enums with exhaustive match for packet types, route types
- Traits for hardware abstraction (`Radio`, `Clock`, `Rng`, `SerialPort`)
- `heapless` collections for `no_std` (`heapless::Vec`, `heapless::FnvIndexMap`)
- `defmt` for embedded logging
- `embedded-hal` 1.0 traits for SPI/GPIO abstraction
- Document public APIs with rustdoc; include protocol references

## Key Dependencies (planned)

| Crate | Purpose | no_std |
|-------|---------|--------|
| `ed25519-dalek` | Ed25519 signing/verification | Yes |
| `x25519-dalek` | ECDH key exchange | Yes |
| `aes` (RustCrypto) | AES-128 encryption | Yes |
| `heapless` | Static collections | Yes |
| `defmt` / `defmt-rtt` | Embedded logging | Yes |
| `embedded-hal` 1.0 | Hardware abstraction traits | Yes |
| `embassy-nrf` | nRF52840 async HAL | Yes |
| `esp-hal` | ESP32 HAL | Yes |

## Platform Notes

- **ESP32:** `esp-hal` + `esp-wifi` (when WiFi needed). Target: `xtensa-esp32s3-none-elf`
- **nRF52840:** `embassy-nrf`. Target: `thumbv7em-none-eabihf`
- **SX1262:** Drive via `embedded-hal` SPI traits; owned by the radio crate
- **USB Serial:** Platform-specific USB peripheral crates (first serial target)
