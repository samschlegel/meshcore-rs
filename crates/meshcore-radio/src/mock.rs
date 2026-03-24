//! Mock implementations of Radio, Rng, and RtcClock traits for host testing.

use heapless::{Deque, Vec};
use meshcore_core::constants::MAX_TRANS_UNIT;

use crate::radio::{Radio, RadioConfig, RadioError, RecvResult};
use crate::rng::Rng;
use crate::rtc::RtcClock;

// ---------------------------------------------------------------------------
// MockRadio
// ---------------------------------------------------------------------------

/// A mock radio for host testing. Sent packets are queued for inspection.
/// Packets to be "received" are pre-loaded via [`push_recv`](MockRadio::push_recv).
pub struct MockRadio {
    /// Packets that were sent via `send()`.
    sent: Deque<Vec<u8, MAX_TRANS_UNIT>, 16>,
    /// Packets queued to be returned by `recv()`.
    recv_queue: Deque<(Vec<u8, MAX_TRANS_UNIT>, f32, f32), 16>,
    /// Current config (set by `configure()`).
    config: Option<RadioConfig>,
    /// Whether the radio is in sleep mode.
    sleeping: bool,
}

impl MockRadio {
    /// Create a new MockRadio with empty queues.
    pub fn new() -> Self {
        Self {
            sent: Deque::new(),
            recv_queue: Deque::new(),
            config: None,
            sleeping: false,
        }
    }

    /// Pre-load a packet to be returned by the next `recv()` call.
    pub fn push_recv(&mut self, data: &[u8], rssi: f32, snr: f32) {
        let mut v = Vec::new();
        v.extend_from_slice(data).ok();
        self.recv_queue.push_back((v, rssi, snr)).ok();
    }

    /// Retrieve the next packet that was sent via `send()`.
    pub fn pop_sent(&mut self) -> Option<Vec<u8, MAX_TRANS_UNIT>> {
        self.sent.pop_front()
    }

    /// Number of packets currently in the sent queue.
    pub fn sent_count(&self) -> usize {
        self.sent.len()
    }

    /// Whether `configure()` has been called.
    pub fn is_configured(&self) -> bool {
        self.config.is_some()
    }

    /// Whether the radio is currently in sleep mode.
    pub fn is_sleeping(&self) -> bool {
        self.sleeping
    }
}

impl Radio for MockRadio {
    async fn configure(&mut self, config: &RadioConfig) -> Result<(), RadioError> {
        self.config = Some(*config);
        self.sleeping = false;
        Ok(())
    }

    async fn send(&mut self, data: &[u8]) -> Result<(), RadioError> {
        if self.sleeping {
            return Err(RadioError::InvalidState);
        }
        let mut v = Vec::new();
        v.extend_from_slice(data).map_err(|_| RadioError::SendFailed)?;
        self.sent.push_back(v).map_err(|_| RadioError::SendFailed)?;
        Ok(())
    }

    async fn recv(&mut self, buf: &mut [u8; MAX_TRANS_UNIT]) -> Result<RecvResult, RadioError> {
        let (data, rssi, snr) = self.recv_queue.pop_front().ok_or(RadioError::RecvFailed)?;
        let len = data.len();
        buf[..len].copy_from_slice(&data);
        Ok(RecvResult { len, rssi, snr })
    }

    async fn channel_active(&mut self) -> Result<bool, RadioError> {
        Ok(false)
    }

    fn estimate_airtime_ms(&self, len: usize) -> u32 {
        len as u32 * 2
    }

    async fn sleep(&mut self) -> Result<(), RadioError> {
        self.sleeping = true;
        Ok(())
    }

