# C → Rust Reference Map

| C Class/File | Rust Crate | Notes |
|-------------|-----------|-------|
| `Packet.h/cpp` | `meshcore-core` | Packet struct, header enums, constants |
| `Identity.h/cpp` | `meshcore-core` | Ed25519 identity, ECDH |
| `Utils.h/cpp` | `meshcore-core` | Crypto helpers |
| `Dispatcher.h/cpp` | `meshcore-dispatch` | Also defines Radio/Clock/PacketManager traits |
| `Mesh.h/cpp` | `meshcore-mesh` | Routing, dedup, flood/direct |
| `BaseChatMesh.h` | `meshcore-app` | Contact management, message handling |
| `BaseSerialInterface.h` | `meshcore-serial` | Serial trait |
| `ArduinoSerialInterface.h` | `meshcore-serial` | USB serial impl |
| `SerialBLEInterface.h` | `meshcore-serial` | BLE serial (future) |
| `SerialWifiInterface.h` | `meshcore-serial` | WiFi serial (future) |
| `examples/simple_repeater/MyMesh.h` | `meshcore-app` | Forwarding + NeighborTable |
| `examples/companion_radio/MyMesh.h` | `meshcore-app` | ContactStore + SerialProtocol |
| `examples/simple_room_server/MyMesh.h` | `meshcore-app` | RoomManagement |

## Key Differences from C
- **No class inheritance** — composable traits instead
- **No virtual dispatch** — generics with monomorphization
- **No dynamic allocation** — heapless collections
- **Serial is role-independent** — any node can use any interface
