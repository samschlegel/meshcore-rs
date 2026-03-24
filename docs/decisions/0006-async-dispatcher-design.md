# Async Dispatcher design: channels + select over radio and TX queue

- Status: accepted
- Date: 2026-03-23

## Context and Problem Statement

The C MeshCore Dispatcher is a polling-based state machine: its `loop()` method is called ~100 Hz and alternates between checking for received packets (`checkRecv`) and attempting to transmit queued packets (`checkSend`). It manages the radio exclusively — only one of TX or RX can happen at a time — and enforces duty cycle, CAD (Channel Activity Detection), and priority-based TX scheduling.

How should the Rust Dispatcher be structured using Embassy async primitives? The design must be power-efficient (CPU sleeps between events), composable (Mesh layer is a separate concern), and testable on host.

## Decision Drivers

- **Power efficiency**: nRF52840 must sleep between events — no busy-polling
- **Embassy idiom**: use channels, timers, and `select!` — not callback traits or polling loops
- **Composability**: Dispatcher should not know about Mesh routing; it handles radio TX/RX scheduling only
- **Exclusive radio ownership**: only the Dispatcher touches the radio — enforced by Rust ownership
- **Static allocation**: all buffers are fixed-size, no alloc
- **Testability**: DutyCycleTracker, TxQueue, and RxDelayQueue should be independently unit-testable without async
- **Wire compatibility**: same timing behavior as C (duty cycle math, RX delay formula, CAD logic)

## Considered Options

### Option A: Direct port of C polling model with Timer-based tick

Translate the C `loop()` into an async loop that sleeps for a fixed tick interval (e.g., 10 ms), then calls `check_recv()` and `check_send()` sequentially. The radio uses short RX windows with timeouts.

```rust
loop {
    Timer::after(Duration::from_millis(10)).await;
    self.check_recv().await;
    self.check_send().await;
}
```

**Pros:** Minimal design divergence from C; easy to verify behavioral equivalence.
**Cons:** Wastes power polling at fixed rate. Doesn't leverage Embassy's interrupt-driven wakers. Adds fixed latency to both RX and TX. Not how Embassy firmware is written.

### Option B: Select-based event loop with channels

The Dispatcher's main loop uses `select!` to wait on multiple event sources simultaneously: radio RX completion, TX request submission, and timer wakeups. The CPU sleeps until one of these events fires.

Inter-task communication uses Embassy channels:
- **TX submission**: Other tasks (Mesh layer) send `TxRequest` values into a `Channel`. The Dispatcher drains this channel on each iteration.
- **RX delivery**: The Dispatcher sends parsed `RxPacket` values to the Mesh layer via a separate `Channel`.

The radio is exclusively owned by the Dispatcher — no other task can access it.

```rust
loop {
    // Drain pending TX submissions into local priority queue
    while let Ok(req) = tx_in.try_receive() {
        self.tx_queue.push(req);
    }

    // Deliver any RX-delayed packets that are now ready
    self.deliver_ready_rx(rx_out).await;

    // Compute next wake time (earliest of: next TX ready, next RX delivery)
    let next_wake = self.next_wake_time();

    // Wait for: radio packet | new TX submission | scheduled wake
    match select3(
        self.radio.recv(&mut rx_buf),
        tx_in.receive(),
        sleep_until(next_wake),
    ).await {
        First(Ok(result)) => self.handle_rx(result, &rx_buf, rx_out).await,
        Second(tx_req) => self.tx_queue.push(tx_req),
        Third(_) => {} // Timer fired, re-evaluate queues at top of loop
    }

    // Attempt TX if something is ready and duty cycle allows
    self.maybe_transmit().await;
}
```

**Pros:** CPU sleeps between events. Natural Embassy pattern. Radio RX is interrupt-driven (DIO1 GPIOTE). TX requests are delivered immediately via channel signal. Timer-based scheduling for delayed TX and RX delivery.
**Cons:** Dropping a pending `recv()` future when TX fires means the radio may need re-initialization for TX. Must handle the `select!` cancellation semantics carefully.

### Option C: Separate TX and RX tasks communicating via channels

Split into two Embassy tasks: one for RX (owns the radio in RX mode), one for TX (borrows the radio for short TX bursts). Coordinate via a shared `Mutex<Radio>` or a "radio lease" pattern.

**Pros:** Clean separation of concerns.
**Cons:** Shared radio ownership is fundamentally unsafe for SPI radios — only one task can hold the SPI bus. Mutex contention adds complexity and latency. Two tasks fighting for the radio creates priority inversion risks. The radio's state machine (RX ↔ TX transitions) is inherently sequential — splitting it across tasks adds complexity without benefit.

