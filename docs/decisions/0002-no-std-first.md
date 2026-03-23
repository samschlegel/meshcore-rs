# Default to no_std with optional std feature

- Status: accepted
- Date: 2026-03-22

## Context and Problem Statement

meshcore-rs targets constrained embedded platforms (ESP32, nRF52840) with limited memory. However, we also want to run tests and simulations on the host. How should we handle the std/no_std split?

## Decision Drivers

- Must run on embedded targets with no allocator by default
- Need full test suites runnable on host (x86_64)
- Prevent accidental std dependencies from creeping into core crates
- Minimize conditional compilation complexity

## Considered Options

- `std` by default, `no_std` feature for embedded
- `no_std` by default, `std` feature for host testing
- Separate crate trees for std and no_std

## Decision Outcome

Chosen option: "`no_std` by default, `std` feature for host testing", because it ensures embedded compatibility from day one and makes std usage an explicit opt-in.

### Implementation

All library crates use:
```rust
#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;
```

Collections use `heapless` (static capacity). Logging uses `defmt`. Error types implement `core::fmt::Debug` (and `std::error::Error` when `std` feature is active).

### Consequences

- Good, because embedded compatibility is guaranteed by default
- Good, because `std` feature gate allows host testing without conditional compilation noise
- Good, because accidental `std` usage causes immediate compile errors
- Bad, because some common patterns (String, Vec, HashMap) require heapless alternatives
- Bad, because test utilities may need `std` feature, adding some Cargo.toml complexity
