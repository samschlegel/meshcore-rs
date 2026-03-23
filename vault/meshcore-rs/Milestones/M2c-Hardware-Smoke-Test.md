# M2c: Hardware Smoke Test

**Status: COMPLETE**

**Goal:** Validate radio TX and RX on the RAK4631 board, including interop with existing MeshCore devices.

## Deliverables
- [x] RAK4631 board crate — Embassy executor, LED checkpoints, SPI + SX1262 init via lora-phy
- [x] TX test firmware — transmit packets on 910.525 MHz, confirmed with SDR
- [x] RX test firmware — continuous RX with USB CDC serial output, hex/ASCII dump, RSSI/SNR
- [x] MeshCore packet parsing — meshcore-core Packet::read_from() validated against live traffic
- [x] TX/RX interop — signed ADVERT received by MeshCore devices, MeshCore packets parsed by us

## Key Findings
- MeshCore uses **private sync word** (0x1424), **16-symbol preamble**, CRC on, explicit header
- lora-phy `LoRa::new(radio, false, delay)` correctly sets private sync word
- Ed25519 signing adds only ~4KB to binary (65KB total with LTO)
- DIO1 async wait via GPIOTE — CPU sleeps between packets

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
- [x] Bidirectional TX/RX with at least one MeshCore device
