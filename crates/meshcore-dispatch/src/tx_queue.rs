//! Priority-based transmission queue for the MeshCore dispatcher.
//!
//! [`TxQueue`] holds outbound packets sorted by priority and scheduled send time.
//! The dispatcher uses it to determine which packet to transmit next.

use crate::types::TxRequest;
use embassy_time::{Duration, Instant};
use meshcore_core::packet::Packet;

/// Internal entry in the TX queue.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct TxEntry {
    /// The packet to transmit.
    pub packet: Packet,
    /// Priority level (0 = highest).
    pub priority: u8,
    /// Earliest eligible send time.
    pub send_after: Instant,
}

/// A fixed-capacity, priority-based transmission queue.
///
/// Packets are popped in order of `(priority, send_after)` — lowest priority
/// number first, then earliest scheduled time. Only entries whose `send_after`
/// has passed are eligible for popping.
///
/// Uses a linear scan internally. This is efficient for small queue sizes
/// (N <= 16), which is the expected use case for LoRa mesh nodes.
pub struct TxQueue<const N: usize> {
    entries: heapless::Vec<TxEntry, N>,
}

impl<const N: usize> TxQueue<N> {
    /// Create a new empty transmission queue.
    pub fn new() -> Self {
        Self {
            entries: heapless::Vec::new(),
        }
    }

    /// Push a [`TxRequest`] into the queue.
    ///
    /// The `delay_ms` field is converted to an absolute `send_after` instant
    /// relative to the current time. Returns `Err(request)` if the queue is full.
    #[allow(clippy::result_large_err)]
    pub fn push(&mut self, request: TxRequest) -> Result<(), TxRequest> {
        if self.is_full() {
            return Err(request);
        }
        let send_after = Instant::now() + Duration::from_millis(request.delay_ms as u64);
        let entry = TxEntry {
            packet: request.packet,
            priority: request.priority,
            send_after,
        };
        let _ = self.entries.push(entry);
        Ok(())
    }

    /// Push a pre-built [`TxEntry`] directly (for internal use / retransmit).
    #[allow(clippy::result_large_err)]
    pub fn push_entry(&mut self, entry: TxEntry) -> Result<(), TxEntry> {
        self.entries.push(entry)
    }

    /// Pop the highest-priority entry whose `send_after <= now`.
    ///
    /// Among eligible entries, the one with the lowest `(priority, send_after)`
    /// tuple is returned. Returns `None` if no entries are ready.
    pub fn pop_ready(&mut self, now: Instant) -> Option<TxEntry> {
        let mut best_idx: Option<usize> = None;

        for (i, entry) in self.entries.iter().enumerate() {
            if entry.send_after > now {
                continue;
            }
            match best_idx {
                None => best_idx = Some(i),
                Some(bi) => {
                    let best = &self.entries[bi];
                    if (entry.priority, entry.send_after) < (best.priority, best.send_after) {
                        best_idx = Some(i);
                    }
                }
            }
        }

        best_idx.map(|idx| self.entries.swap_remove(idx))
    }

    /// Peek at the next scheduled time across all entries.
    ///
    /// Returns `Some(min(send_after))` regardless of priority. This is used
    /// by the dispatcher to set a timer wakeup. Returns `None` if the queue
    /// is empty.
    pub fn next_ready_time(&self) -> Option<Instant> {
        self.entries.iter().map(|e| e.send_after).min()
    }

    /// Returns the number of entries in the queue.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the queue contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns `true` if the queue is at capacity.
    pub fn is_full(&self) -> bool {
        self.entries.len() == N
    }

    /// Remove all entries from the queue.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl<const N: usize> Default for TxQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meshcore_core::packet::Packet;

    fn make_request(priority: u8, delay_ms: u32) -> TxRequest {
        TxRequest {
            packet: Packet::new(),
            priority,
            delay_ms,
        }
    }

    fn make_entry(priority: u8, send_after_ms: u64) -> TxEntry {
        TxEntry {
            packet: Packet::new(),
            priority,
            send_after: Instant::from_millis(send_after_ms),
        }
    }

    #[test]
    fn push_and_pop_single() {
        let mut q = TxQueue::<4>::new();
        assert!(q.entries.push(make_entry(0, 100)).is_ok());
        assert_eq!(q.len(), 1);

        let entry = q.pop_ready(Instant::from_millis(100)).unwrap();
        assert_eq!(entry.priority, 0);
        assert_eq!(entry.send_after, Instant::from_millis(100));
        assert!(q.is_empty());
    }

    #[test]
    fn priority_ordering() {
        let mut q = TxQueue::<4>::new();
        let _ = q.entries.push(make_entry(2, 0));
        let _ = q.entries.push(make_entry(0, 0));
        let _ = q.entries.push(make_entry(1, 0));

        let now = Instant::from_millis(0);
        assert_eq!(q.pop_ready(now).unwrap().priority, 0);
        assert_eq!(q.pop_ready(now).unwrap().priority, 1);
        assert_eq!(q.pop_ready(now).unwrap().priority, 2);
        assert!(q.pop_ready(now).is_none());
    }

    #[test]
    fn scheduled_ordering() {
        let mut q = TxQueue::<4>::new();
        let _ = q.entries.push(make_entry(1, 300));
        let _ = q.entries.push(make_entry(1, 100));
        let _ = q.entries.push(make_entry(1, 200));

        let now = Instant::from_millis(300);
        assert_eq!(q.pop_ready(now).unwrap().send_after, Instant::from_millis(100));
        assert_eq!(q.pop_ready(now).unwrap().send_after, Instant::from_millis(200));
        assert_eq!(q.pop_ready(now).unwrap().send_after, Instant::from_millis(300));
    }

    #[test]
    fn pop_ready_respects_time() {
        let mut q = TxQueue::<4>::new();
        let _ = q.entries.push(make_entry(0, 100));
        let _ = q.entries.push(make_entry(0, 500));

        assert!(q.pop_ready(Instant::from_millis(50)).is_none());
        assert!(q.pop_ready(Instant::from_millis(100)).is_some());
        assert!(q.pop_ready(Instant::from_millis(200)).is_none());
        assert!(q.pop_ready(Instant::from_millis(500)).is_some());
    }

    #[test]
    fn next_ready_time_returns_min() {
        let mut q = TxQueue::<4>::new();
        assert!(q.next_ready_time().is_none());

        let _ = q.entries.push(make_entry(0, 300));
        let _ = q.entries.push(make_entry(1, 100));
        let _ = q.entries.push(make_entry(2, 200));

        assert_eq!(q.next_ready_time(), Some(Instant::from_millis(100)));
    }

    #[test]
    fn full_queue_returns_err() {
        let mut q = TxQueue::<2>::new();
        let _ = q.entries.push(make_entry(0, 0));
        let _ = q.entries.push(make_entry(1, 0));
        assert!(q.is_full());

        let result = q.push(make_request(2, 0));
        assert!(result.is_err());
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn clear_empties_queue() {
        let mut q = TxQueue::<4>::new();
        let _ = q.entries.push(make_entry(0, 0));
        let _ = q.entries.push(make_entry(1, 100));
        assert_eq!(q.len(), 2);

        q.clear();
        assert!(q.is_empty());
        assert!(q.pop_ready(Instant::from_millis(1000)).is_none());
    }
}
