//! SX1262 radio driver wrapping the `lora-phy` crate.
//!
//! Provides [`Sx1262Radio`], which implements the [`Radio`](crate::Radio) trait
//! using lora-phy's async SX1262 support. Requires the `sx1262` feature.
//!
//! # Usage
//!
//! Board crates construct this with platform-specific SPI and GPIO types:
//! ```ignore
//! let iv = GenericSx126xInterfaceVariant::new(reset, dio1, busy, None, None).unwrap();
//! let sx = Sx126x::new(spi, iv);
//! let lora = LoRa::new(sx, false, delay).await.unwrap();
//! let mut radio = Sx1262Radio::new(lora);
//! radio.configure(&RadioConfig::default()).await.unwrap();
//! ```

use crate::radio::{Radio, RadioConfig, RadioError, RecvResult};
use lora_phy::mod_params::{
    Bandwidth, CodingRate, ModulationParams, PacketParams, SpreadingFactor,
};
use lora_phy::mod_traits::RadioKind;
use lora_phy::{DelayNs, LoRa, RxMode};
use meshcore_core::constants::MAX_TRANS_UNIT;

/// Convert our RadioConfig bandwidth (kHz) to lora-phy Bandwidth enum.
fn to_bandwidth(bw_khz: f32) -> Bandwidth {
    if bw_khz <= 7.9 {
        Bandwidth::_7KHz
    } else if bw_khz <= 10.5 {
        Bandwidth::_10KHz
    } else if bw_khz <= 15.7 {
        Bandwidth::_15KHz
    } else if bw_khz <= 20.9 {
        Bandwidth::_20KHz
    } else if bw_khz <= 31.3 {
        Bandwidth::_31KHz
    } else if bw_khz <= 41.7 {
        Bandwidth::_41KHz
    } else if bw_khz <= 62.6 {
        Bandwidth::_62KHz
    } else if bw_khz <= 125.1 {
        Bandwidth::_125KHz
    } else if bw_khz <= 250.1 {
        Bandwidth::_250KHz
    } else {
        Bandwidth::_500KHz
    }
}

/// Convert our spreading factor (5-12) to lora-phy SpreadingFactor.
fn to_spreading_factor(sf: u8) -> SpreadingFactor {
    match sf {
        5 => SpreadingFactor::_5,
        6 => SpreadingFactor::_6,
        7 => SpreadingFactor::_7,
        9 => SpreadingFactor::_9,
        10 => SpreadingFactor::_10,
        11 => SpreadingFactor::_11,
        12 => SpreadingFactor::_12,
        _ => SpreadingFactor::_8, // default for 8 and invalid values
    }
}

/// Convert our coding rate (5-8) to lora-phy CodingRate.
fn to_coding_rate(cr: u8) -> CodingRate {
    match cr {
        6 => CodingRate::_4_6,
        7 => CodingRate::_4_7,
        8 => CodingRate::_4_8,
        _ => CodingRate::_4_5, // default for 5 and invalid values
    }
}

/// SX1262 radio driver wrapping lora-phy.
///
/// Generic over the lora-phy `RadioKind` and `DelayNs` types, which are
/// provided by the board crate with platform-specific SPI and GPIO.
pub struct Sx1262Radio<RK: RadioKind, DLY: DelayNs> {
    lora: LoRa<RK, DLY>,
    config: RadioConfig,
    modulation: Option<ModulationParams>,
    tx_pkt_params: Option<PacketParams>,
    rx_pkt_params: Option<PacketParams>,
}

impl<RK: RadioKind, DLY: DelayNs> Sx1262Radio<RK, DLY> {
    /// Create a new SX1262 radio from an initialized lora-phy `LoRa` instance.
    ///
    /// Call [`configure`](Radio::configure) before sending or receiving.
    pub fn new(lora: LoRa<RK, DLY>) -> Self {
        Self {
            lora,
            config: RadioConfig::default(),
            modulation: None,
            tx_pkt_params: None,
            rx_pkt_params: None,
        }
    }
}

