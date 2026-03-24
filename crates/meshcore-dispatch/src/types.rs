//! Shared types for the dispatcher crate.

use meshcore_core::packet::Packet;

/// Request to transmit a packet, submitted by the Mesh layer.
#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct TxRequest {
    /// The packet to transmit.
    pub packet: Packet,
    /// Transmission priority (0 = highest).
    pub priority: u8,
    /// Delay in milliseconds before the packet is eligible for transmission.
    pub delay_ms: u32,
}

/// A received packet with radio metadata, delivered to the Mesh layer.
#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct RxPacket {
    /// The parsed packet.
    pub packet: Packet,
    /// Received signal strength indicator (dBm).
    pub rssi: f32,
    /// Signal-to-noise ratio (dB).
    pub snr: f32,
}

/// Dispatcher configuration parameters.
///
/// Default values match the MeshCore C implementation.
#[derive(Clone)]
pub struct DispatcherConfig {
    /// Airtime budget factor. 1.0 = 50% TX / 50% RX duty cycle.
    pub airtime_budget_factor: f32,
    /// Rolling window for duty cycle tracking, in milliseconds. Default: 3,600,000 (1 hour).
    pub duty_cycle_window_ms: u32,
    /// Minimum TX budget reserve in milliseconds. Default: 100.
    pub min_tx_budget_reserve_ms: u32,
    /// CAD retry delay minimum in milliseconds. Default: 120.
    pub cad_retry_min_ms: u32,
    /// CAD retry delay maximum in milliseconds. Default: 480.
    pub cad_retry_max_ms: u32,
    /// CAD timeout — force TX after this many ms of busy channel. Default: 4000.
    pub cad_timeout_ms: u32,
    /// Maximum RX delay for weak-signal flood packets, in milliseconds. Default: 32,000.
    pub max_rx_delay_ms: u32,
    /// Minimum RX delay threshold — delays below this are delivered immediately. Default: 50.
    pub min_rx_delay_threshold_ms: u32,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self {
            airtime_budget_factor: 1.0,
            duty_cycle_window_ms: 3_600_000,
            min_tx_budget_reserve_ms: 100,
            cad_retry_min_ms: 120,
            cad_retry_max_ms: 480,
            cad_timeout_ms: 4_000,
            max_rx_delay_ms: 32_000,
            min_rx_delay_threshold_ms: 50,
        }
    }
}

/// Dispatcher statistics counters.
#[derive(Clone, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct DispatcherStats {
    /// Total milliseconds spent transmitting.
    pub total_air_time_ms: u32,
    /// Total milliseconds spent receiving.
    pub rx_air_time_ms: u32,
    /// Flood packets sent.
    pub sent_flood: u32,
    /// Direct packets sent.
    pub sent_direct: u32,
    /// Flood packets received.
    pub recv_flood: u32,
    /// Direct packets received.
    pub recv_direct: u32,
}