## Decision Outcome

Chosen option: **Option B — select-based event loop with channels**, because it is the idiomatic Embassy pattern, provides power-efficient interrupt-driven wakeups, and keeps exclusive radio ownership in a single task.

### Key design elements

#### 1. Dispatcher owns the Radio exclusively

```rust
pub struct Dispatcher<R: Radio, Rng: crate::Rng, const TX_Q: usize, const RX_Q: usize> {
    radio: R,
    rng: Rng,
    config: DispatcherConfig,
    tx_queue: TxQueue<TX_Q>,
    rx_delay_queue: RxDelayQueue<RX_Q>,
    duty_cycle: DutyCycleTracker,
    stats: DispatcherStats,
}
```

The Dispatcher takes ownership of the `Radio` impl. No other code can access the radio. This is enforced at compile time by Rust's ownership system — no runtime locking needed.

#### 2. Channels for inter-task communication

```
[Mesh task] --TxRequest--> [tx Channel] ---> [Dispatcher] --RxPacket--> [rx Channel] ---> [Mesh task]
```

- `TxRequest` contains a `Packet`, priority (u8), and delay (ms).
- `RxPacket` contains a parsed `Packet` plus RSSI/SNR metadata.
- Both channels are `embassy_sync::channel::Channel` with `CriticalSectionRawMutex` and static capacity.

This replaces the C pattern where `Mesh` inherits from `Dispatcher` and overrides `onRecvPacket()`. In Rust, Mesh and Dispatcher are independent tasks connected by channels — no inheritance, no tight coupling.

#### 3. Packets by value in channels (no pool)

The C implementation uses a `StaticPoolPacketManager` to pre-allocate packets and pass them by pointer. This avoids heap allocation but requires manual lifecycle management (`alloc`/`free`).

In Rust, `Packet` is a value type (~250 bytes with heapless::Vec). Embassy channels store values inline in their static buffer. A `Channel<_, Packet, 8>` uses ~2 KB of static memory — acceptable on both nRF52840 (256 KB RAM) and ESP32 (520 KB RAM).

Passing packets by value through channels:
- Eliminates manual pool management and use-after-free risks
- Makes ownership transfer explicit (sender gives up the packet, receiver owns it)
- Leverages Embassy's proven channel implementation
- Costs a memcpy per transfer (~250 bytes), which is negligible vs radio airtime

If profiling shows the memcpy is a bottleneck (unlikely given radio speeds), a pool can be added later as an optimization behind the same channel API.

#### 4. select! for multiplexing events

The core loop uses `embassy_futures::select::select3` (or similar) to wait on:
1. **Radio RX**: `radio.recv()` blocks on DIO1 interrupt — CPU sleeps
2. **TX submission**: `channel.receive()` wakes when another task sends
3. **Timer**: `Timer::at(instant)` wakes for scheduled TX or RX delivery

When a TX request wins the select, the pending `recv()` future is dropped. This is safe because:
- lora-phy's `rx()` is cancel-safe — dropping the future leaves the radio in an indeterminate state, but the next `send()` call will reconfigure the radio for TX
- After TX completes, the next loop iteration starts a fresh `recv()` call

#### 5. Priority TX queue (sync data structure)

`TxQueue` is a synchronous heapless priority queue sorted by `(scheduled_time, priority)`. It is NOT an async primitive — it's a plain struct that the Dispatcher mutates within its single task.

```rust
pub struct TxEntry {
    pub packet: Packet,
    pub priority: u8,         // 0 = highest
    pub send_after: Instant,  // Earliest send time
}

pub struct TxQueue<const N: usize> {
    entries: heapless::Vec<TxEntry, N>,
}
```

This is intentionally simple — O(N) insert and O(N) pop-min. For typical queue depths (< 16 packets), this outperforms a heap due to cache locality and avoids the complexity of heapless::BinaryHeap's API.

#### 6. RX delay queue (sync data structure)

Flood packets with weak signals are delayed before delivery, giving stronger copies time to arrive. The delay formula matches the C implementation:

```
delay = (10^(0.85 - snr_score) - 1) * airtime_ms
```

Capped at 32 seconds. If delay < 50 ms, deliver immediately.

`RxDelayQueue` has the same structure as `TxQueue` — a sorted heapless::Vec of `(deliver_at, RxPacket)` entries.

#### 7. DutyCycleTracker (sync, independently testable)

