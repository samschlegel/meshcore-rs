//! Async radio trait for LoRa transceivers.
//!
//! The [`Radio`] trait abstracts over LoRa hardware (SX1262, etc.) with async
//! send/recv operations that sleep until the radio fires an interrupt.
//!
//! Reference: `Dispatcher.h` Radio class in the C implementation.

use meshcore_core::constants::MAX_TRANS_UNIT;

/// Result of a successful receive operation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct RecvResult {
    /// Number of bytes received.
    pub len: usize,
    /// Received signal strength indicator (dBm).
    pub rssi: f32,
    /// Signal-to-noise ratio (dB).
    pub snr: f32,
}

/// Radio errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum RadioError {
    /// Send failed (SPI error, timeout, etc.)
    SendFailed,
    /// Receive failed or timed out.
    RecvFailed,
    /// Buffer too small for received packet.
    BufferTooSmall,
    /// Radio not in the expected state.
    InvalidState,
    /// Configuration error (invalid frequency, power, etc.)
    ConfigError,
}

/// LoRa radio configuration parameters.
///
/// Default values match the MeshCore C defaults.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct RadioConfig {
    /// Frequency in MHz (e.g., 869.618).
    pub frequency_mhz: f32,
    /// Bandwidth in kHz (e.g., 62.5, 125.0, 250.0, 500.0).
    pub bandwidth_khz: f32,
    /// Spreading factor (6-12).
    pub spreading_factor: u8,
    /// Coding rate denominator (5-8, where 5 = 4/5, 8 = 4/8).
    pub coding_rate: u8,
    /// Transmit power in dBm.
    pub tx_power: i8,
}

impl Default for RadioConfig {
    /// MeshCore C defaults: 869.618 MHz, BW 62.5 kHz, SF 8, CR 4/5, TX 22 dBm.
    fn default() -> Self {
        Self {
            frequency_mhz: 869.618,
            bandwidth_khz: 62.5,
            spreading_factor: 8,
            coding_rate: 5,
            tx_power: 22,
        }
    }
}

/// Async LoRa radio interface.
///
/// Implementations drive the radio hardware via SPI and use interrupt-driven
/// wakers for power-efficient send/recv. This maps to the C `Radio` abstract
/// class but collapses the split-phase `startSendRaw`/`isSendComplete` into
/// a single `async fn send`.
///
/// # Timing
///
/// All timing (duty cycle, backoff, CAD timeouts) is handled by the caller
/// using `embassy_time::Timer`. The Radio trait focuses on raw packet I/O.
pub trait Radio {
    /// Apply radio configuration (frequency, bandwidth, SF, CR, TX power).
    ///
    /// Called once during setup, or when configuration changes at runtime.
    async fn configure(&mut self, config: &RadioConfig) -> Result<(), RadioError>;

    /// Send raw bytes over the air. Blocks until transmission is complete.
    ///
    /// `data` must be at most [`MAX_TRANS_UNIT`] bytes.
    async fn send(&mut self, data: &[u8]) -> Result<(), RadioError>;

    /// Receive raw bytes. Blocks until a packet arrives or an error occurs.
    ///
    /// On success, the received bytes are written to `buf` and a [`RecvResult`]
    /// is returned with the length, RSSI, and SNR.
    async fn recv(&mut self, buf: &mut [u8; MAX_TRANS_UNIT]) -> Result<RecvResult, RadioError>;

    /// Perform Channel Activity Detection (listen-before-talk).
    ///
    /// Returns `true` if the channel is currently active (another node is transmitting).
    async fn channel_active(&mut self) -> Result<bool, RadioError>;

    /// Estimate the over-the-air time in milliseconds for a packet of `len` bytes.
    ///
    /// Used by the Dispatcher for duty cycle management.
    fn estimate_airtime_ms(&self, len: usize) -> u32;

    /// Put the radio into low-power sleep mode.
    async fn sleep(&mut self) -> Result<(), RadioError>;

    /// Put the radio into standby (ready to send/recv quickly).
    async fn standby(&mut self) -> Result<(), RadioError>;
}
