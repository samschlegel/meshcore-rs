//! RX delay queue for flood packet scheduling.
//!
//! When a flood packet is received with a weak signal, it is delayed before
//! delivery to give stronger copies (fewer hops) time to arrive first. This
//! naturally suppresses duplicates from longer paths.
//!
//! The delay formula matches the C MeshCore `Dispatcher::calcRxDelay`:
//! ```text
//! score = snr / 4.0
//! delay_ms = (10^(0.85 - score) - 1) * airtime_ms
//! ```

use crate::types::{DispatcherConfig, RxPacket};
use embassy_time::Instant;

/// Internal entry in the RX delay queue.
struct RxDelayEntry {
    rx_packet: RxPacket,
    deliver_at: Instant,
}

/// Approximate `10^x` without libm, using `e^(x * ln(10))` via Taylor series.
///
/// Uses range reduction (`e^t = e^n * e^f` where `n` is integer and `|f| < 1`)
/// combined with a 12-term Taylor expansion for `e^f`. Accurate enough for
/// delay scheduling (not crypto).
fn pow10_approx(x: f32) -> f32 {
    const LN10: f32 = core::f32::consts::LN_10;
    let t = x * LN10;

    if t < -20.0 {
        return 0.0;
    }
    if t > 20.0 {
        return f32::MAX;
    }

    // Range reduction: split t into integer n and fractional f.
    let n = if t >= 0.0 {
        t as i32
    } else {
        let trunc = t as i32;
        if (trunc as f32) > t { trunc - 1 } else { trunc }
    };
    let f = t - n as f32;

    // Compute e^f via Taylor series (|f| < 1.0, 12 terms converge well).
    let mut result: f32 = 1.0;
    let mut term: f32 = 1.0;
    for i in 1..=12 {
        term *= f / (i as f32);
        result += term;
    }

    // Compute e^n by repeated squaring.
    const E: f32 = core::f32::consts::E;
    let mut en: f32 = 1.0;
    let mut base = if n >= 0 { E } else { 1.0 / E };
    let mut exp = if n >= 0 { n as u32 } else { (-n) as u32 };
    while exp > 0 {
        if exp & 1 != 0 {
            en *= base;
        }
        base *= base;
        exp >>= 1;
    }

    en * result
}

/// Calculate the RX delay for a flood packet based on SNR and airtime.
///
/// Returns `Some(delay_ms)` if the packet should be delayed, `None` if it should
/// be delivered immediately (delay below threshold).
///
/// Formula matches C `Dispatcher::calcRxDelay`:
/// ```text
/// score = snr / 4.0
/// delay = (10^(0.85 - score) - 1) * airtime_ms
/// ```
/// Capped at `config.max_rx_delay_ms`. If the computed delay is below
/// `config.min_rx_delay_threshold_ms`, returns `None` (deliver immediately).
pub fn calc_rx_delay(snr: f32, airtime_ms: u32, config: &DispatcherConfig) -> Option<u32> {
    let score = snr / 4.0;
    let raw = (pow10_approx(0.85 - score) - 1.0) * airtime_ms as f32;

    if raw < 0.0 || (raw as u32) < config.min_rx_delay_threshold_ms {
        return None;
    }

    let delay_ms = if raw as u32 > config.max_rx_delay_ms {
        config.max_rx_delay_ms
    } else {
        raw as u32
    };

    Some(delay_ms)
}

/// A bounded queue for delayed RX packets.
///
/// Packets are inserted with a `deliver_at` timestamp and retrieved when that
/// time has passed. The queue is backed by a `heapless::Vec` with capacity `N`.
pub struct RxDelayQueue<const N: usize> {
    entries: heapless::Vec<RxDelayEntry, N>,
}

impl<const N: usize> Default for RxDelayQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> RxDelayQueue<N> {
    /// Create a new empty delay queue.
    pub fn new() -> Self {
        Self {
            entries: heapless::Vec::new(),
        }
    }

    /// Push a delayed RX packet into the queue.
    ///
    /// `deliver_at` is the absolute time at which the packet should be delivered.
    /// Returns `Err(rx_packet)` if the queue is full.
    #[allow(clippy::result_large_err)]
    pub fn push(&mut self, rx_packet: RxPacket, deliver_at: Instant) -> Result<(), RxPacket> {
        self.entries
            .push(RxDelayEntry {
                rx_packet,
                deliver_at,
            })
            .map_err(|entry| entry.rx_packet)
    }