```rust
pub struct DutyCycleTracker {
    budget_ms: u32,
    last_refill: Instant,
    window_ms: u32,
    duty_factor: f32,  // 1.0 = 50/50 TX/RX
}

impl DutyCycleTracker {
    pub fn can_transmit(&self, est_airtime_ms: u32) -> bool { ... }
    pub fn deduct(&mut self, actual_airtime_ms: u32) { ... }
    pub fn refill(&mut self, now: Instant) { ... }
}
```

Pure data + math — no async, no Embassy dependency needed. Fully unit-testable with synthetic time values.

#### 8. CAD (Channel Activity Detection)

Before transmitting, the Dispatcher calls `radio.channel_active().await`. If the channel is busy:
1. Wait `cad_retry_delay` (200 ms default, randomized 120–480 ms in Mesh config)
2. Retry up to `cad_timeout` (4 seconds)
3. If still busy after timeout, transmit anyway and log an error

```rust
async fn wait_for_clear_channel(&mut self) -> bool {
    let deadline = Instant::now() + Duration::from_millis(self.config.cad_timeout_ms as u64);
    loop {
        if !self.radio.channel_active().await.unwrap_or(false) {
            return true; // Channel clear
        }
        if Instant::now() >= deadline {
            return false; // Timeout — transmit anyway
        }
        let delay = self.rng.next_u32(
            self.config.cad_retry_min_ms,
            self.config.cad_retry_max_ms,
        );
        Timer::after(Duration::from_millis(delay as u64)).await;
    }
}
```

### What changes from C

| C Dispatcher | Rust Dispatcher | Why |
|---|---|---|
| Polling `loop()` at ~100 Hz | `select!` over radio + channel + timer | Power efficiency, Embassy idiom |
| `onRecvPacket()` virtual method | RX delivery channel | Decoupled, no inheritance |
| `PacketManager` pool with alloc/free | Packets by value in channels | Rust ownership, no lifecycle bugs |
| `Mesh` inherits `Dispatcher` | Separate tasks, connected by channels | Composable, testable independently |
| `radio.recvRaw()` polling | `radio.recv().await` on DIO1 interrupt | Interrupt-driven, zero-cost idle |
| `radio.startSendRaw()` + `isSendComplete()` | `radio.send().await` | Single async operation |
| Error flags (`ERR_EVENT_*`) | Result types + defmt logging | Idiomatic Rust error handling |

### What stays the same

- Duty cycle math (same formula, same constants)
- RX delay formula (same SNR-based calculation)
- CAD timeout logic (same thresholds)
- Priority queue semantics (lower number = higher priority)
- Wire format (handled by meshcore-core, unchanged)

### Consequences

- Good, because the CPU sleeps between radio events — maximizes battery life on nRF52840
- Good, because Dispatcher and Mesh are fully decoupled — each can be tested and developed independently
- Good, because `select!` cancellation is the standard Embassy pattern for exclusive-resource multiplexing
- Good, because DutyCycleTracker and queue types are sync and trivially unit-testable
- Good, because no manual packet lifecycle management — Rust ownership prevents use-after-free
- Neutral, because packets-by-value (~250 byte copies) costs some CPU, but is negligible vs. radio airtime
- Bad, because dropping a pending `recv()` future on TX interrupt wastes any partially-received packet — same as C behavior but worth noting
- Bad, because the single-task design means a slow TX (long airtime) blocks RX delivery — acceptable since the radio can only do one at a time anyway

## More Information

### select! cancellation safety

When `select!` picks the TX branch, the pending `radio.recv()` future is dropped. This is safe because:
1. lora-phy does not hold any locks or resources across await points
2. The next call to `radio.recv()` or `radio.send()` will reconfigure the radio state machine
3. Any packet partially received during the dropped future is lost — this is identical to the C behavior where the radio switches from RX to TX

### Capacity sizing

Recommended defaults:
- TX queue: 8 entries (~2 KB). In practice, MeshCore nodes rarely have more than 2-3 pending outbound packets.
- RX delay queue: 8 entries (~2 KB). Flood packets with weak signals are delayed up to 32 seconds, but few overlap.
- TX submission channel: 4 slots. Mesh layer shouldn't outpace the radio.
- RX delivery channel: 4 slots. If Mesh layer can't keep up, packets are dropped (same as C pool exhaustion).

### Future optimization: packet pool

If profiling shows that 250-byte memcpy through channels is a bottleneck, the channel type can be changed to carry pool handles (indices into a static array) instead of full packets. This is an internal optimization that doesn't change the Dispatcher's external API.
