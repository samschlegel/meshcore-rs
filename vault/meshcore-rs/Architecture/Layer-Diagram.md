# Architecture Layers

```
┌─────────────────────────────────────────┐
│           Application Layer             │
│  (Forwarding, ContactStore, RoomMgmt)   │
│              meshcore-app               │
├─────────────────────────────────────────┤
│             Mesh Layer                  │
│  (Routing, Encryption, Deduplication)   │
│             meshcore-mesh               │
├─────────────────────────────────────────┤
│           Dispatcher Layer              │
│  (Queue, Scheduling, Duty Cycle)        │
│           meshcore-dispatch             │
├──────────────────┬──────────────────────┤
│   Radio Layer    │   Serial Layer       │
│  (SX1262, LoRa)  │  (USB, BLE, WiFi)    │
│  meshcore-radio  │  meshcore-serial     │
├──────────────────┴──────────────────────┤
│           Core Layer                    │
│  (Packet, Identity, Crypto, Constants)  │
│           meshcore-core                 │
└─────────────────────────────────────────┘
```

Serial interfaces are **independent** of the radio/mesh stack. Any node role can use any serial interface.
