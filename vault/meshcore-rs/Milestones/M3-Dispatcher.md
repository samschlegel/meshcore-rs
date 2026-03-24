# M3: Dispatcher

**Status: COMPLETE**

**Goal:** Implement packet queuing, transmission scheduling, and duty cycle management.

## Deliverables
- [x] TxQueue — priority queue with scheduled send times (heapless::Vec, O(N) linear scan)
- [x] RxDelayQueue — SNR-based delay queue for flood packets (pow10 approx, no libm)
- [x] DutyCycleTracker — TX budget management with rolling window (matches C math)
- [x] Dispatcher async run loop — select! over radio RX, TX channel, timers
- [x] CAD (Channel Activity Detection) with random backoff and timeout

## Design Decisions
- ADR-0006: select-based event loop with channels (not C polling model)
- Packets by value in channels (no pool needed — Rust ownership handles lifecycle)
- Dispatcher exclusively owns Radio (enforced by Rust type system)
- DutyCycleTracker, TxQueue, RxDelayQueue are sync data structures, independently unit-testable

## Key Constants (matching C)
- Duty cycle: 50% TX / 50% RX (factor=1.0), 1-hour rolling window
- CAD retry: 120-480 ms (randomized), 4s timeout
- RX delay: up to 32s for weak signals, formula: `(10^(0.85 - snr/4) - 1) * airtime`
- Min TX budget reserve: 100 ms

## Acceptance Criteria
- [x] 23 unit tests passing (9 duty cycle + 7 rx_delay + 7 tx_queue)
- [x] `cargo clippy -p meshcore-dispatch -- -D warnings` clean
- [x] Dispatcher compiles with MockRadio + MockRng
- [x] All 85 workspace tests pass

## Dependencies
- [[M1-Core-Types]], [[M2-Radio-Traits]]