impl<RK: RadioKind, DLY: DelayNs> Radio for Sx1262Radio<RK, DLY> {
    async fn configure(&mut self, config: &RadioConfig) -> Result<(), RadioError> {
        self.config = *config;

        let freq_hz = (config.frequency_mhz * 1_000_000.0) as u32;
        let sf = to_spreading_factor(config.spreading_factor);
        let bw = to_bandwidth(config.bandwidth_khz);
        let cr = to_coding_rate(config.coding_rate);

        let modulation = self
            .lora
            .create_modulation_params(sf, bw, cr, freq_hz)
            .map_err(|_| RadioError::ConfigError)?;

        let preamble = config.preamble_symbols;

        let tx_pkt_params = self
            .lora
            .create_tx_packet_params(preamble, false, true, false, &modulation)
            .map_err(|_| RadioError::ConfigError)?;

        let rx_pkt_params = self
            .lora
            .create_rx_packet_params(preamble, false, MAX_TRANS_UNIT as u8, true, false, &modulation)
            .map_err(|_| RadioError::ConfigError)?;

        self.modulation = Some(modulation);
        self.tx_pkt_params = Some(tx_pkt_params);
        self.rx_pkt_params = Some(rx_pkt_params);
        Ok(())
    }

    async fn send(&mut self, data: &[u8]) -> Result<(), RadioError> {
        let modulation = self.modulation.as_ref().ok_or(RadioError::InvalidState)?;
        let tx_params = self.tx_pkt_params.as_mut().ok_or(RadioError::InvalidState)?;

        self.lora
            .prepare_for_tx(modulation, tx_params, self.config.tx_power as i32, data)
            .await
            .map_err(|_| RadioError::SendFailed)?;

        self.lora.tx().await.map_err(|_| RadioError::SendFailed)?;
        Ok(())
    }

    async fn recv(&mut self, buf: &mut [u8; MAX_TRANS_UNIT]) -> Result<RecvResult, RadioError> {
        let modulation = self.modulation.as_ref().ok_or(RadioError::InvalidState)?;
        let rx_params = self.rx_pkt_params.as_ref().ok_or(RadioError::InvalidState)?;

        self.lora
            .prepare_for_rx(RxMode::Continuous, modulation, rx_params)
            .await
            .map_err(|_| RadioError::RecvFailed)?;

        let (len, status) = self
            .lora
            .rx(rx_params, buf)
            .await
            .map_err(|_| RadioError::RecvFailed)?;

        Ok(RecvResult {
            len: len as usize,
            rssi: status.rssi as f32,
            snr: status.snr as f32,
        })
    }

    async fn channel_active(&mut self) -> Result<bool, RadioError> {
        let modulation = self.modulation.as_ref().ok_or(RadioError::InvalidState)?;

        self.lora
            .prepare_for_cad(modulation)
            .await
            .map_err(|_| RadioError::RecvFailed)?;

        self.lora
            .cad(modulation)
            .await
            .map_err(|_| RadioError::RecvFailed)
    }

    fn estimate_airtime_ms(&self, len: usize) -> u32 {
        // Simplified LoRa airtime estimation.
        let sf = self.config.spreading_factor as u32;
        let bw_hz = (self.config.bandwidth_khz * 1000.0) as u32;
        if bw_hz == 0 {
            return 0;
        }
        let symbol_duration_us = (1u32 << sf) * 1_000_000 / bw_hz;
        let preamble_symbols = 8 + 4; // 8 configured + 4.25 sync
        let payload_symbols = {
            let payload_bits = (len as u32) * 8;
            let sf_bits = sf * 4;
            let cr = self.config.coding_rate as u32;
            if sf_bits == 0 {
                0
            } else {
                let numerator = payload_bits.saturating_sub(sf_bits) + 28 + 16;
                let symbols = numerator.div_ceil(sf_bits);
                8 + symbols * cr
            }
        };
        let total_symbols = preamble_symbols + payload_symbols;
        (total_symbols * symbol_duration_us + 500) / 1000
    }

    async fn sleep(&mut self) -> Result<(), RadioError> {
        self.lora
            .sleep(true)
            .await
            .map_err(|_| RadioError::InvalidState)
    }

    async fn standby(&mut self) -> Result<(), RadioError> {
        self.lora
            .enter_standby()
            .await
            .map_err(|_| RadioError::InvalidState)
    }
}
