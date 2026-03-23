# M2: Radio Traits

**Goal:** Define async Radio trait and supporting hardware abstraction traits per ADR-0004. Provide mock implementations for host testing.

**Status:** Complete

## Deliverables
- [x] `Radio` async trait — `async fn send(&mut self, data: &[u8])`, `async fn recv(&mut self, buf: &mut [u8]) -> RecvResult`, plus airtime estimation, RSSI/SNR, CAD
- [x] `RadioConfig` struct — frequency, bandwidth, spreading factor, coding rate, TX power
- [x] `Rng` trait — `fn random(&mut self, dest: &mut [u8])` (sync — no timing needed)
- [x] `RtcClock` trait — `fn get_time(&self) -> u32`, `fn set_time(&mut self, epoch: u32)` for wall-clock time
- [x] Mock implementations for host testing (MockRadio, MockRng, MockRtcClock)

## Acceptance Criteria
- [x] MockRadio send/recv round-trip test passes using `embassy_futures::block_on` (13 tests)
- [x] RadioConfig covers LoRa parameters: freq, BW, SF, CR, TX power
- [x] `cargo test -p meshcore-radio` passes
- [x] `cargo clippy --workspace -- -D warnings` clean
