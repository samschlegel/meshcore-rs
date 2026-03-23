# M2: Radio Abstraction

**Goal:** Define async Radio trait and supporting hardware traits per ADR-0004 (Embassy async execution model). Include a hardware smoke test against real boards.

## Deliverables
- [ ] `Radio` async trait — `async fn send(&mut self, data: &[u8])`, `async fn recv(&mut self, buf: &mut [u8]) -> RecvResult`, plus airtime estimation, RSSI/SNR, CAD
- [ ] `RadioConfig` struct — frequency, bandwidth, spreading factor, coding rate, TX power
- [ ] `Rng` trait — `fn random(&mut self, dest: &mut [u8])` (sync — no timing needed)
- [ ] `RtcClock` trait — `fn get_time(&self) -> u32`, `fn set_time(&mut self, epoch: u32)` for wall-clock time (advertisements, timestamps). Monotonic time uses `embassy_time::Instant` directly.
- [ ] Mock implementations for host testing (MockRadio, MockRng, MockRtcClock)
- [ ] SX1262 driver for RAK4631 (nRF52840 + SX1262 via SPI, known pin mapping)
- [ ] Hardware smoke test: TX a packet on one board, RX on the other (RAK4631 ↔ Heltec V3)

## Target Hardware
- **RAK4631**: nRF52840 + SX1262 (WisBlock Core). USB dev board. Primary target.
- **Heltec V3**: ESP32-S3 + SX1262 (WiFi LoRa 32 V3). USB dev board. Secondary target.

Both have integrated SX1262 with known pin mappings — no external wiring needed.

## Changes from initial plan
- **Async radio**: ADR-0004 requires async traits. C's split-phase `startSendRaw`/`isSendComplete` becomes a single `async fn send`.
- **No Clock trait**: `MillisecondClock` is replaced by `embassy_time::Instant::now()`. Only `RtcClock` (wall-clock) remains as a custom trait.
- **RadioConfig struct**: Extracted from C's build-time `#define` constants into a runtime configuration.
- **Real hardware target**: SX1262 driver is no longer a stub — RAK4631 is the first concrete implementation, with Heltec V3 as cross-platform validation.

## Dependencies
- [[M1-Core-Types]] (Packet type, constants)
- `embassy-time` (monotonic timers)
- `embassy-nrf` (RAK4631 HAL)
- `esp-hal` (Heltec V3 HAL)
- `embedded-hal` (SPI traits for SX1262)

## Acceptance Criteria
- [ ] MockRadio send/recv round-trip test passes using `embassy_futures::block_on`
- [ ] Traits compile for both host and `thumbv7em-none-eabihf` target
- [ ] RadioConfig covers LoRa parameters: freq, BW, SF, CR, TX power
- [ ] `cargo test -p meshcore-radio` passes
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] RAK4631 binary flashes and configures SX1262 over SPI
- [ ] Two-board TX/RX smoke test: send bytes on one board, receive on the other

## C Reference
- `Dispatcher.h` Radio class: `recvRaw`, `startSendRaw`, `isSendComplete`, `getEstAirtimeFor`, `getLastRSSI/SNR`
- `MeshCore.h` RTCClock: `getCurrentTime`, `setCurrentTime`, `getCurrentTimeUnique`
- `Utils.h` RNG: `random(dest, sz)`, `nextInt(min, max)`
- Default LoRa config: 869.618 MHz, BW 62.5 kHz, SF 8, CR 5, TX 22 dBm
