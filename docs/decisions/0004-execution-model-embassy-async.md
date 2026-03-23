# Embassy async execution model for firmware event loop

- Status: accepted
- Date: 2026-03-22

## Context and Problem Statement

meshcore-rs needs an execution model that drives the radio, serial interfaces, timers, and mesh logic concurrently. The C implementation uses a single-threaded superloop with polling, which wastes power and couples all subsystems to the poll cadence. How should the Rust firmware orchestrate concurrent tasks — particularly on power-sensitive nRF52840 boards — while keeping core mesh logic portable and testable?

## Decision Drivers

- **Power efficiency on nRF52840** is the primary deployment target; the MCU should sleep between events
- **Cross-platform**: must work on both nRF52840 (embassy-nrf) and ESP32 (esp-hal)
- **Zephyr-like event primitives**: channels, signals, timers, deferred work — not bare polling
- **Testability**: core mesh logic should be testable on host without an async runtime
- **`#![no_std]`**: no heap allocation in the core crates
- **Idiomatic Rust**: leverage the async/await ecosystem rather than C-style callbacks

## Considered Options

### Option A: Synchronous core + Embassy async at board layer only

Core crates are fully synchronous — method calls in, action values out. Board crates use Embassy tasks to drive the event loop and interpret returned actions.

Effectively a lightweight actor model: each Embassy task is an "actor shell" that owns a sync state machine, receives events, and dispatches returned commands. The sync core is trivially testable but can't express timing inline — delays must be returned as `Action::ScheduleWake(Duration)` commands for the board crate to execute.

**Pros:** Trivially testable, no Embassy coupling in core, portable to other executors.
**Cons:** Action/command boilerplate, board crates carry orchestration complexity, fights the natural async model.

### Option B: Async traits throughout dispatch/mesh/radio/serial

All crates above `meshcore-core` use `async fn` in traits. The Radio trait, Dispatcher, and Mesh layer are all async-aware, using `embassy-sync` channels and `embassy-time` timers directly. Timing operations read as natural inline `await` expressions.

**Pros:** Idiomatic Embassy, natural control flow for timing, less boilerplate, matches nRF/ESP ecosystem conventions.
**Cons:** Core crates coupled to embassy-time/embassy-sync, testing needs async executor, less portable to non-Embassy runtimes.

### Option C: Sync core with async adapter crate

A `meshcore-async` adapter wraps sync core logic in Embassy tasks. Board crates use the adapter.

**Pros:** Core stays sync and testable, async isolated.
**Cons:** Extra crate, adapter risks becoming a "god crate", same action/command indirection as Option A.

## Decision Outcome

Chosen option: **Option B — async traits throughout the stack**, because it is the idiomatic Embassy approach, provides natural expression of timing-dependent mesh operations, and the portability concern is not justified given the target platforms.

### Architecture boundary

```
Sync:   meshcore-core          — Packet, Identity, crypto (pure data, no timing)
Async:  meshcore-radio         — Radio trait with async send/recv
Async:  meshcore-dispatch      — Packet queue, TX scheduling, duty cycle (embassy-time)
Async:  meshcore-mesh          — Routing, encryption, packet handling (embassy-sync channels)
Async:  meshcore-serial        — Serial traits with async read/write
Async:  meshcore-app           — Composable behaviors (forwarding, contacts, rooms)
Thin:   boards/esp32, nrf52840 — Hardware init + task spawning
```

`meshcore-core` stays synchronous — it has no timing needs and benefits from trivial testability. All other crates use Embassy async primitives natively.

### Key design decisions within Option B

- **Radio trait**: `async fn send(&mut self, packet: &Packet)`, `async fn recv(&mut self) -> Packet` — wakes on hardware interrupt
- **Dispatcher**: owns TX queue as `embassy_sync::channel::Channel`, schedules via `embassy_time::Timer`
- **Mesh layer**: receives packets from Dispatcher via channel, sends responses back
- **Testing**: use `embassy_executor::Spawner` in test mode or `embassy_futures::block_on` for host tests; `meshcore-core` tests remain plain `#[test]`

### Consequences

- Good, because timing logic (duty cycle, retransmission backoff, CAD) reads naturally as `Timer::after(delay).await`
- Good, because nRF52840 sleeps between events via Embassy's hardware-backed executor — maximizes power efficiency
- Good, because the architecture matches how Embassy examples and real firmware are structured
- Good, because `async fn` in traits is stable Rust — no macros or workarounds needed
- Neutral, because ESP32 Embassy support is less mature but actively developing and sufficient
- Bad, because host testing of async crates requires an executor (solvable with `embassy_futures::block_on`)
- Bad, because porting to a non-Embassy runtime (RTIC, bare-metal) would require rewriting async trait impls

## More Information

### Zephyr → Embassy primitive mapping

| Zephyr | Embassy | Notes |
|--------|---------|-------|
| `k_thread` | `#[embassy_executor::task]` | Cooperative, not preemptive |
| `k_msgq` / `k_fifo` | `embassy_sync::channel::Channel` | Bounded, static allocation |
| `k_event` | `embassy_sync::signal::Signal` | Single-value signaling |
| `k_timer` | `embassy_time::Timer` | Hardware-backed on nRF52 |
| `k_sem` / `k_mutex` | `embassy_sync::mutex::Mutex` | Async-aware, no priority inversion |
| `k_work_submit` | Task spawning / `Signal::signal()` | Deferred execution |

### MeshCore C timing requirements

From the C Dispatcher, these operations involve timing:
- **TX scheduling**: delay between transmissions (duty cycle)
- **CAD (Channel Activity Detection)**: listen-before-talk with timeout
- **Retransmission**: re-queue failed sends after backoff
- **Neighbor expiry**: periodic cleanup of stale neighbor entries
- **Advertisement interval**: periodic self-advertisement

All of these are "do X after Y milliseconds" patterns that read naturally as `Timer::after(duration).await` in Option B.

### Why Option A was not chosen

Option A is effectively a lightweight actor model — and a good one. The sync core with action/command returns is clean and testable. However, MeshCore's Dispatcher has significant timing logic (5+ timing-dependent operations), and expressing these as returned action values creates boilerplate that fights the async model Embassy provides natively. Since nRF52840 with Embassy is the primary target, the portability benefit of Option A does not justify the ergonomic cost.
