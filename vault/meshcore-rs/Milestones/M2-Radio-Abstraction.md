# M2: Radio Abstraction

**Goal:** Define async Radio trait and supporting hardware traits per ADR-0004 (Embassy async execution model).

## Deliverables
- [ ] `Radio` async trait — `async fn send(&mut self, data: &[u8])`, `async fn recv(&mut self, buf: &mut [u8]) -> RecvResult`, plus airtime estimation, RSSI/SNR, CAD
- [ ] `RadioConfig` struct — frequency, bandwidth, spreading factor, coding rate, TX power
- [ ] `Rng` trait — `fn random(&mut self, dest: &mut [u8])` (sync — no timing needed)
- [ ] `RtcClock` trait — `fn get_time(&self) -> u32`, `fn set_time(&mut self, epoch: u32)` for wall-clock time (advertisements, timestamps). Monotonic time uses `embassy_time::Instant` directly.
- [ ] Mock implementations for host testing (MockRadio, MockRng, MockRtcClock)
- [ ] SX1262 driver stub — placeholder struct, to be filled in later

## Changes from initial plan
- **Async radio**: ADR-0004 requires async traits. C's split-phase `startSendRaw`/`isSendComplete` becomes a single `async fn send`.
- **No Clock trait**: `MillisecondClock` is replaced by `embassy_time::Instant::now()`. Only `RtcClock` (wall-clock) remains as a custom trait.
- **RadioConfig struct**: Extracted from C's build-time `#define` constants into a runtime configuration.

## Dependencies
- [[M1-Core-Types]] (Packet type, constants)
- `embassy-time` (monotonic timers)
- `embedded-hal` (SPI traits for SX1262 stub)

## Acceptance Criteria
- [ ] MockRadio send/recv round-trip test passes using `embassy_futures::block_on`
- [ ] Traits compile for both host and `thumbv7em-none-eabihf` target
- [ ] RadioConfig covers LoRa parameters: freq, BW, SF, CR, TX power
- [ ] `cargo test -p meshcore-radio` passes
- [ ] `cargo clippy --workspace -- -D warnings` clean

## C Reference
- `Dispatcher.h` Radio class: `recvRaw`, `startSendRaw`, `isSendComplete`, `getEstAirtimeFor`, `getLastRSSI/SNR`
- `MeshCore.h` RTCClock: `getCurrentTime`, `setCurrentTime`, `getCurrentTimeUnique`
- `Utils.h` RNG: `random(dest, sz)`, `nextInt(min, max)`
- Default LoRa config: 869.618 MHz, BW 62.5 kHz, SF 8, CR 5, TX 22 dBm
