# Use Cargo workspace with per-layer crates

- Status: accepted
- Date: 2026-03-22

## Context and Problem Statement

meshcore-rs is a multi-platform embedded Rust project targeting ESP32 and nRF52840. The MeshCore C implementation is organized into layers (Packet/Identity, Dispatcher, Mesh, Serial, Application). How should we organize the Rust codebase to enforce clean layer boundaries while supporting multiple target platforms?

## Decision Drivers

- Enforce dependency boundaries at compile time
- Each crate can independently set `no_std`/`std` feature flags
- Enable independent compilation and testing per layer
- Support both ESP32 and nRF52840 binary targets alongside shared library crates
- Mirror C architecture layers for familiarity while improving on the design

## Considered Options

- Single crate with feature flags
- Cargo workspace with per-layer crates
- Cargo workspace with platform-split crates

## Decision Outcome

Chosen option: "Cargo workspace with per-layer crates", because it enforces layer boundaries at compile time, allows each crate to have its own dependency set, and maps naturally to the existing C architecture layers while enabling Rust-idiomatic design improvements.

### Workspace Layout

```
crates/
  meshcore-core/        # Packet, Identity, crypto, constants (no_std)
  meshcore-radio/       # Radio trait + SX1262 driver (no_std)
  meshcore-dispatch/    # Dispatcher: queue, scheduling, duty cycle (no_std)
  meshcore-mesh/        # Mesh routing, encryption, packet handling (no_std)
  meshcore-serial/      # Serial interface traits + USB impl (no_std)
  meshcore-app/         # Composable application behaviors (no_std)
boards/
  esp32/                # ESP32 binary crate
  nrf52840/             # nRF52840 binary crate
```

### Consequences

- Good, because compile-time enforcement prevents accidental cross-layer dependencies
- Good, because each crate can be tested independently on the host
- Good, because board crates can select only the features they need
- Bad, because more Cargo.toml files to maintain
- Bad, because cross-crate refactoring requires coordinated changes

## More Information

This mirrors the C implementation's layer structure:
- `Packet.h/Identity.h` → `meshcore-core`
- `Dispatcher.h` → `meshcore-dispatch`
- `Mesh.h` → `meshcore-mesh`
- `BaseSerialInterface.h` → `meshcore-serial`
- `BaseChatMesh.h` → `meshcore-app`
