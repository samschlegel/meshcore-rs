# M2: Radio Traits

**Goal:** Define async Radio trait and supporting hardware abstraction traits per ADR-0004. Provide mock implementations for host testing.

## Deliverables
- [ ] `Radio` async trait — `async fn send(&mut self, data: &[u8])`, `async fn recv(&mut self, buf: &mut [u8]) -> RecvResult`, plus airtime estimation, RSSI/SNR, CAD
- [ ] `RadioConfig` struct — frequency, bandwidth, spreading factor, coding rate, TX power
- [ ] `Rng` trait — `fn random(&mut self, dest: &mut [u8])` (sync — no timing needed)
- [ ] `RtcClock` trait — `fn get_time(&self) -> u32`, `fn set_time(&mut self, epoch: u32)` for wall-clock time (advertisements, timestamps). Monotonic time uses `embassy_time::Instant` directly.
- [ ] Mock implementations for host testing (MockRadio, MockRng, MockRtcClock)

## Dependencies
- [[M1-Core-Types]] (Packet type, constants) — done
- `embassy-time` (monotonic timers)

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
