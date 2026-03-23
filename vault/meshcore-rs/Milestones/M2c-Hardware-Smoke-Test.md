# M2c: Hardware Smoke Test

**Goal:** Flash firmware to real boards and validate radio TX/RX between two devices.

## Deliverables
- [ ] RAK4631 board crate — Embassy executor, SPI pin config, SX1262 init, USB serial output
- [ ] Heltec V3 board crate — Embassy executor, SPI pin config, SX1262 init, USB serial output
- [ ] TX test firmware — transmit a known packet every 2 seconds, print to USB serial
- [ ] RX test firmware — listen for packets, print received data + RSSI/SNR to USB serial
- [ ] Two-board smoke test — TX on one board, RX on the other, confirm data matches

## Dependencies
- [[M2-Radio-Traits]] (Radio trait)
- [[M2b-SX1262-Driver]] (SX1262 Radio impl)
- `embassy-nrf` (RAK4631)
- `esp-hal` (Heltec V3)
- Physical hardware + USB cables

## Acceptance Criteria
- [ ] RAK4631 firmware compiles and flashes via USB
- [ ] Heltec V3 firmware compiles and flashes via USB
- [ ] TX board sends packets visible on RX board's serial output
- [ ] RSSI/SNR values are reasonable (not all zeros)
- [ ] Both boards can act as either TX or RX
