# M2c: Hardware Smoke Test

**Goal:** Validate radio TX and RX on the RAK4631 board, including interop with existing MeshCore devices.

## Deliverables
- [x] RAK4631 board crate — Embassy executor, LED checkpoints, SPI + SX1262 init via lora-phy
- [x] TX test firmware — transmit packets on 910.525 MHz, confirmed with SDR
- [x] RX test firmware — continuous RX with USB CDC serial output, hex/ASCII dump, RSSI/SNR
- [x] MeshCore packet parsing — meshcore-core Packet::read_from() validated against live traffic
- [ ] TX/RX interop — send packets that a MeshCore device can receive, and vice versa

## Dependencies
- [[M2-Radio-Traits]] (Radio trait)
- [[M2b-SX1262-Driver]] (SX1262 Radio impl)
- `embassy-nrf` (RAK4631)
- Physical hardware: RAK4631 + RAK19007, MeshCore devices, SDR for debugging

## Acceptance Criteria
- [x] RAK4631 firmware compiles and flashes via UF2
- [x] TX confirmed on air with SDR
- [x] RX firmware receives packets from MeshCore devices
- [x] Packet parsing works against real MeshCore ADVERT packets
- [ ] Bidirectional TX/RX with at least one MeshCore device