    /// Pop the next packet whose `deliver_at <= now`.
    ///
    /// If multiple packets are ready, returns the one with the earliest
    /// `deliver_at` timestamp.
    pub fn pop_ready(&mut self, now: Instant) -> Option<RxPacket> {
        let mut best_idx: Option<usize> = None;
        let mut best_time = Instant::MAX;

        for (i, entry) in self.entries.iter().enumerate() {
            if entry.deliver_at <= now && entry.deliver_at < best_time {
                best_time = entry.deliver_at;
                best_idx = Some(i);
            }
        }

        best_idx.map(|idx| self.entries.swap_remove(idx).rx_packet)
    }

    /// Peek at the earliest delivery time across all entries.
    ///
    /// Useful for setting a timer wakeup. Returns `None` if the queue is empty.
    pub fn next_ready_time(&self) -> Option<Instant> {
        self.entries.iter().map(|e| e.deliver_at).min()
    }

    /// Number of entries currently in the queue.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether the queue is full.
    pub fn is_full(&self) -> bool {
        self.entries.len() == N
    }

    /// Remove all entries from the queue.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meshcore_core::packet::Packet;

    fn make_rx_packet() -> RxPacket {
        RxPacket {
            packet: Packet::new(),
            rssi: -50.0,
            snr: 10.0,
        }
    }

    #[test]
    fn calc_rx_delay_strong_signal() {
        // High SNR (40.0) -> score = 10.0 -> 10^(-9.15) ≈ 0 -> delay ≈ 0 -> None
        let config = DispatcherConfig::default();
        assert!(calc_rx_delay(40.0, 100, &config).is_none());
    }

    #[test]
    fn calc_rx_delay_weak_signal() {
        // Low SNR (-4.0) -> score = -1.0 -> 10^1.85 ≈ 70.8
        // delay = (70.8 - 1) * 500 ≈ 34900 -> capped at 32000
        let config = DispatcherConfig::default();
        let delay = calc_rx_delay(-4.0, 500, &config).unwrap();
        assert_eq!(delay, 32_000);
    }

    #[test]
    fn calc_rx_delay_medium_signal() {
        // Moderate SNR (2.0) -> score = 0.5 -> 10^0.35 ≈ 2.24
        // delay = (2.24 - 1) * 200 ≈ 248 ms
        let config = DispatcherConfig::default();
        let delay = calc_rx_delay(2.0, 200, &config).unwrap();
        assert!(delay > 150 && delay < 400, "got {} ms", delay);
    }

    #[test]
    fn push_and_pop_ready() {
        let mut queue: RxDelayQueue<4> = RxDelayQueue::new();
        assert!(queue.push(make_rx_packet(), Instant::from_millis(100)).is_ok());
        assert_eq!(queue.len(), 1);

        let result = queue.pop_ready(Instant::from_millis(100));
        assert!(result.is_some());
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn pop_ready_respects_time() {
        let mut queue: RxDelayQueue<4> = RxDelayQueue::new();
        assert!(queue.push(make_rx_packet(), Instant::from_millis(200)).is_ok());

        assert!(queue.pop_ready(Instant::from_millis(100)).is_none());
        assert_eq!(queue.len(), 1);

        assert!(queue.pop_ready(Instant::from_millis(200)).is_some());
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn next_ready_time_min() {
        let mut queue: RxDelayQueue<4> = RxDelayQueue::new();
        assert!(queue.next_ready_time().is_none());

        assert!(queue.push(make_rx_packet(), Instant::from_millis(300)).is_ok());
        assert!(queue.push(make_rx_packet(), Instant::from_millis(100)).is_ok());
        assert!(queue.push(make_rx_packet(), Instant::from_millis(200)).is_ok());

        assert_eq!(queue.next_ready_time(), Some(Instant::from_millis(100)));
    }

    #[test]
    fn full_queue() {
        let mut queue: RxDelayQueue<2> = RxDelayQueue::new();
        assert!(queue.push(make_rx_packet(), Instant::from_millis(100)).is_ok());
        assert!(queue.push(make_rx_packet(), Instant::from_millis(200)).is_ok());
        assert!(queue.is_full());

        assert!(queue.push(make_rx_packet(), Instant::from_millis(300)).is_err());
        assert_eq!(queue.len(), 2);
    }
}
