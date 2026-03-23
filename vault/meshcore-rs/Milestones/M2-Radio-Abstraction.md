# M2: Radio Abstraction

**Goal:** Define the Radio trait and supporting hardware traits.

## Deliverables
- [ ] Radio trait (send, recv, set_frequency, set_tx_power, etc.)
- [ ] Clock trait (millis, RTC time)
- [ ] Rng trait (random bytes)
- [ ] Mock implementations for host testing
- [ ] SX1262 driver stub using embedded-hal SPI

## Dependencies
- [[M1-Core-Types]] (Packet type)

## Acceptance Criteria
- Mock radio passes basic send/recv test
- Traits compile for both host and embedded targets
