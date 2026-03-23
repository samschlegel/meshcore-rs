# Composable trait-based roles instead of rigid role classes

- Status: accepted
- Date: 2026-03-22

## Context and Problem Statement

MeshCore C defines three firmware roles (Repeater, Companion, Room Server) as separate class hierarchies inheriting from BaseChatMesh. Each role is a monolithic MyMesh class in its own example directory. Serial interfaces (BLE, USB, WiFi) are tied to specific roles. How should the Rust implementation structure node behaviors?

## Decision Drivers

- Serial interfaces must NOT be tied to roles
- Roles should be more fluid — a node should be able to combine capabilities freely
- Enable runtime feature composition and easier testing
- Avoid the C pattern where adding a feature requires modifying a monolithic class
- Keep embedded binary size reasonable (no unnecessary code inclusion)

## Considered Options

- Port the C class hierarchy using Rust enums for role selection
- Trait-based composable modules assembled at compile time
- Dynamic plugin system with trait objects

## Decision Outcome

Chosen option: "Trait-based composable modules assembled at compile time", because it provides maximum flexibility while maintaining zero-cost abstraction for embedded targets.

### Design

A node is assembled from independent capabilities:

```rust
// Conceptual — a repeater is:
struct RepeaterNode<R: Radio, C: Clock> {
    mesh: MeshStack<R, C>,
    forwarding: ForwardingConfig,
    neighbors: NeighborTable,
    cli: CliHandler,
}

// A companion is:
struct CompanionNode<R: Radio, C: Clock, S: SerialPort> {
    mesh: MeshStack<R, C>,
    contacts: ContactStore,
    serial: SerialProtocol<S>,  // USB, BLE, or WiFi — independent of role
}
```

Capabilities are independent modules:
- **Forwarding** — retransmit policy, path management
- **ContactStore** — contact list, message history
- **SerialProtocol** — frame encoding/decoding (KISS, Companion protocol)
- **NeighborTable** — neighbor tracking, stats
- **RoomManagement** — room message queue, client sync

### Consequences

- Good, because serial interfaces are completely decoupled from roles
- Good, because new combinations can be created without modifying existing code
- Good, because each capability can be tested in isolation
- Good, because monomorphization keeps binary size lean (only included capabilities are compiled)
- Bad, because slightly more complex initialization compared to a single class
- Bad, because trait bounds can become verbose with many generic parameters

## More Information

The C implementation's role mapping:
- Repeater (`examples/simple_repeater/MyMesh.h`) → Forwarding + NeighborTable + CLI
- Companion (`examples/companion_radio/MyMesh.h`) → ContactStore + SerialProtocol + Forwarding
- Room Server (`examples/simple_room_server/MyMesh.h`) → RoomManagement + ContactStore + Forwarding