    async fn standby(&mut self) -> Result<(), RadioError> {
        self.sleeping = false;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MockRng
// ---------------------------------------------------------------------------

/// A deterministic "random" number generator using a simple incrementing seed.
pub struct MockRng {
    seed: u8,
}

impl MockRng {
    /// Create a new MockRng with the given initial seed.
    pub fn new(seed: u8) -> Self {
        Self { seed }
    }
}

impl Rng for MockRng {
    fn random(&mut self, dest: &mut [u8]) {
        for byte in dest.iter_mut() {
            *byte = self.seed;
            self.seed = self.seed.wrapping_add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// MockRtcClock
// ---------------------------------------------------------------------------

/// A simple wall clock with settable time for testing.
pub struct MockRtcClock {
    time: u32,
    last_unique: u32,
}

impl MockRtcClock {
    /// Create a new MockRtcClock with the given initial time.
    pub fn new(initial_time: u32) -> Self {
        Self {
            time: initial_time,
            last_unique: 0,
        }
    }

    /// Advance the clock by the given number of seconds.
    pub fn advance(&mut self, seconds: u32) {
        self.time += seconds;
    }
}

impl RtcClock for MockRtcClock {
    fn get_time(&self) -> u32 {
        self.time
    }

    fn set_time(&mut self, epoch_secs: u32) {
        self.time = epoch_secs;
    }

    fn get_time_unique(&mut self) -> u32 {
        let t = self.get_time();
        if t <= self.last_unique {
            self.last_unique += 1;
        } else {
            self.last_unique = t;
        }
        self.last_unique
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use embassy_futures::block_on;

    // -- MockRadio tests --

    #[test]
    fn mock_radio_send_recv() {
        block_on(async {
            let mut radio = MockRadio::new();

            // Pre-load a packet to receive
            radio.push_recv(&[1, 2, 3, 4], -50.0, 10.5);

            // Receive the pre-loaded packet
            let mut buf = [0u8; MAX_TRANS_UNIT];
            let result = radio.recv(&mut buf).await.unwrap();
            assert_eq!(result.len, 4);
            assert_eq!(&buf[..4], &[1, 2, 3, 4]);
            assert_eq!(result.rssi, -50.0);
            assert_eq!(result.snr, 10.5);

            // Send a packet
            radio.send(&[10, 20, 30]).await.unwrap();
            assert_eq!(radio.sent_count(), 1);

            // Pop the sent packet and verify
            let sent = radio.pop_sent().unwrap();
            assert_eq!(sent.as_slice(), &[10, 20, 30]);
            assert_eq!(radio.sent_count(), 0);
        });
    }

    #[test]
    fn mock_radio_configure() {
        block_on(async {
            let mut radio = MockRadio::new();
            assert!(!radio.is_configured());

            let config = RadioConfig {
                frequency_mhz: 915.0,
                bandwidth_khz: 125.0,
                spreading_factor: 7,
                coding_rate: 5,
                tx_power: 14,
                ..Default::default()
            };
            radio.configure(&config).await.unwrap();
            assert!(radio.is_configured());
        });
    }

    #[test]
    fn mock_radio_sleep_standby() {
        block_on(async {
            let mut radio = MockRadio::new();
            assert!(!radio.is_sleeping());

            // Sleep and verify
            radio.sleep().await.unwrap();
            assert!(radio.is_sleeping());

            // Sending while sleeping should fail
            let err = radio.send(&[1, 2, 3]).await.unwrap_err();
            assert_eq!(err, RadioError::InvalidState);

            // Standby and verify send works
            radio.standby().await.unwrap();
            assert!(!radio.is_sleeping());
            radio.send(&[1, 2, 3]).await.unwrap();
            assert_eq!(radio.sent_count(), 1);
        });
    }

    #[test]
    fn mock_radio_empty_recv() {
        block_on(async {
            let mut radio = MockRadio::new();
            let mut buf = [0u8; MAX_TRANS_UNIT];
            let err = radio.recv(&mut buf).await.unwrap_err();
            assert_eq!(err, RadioError::RecvFailed);
        });
    }

    #[test]
    fn mock_radio_channel_active() {
        block_on(async {
            let mut radio = MockRadio::new();
            assert!(!radio.channel_active().await.unwrap());
        });
    }

    #[test]
    fn mock_radio_estimate_airtime() {
        let radio = MockRadio::new();
        assert_eq!(radio.estimate_airtime_ms(10), 20);
        assert_eq!(radio.estimate_airtime_ms(0), 0);
        assert_eq!(radio.estimate_airtime_ms(100), 200);
    }

    // -- MockRng tests --

    #[test]
    fn mock_rng_deterministic() {
        let mut rng = MockRng::new(42);

        let mut buf1 = [0u8; 4];
        rng.random(&mut buf1);
        assert_eq!(buf1, [42, 43, 44, 45]);

        let mut buf2 = [0u8; 3];
        rng.random(&mut buf2);
        assert_eq!(buf2, [46, 47, 48]);

        // Second instance with same seed produces same results
        let mut rng2 = MockRng::new(42);
        let mut buf3 = [0u8; 4];
        rng2.random(&mut buf3);
        assert_eq!(buf3, [42, 43, 44, 45]);
    }

    #[test]
    fn mock_rng_wrapping() {
        let mut rng = MockRng::new(254);
        let mut buf = [0u8; 4];
        rng.random(&mut buf);
        assert_eq!(buf, [254, 255, 0, 1]);
    }

    #[test]
    fn mock_rng_next_u32() {
        let mut rng = MockRng::new(0);
        for _ in 0..20 {
            let val = rng.next_u32(10, 20);
            assert!(val >= 10 && val < 20);
        }
    }

    #[test]
    fn mock_rng_next_u32_equal_bounds() {
        let mut rng = MockRng::new(0);
        let val = rng.next_u32(5, 5);
        assert_eq!(val, 5);
    }

    // -- MockRtcClock tests --

    #[test]
    fn mock_rtc_get_set() {
        let mut clock = MockRtcClock::new(1000);
        assert_eq!(clock.get_time(), 1000);

        clock.set_time(2000);
        assert_eq!(clock.get_time(), 2000);
    }

    #[test]
    fn mock_rtc_advance() {
        let mut clock = MockRtcClock::new(1000);
        clock.advance(60);
        assert_eq!(clock.get_time(), 1060);
        clock.advance(40);
        assert_eq!(clock.get_time(), 1100);
    }

    #[test]
    fn mock_rtc_unique() {
        let mut clock = MockRtcClock::new(100);

        // First call returns the current time
        let t1 = clock.get_time_unique();
        assert_eq!(t1, 100);

        // Subsequent calls within same "second" return incrementing values
        let t2 = clock.get_time_unique();
        assert_eq!(t2, 101);

        let t3 = clock.get_time_unique();
        assert_eq!(t3, 102);

        // All values are unique
        assert_ne!(t1, t2);
        assert_ne!(t2, t3);
        assert_ne!(t1, t3);

        // Advancing past last_unique resets to current time
        clock.advance(50);
        let t4 = clock.get_time_unique();
        assert_eq!(t4, 150);
    }
}
