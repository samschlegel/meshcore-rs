# M2b: SX1262 Driver

**Goal:** Implement the Radio trait for the SX1262 LoRa transceiver wrapping lora-phy.

**Status:** Complete

## Deliverables
- [x] Evaluate existing Rust SX1262 crates — chose lora-phy (ADR-0005)
- [x] `Sx1262Radio` struct implementing the Radio async trait via lora-phy
- [x] RadioConfig → lora-phy ModulationParams/PacketParams conversion
- [x] LoRa configuration (frequency, BW, SF, CR, TX power) via RadioConfig
- [x] IRQ-driven async recv (DIO1 interrupt via lora-phy's InterfaceVariant)
- [x] CAD (channel activity detection) support
- [x] Sleep/standby power management
- [x] Airtime estimation for duty cycle management
- [x] Behind `sx1262` feature flag — doesn't bloat builds that only need mocks

## Acceptance Criteria
- [x] Compiles with `--features sx1262` on host
- [x] `cargo clippy --workspace --features meshcore-radio/sx1262 -- -D warnings` clean
- [x] ADR-0005 accepted (lora-phy chosen over writing from scratch)
- [x] 62 workspace tests still pass
