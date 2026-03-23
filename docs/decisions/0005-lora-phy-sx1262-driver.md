# Use lora-phy crate for SX1262 radio driver

- Status: accepted
- Date: 2026-03-22

## Context and Problem Statement

meshcore-rs needs an SX1262 LoRa radio driver that works with embedded-hal 1.0 async traits and Embassy. Should we write a driver from scratch, or wrap an existing crate behind our `Radio` trait?

## Decision Drivers

- **Avoid reinventing SX1262 register-level control** — complex, error-prone, already solved
- **Embassy async compatibility** — must support interrupt-driven async send/recv
- **embedded-hal 1.0 + embedded-hal-async** — our target HAL abstraction
- **nRF52840 + ESP32-S3 support** — must work on both platforms
- **Active maintenance** — LoRa radio drivers need ongoing fixes for edge cases

## Considered Options

### lora-phy (lora-rs organization)
- v3.0+, active (Jan 2026), 427+ stars
- Full embedded-hal-async support, DIO1 interrupt handling built-in
- nRF52840+SX1262 examples exist
- High-level LoRa PHY abstraction over multiple chip families
- MIT/Apache-2.0

### sx1262 (BroderickCarlin)
- v0.3.0, active (Oct 2025), 9 stars
- embedded-hal 1.0-alpha (not stable 1.0)
- Register-level access, no built-in async recv
- Would need significant integration work for Embassy async

### sx126x (tweedegolf)
- v0.3.0, dormant (Jul 2024), 28 stars
- embedded-hal 1.0 but synchronous only
- No async support, minimal abstraction

### Write from scratch
- Full control, no external dependency
- Months of work replicating what lora-phy already provides
- High risk of subtle radio timing bugs

## Decision Outcome

Chosen option: **lora-phy**, because it is the only crate with production-quality Embassy async integration, DIO1 interrupt-driven receive, and existing nRF52840+SX1262 examples. It is actively maintained by the lora-rs organization (successor to embassy-rs/lora-phy).

### Integration approach

Our `meshcore-radio::Radio` trait wraps lora-phy's API:

```rust
// In meshcore-radio (or a meshcore-radio-sx1262 sub-crate)
pub struct Sx1262Radio<SPI, IV, RESET, BUSY, DIO1> {
    lora: LoRa<Sx1262<SPI, IV>, RESET, BUSY, DIO1>,
    config: RadioConfig,
}

impl<...> Radio for Sx1262Radio<...> {
    async fn send(&mut self, data: &[u8]) -> Result<(), RadioError> {
        // Delegate to lora-phy's tx() method
    }
    async fn recv(&mut self, buf: &mut [u8; 255]) -> Result<RecvResult, RadioError> {
        // Delegate to lora-phy's rx() method — blocks on DIO1 interrupt
    }
}
```

### Consequences

- Good, because we get a battle-tested SX1262 driver without reinventing register access
- Good, because lora-phy handles the complex radio state machine (standby → TX → RX transitions)
- Good, because DIO1 interrupt-driven async recv is already implemented
- Good, because the same crate supports SX1261/SX1276/SX1278 if we expand radio support later
- Bad, because we depend on an external crate's API stability
- Bad, because lora-phy's abstraction may not expose all SX1262 features we need (can use raw register access as escape hatch)

## More Information

- lora-phy repo: https://github.com/lora-rs/lora-rs
- nRF52840+SX1262 example in lora-phy test suite
- MeshCore C uses direct SX1262 register access via CustomSX1262.h (RadioLib wrapper)
