# M2b: SX1262 Driver

**Goal:** Implement the Radio trait for the SX1262 LoRa transceiver via embedded-hal SPI.

## Deliverables
- [ ] Evaluate existing Rust SX1262 crates (lora-phy, sx126x-rs, etc.) for suitability
- [ ] SX1262 struct implementing the Radio async trait
- [ ] SPI + GPIO pin abstraction via embedded-hal 1.0 traits
- [ ] LoRa configuration (frequency, BW, SF, CR, TX power) via RadioConfig
- [ ] IRQ-driven async recv (DIO1 interrupt → waker)
- [ ] Basic error handling for SPI failures and radio timeouts

## Dependencies
- [[M2-Radio-Traits]] (Radio trait, RadioConfig)
- `embedded-hal` 1.0 (SPI, GPIO traits)
- `embedded-hal-async` (async SPI)

## Acceptance Criteria
- [ ] Compiles for `thumbv7em-none-eabihf` (nRF52840) and `xtensa-esp32s3-none-elf` (ESP32-S3)
- [ ] Unit tests with mock SPI pass on host
- [ ] ADR written if choosing to wrap an existing crate vs writing from scratch

## Target Hardware
- **RAK4631**: nRF52840 + SX1262 (known SPI pins)
- **Heltec V3**: ESP32-S3 + SX1262 (known SPI pins)
