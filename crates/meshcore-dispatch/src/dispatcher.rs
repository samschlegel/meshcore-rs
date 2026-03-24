//! Async dispatcher: manages radio TX/RX, packet scheduling, duty cycle.
//!
//! The [`Dispatcher`] owns the radio exclusively and coordinates all
//! transmission and reception through an async event loop. It communicates
//! with other tasks (Mesh layer) via Embassy channels.
//!
//! Design: ADR-0006 — select-based event loop with channels.

use embassy_futures::select::{select, select3, Either, Either3};
use embassy_sync::channel::Channel;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Duration, Instant, Timer};

use meshcore_core::constants::MAX_TRANS_UNIT;
use meshcore_core::packet::Packet;
use meshcore_radio::radio::Radio;
use meshcore_radio::rng::Rng;

use crate::duty_cycle::DutyCycleTracker;
use crate::rx_delay::{calc_rx_delay, RxDelayQueue};
use crate::tx_queue::TxQueue;
use crate::types::{DispatcherConfig, DispatcherStats, RxPacket, TxRequest};

/// The Dispatcher owns the radio and manages TX/RX scheduling.
///
/// Generic over:
/// - `R`: Radio implementation
/// - `RNG`: Random number generator (for CAD jitter)
/// - `TX_Q`: TX queue capacity
/// - `RX_Q`: RX delay queue capacity
pub struct Dispatcher<R: Radio, RNG: Rng, const TX_Q: usize, const RX_Q: usize> {
    radio: R,
    rng: RNG,
    config: DispatcherConfig,
    tx_queue: TxQueue<TX_Q>,
    rx_delay_queue: RxDelayQueue<RX_Q>,
    duty_cycle: DutyCycleTracker,
    stats: DispatcherStats,
}

impl<R: Radio, RNG: Rng, const TX_Q: usize, const RX_Q: usize>
    Dispatcher<R, RNG, TX_Q, RX_Q>
{
    /// Create a new Dispatcher, taking ownership of the radio and RNG.
    pub fn new(radio: R, rng: RNG, config: DispatcherConfig) -> Self {
        let duty_cycle = DutyCycleTracker::new(&config);
        Self {
            radio,
            rng,
            config,
            tx_queue: TxQueue::new(),
            rx_delay_queue: RxDelayQueue::new(),
            duty_cycle,
            stats: DispatcherStats::default(),
        }
    }

    /// Get a reference to the dispatcher statistics.
    pub fn stats(&self) -> &DispatcherStats {
        &self.stats
    }

    /// Reset statistics counters.
    pub fn reset_stats(&mut self) {
        self.stats = DispatcherStats::default();
    }

    /// Run the dispatcher event loop.
    ///
    /// This is the main async task. It never returns. It:
    /// 1. Drains TX requests from `tx_in` into the priority queue
    /// 2. Delivers ready RX-delayed packets to `rx_out`
    /// 3. Uses `select!` to wait for: radio RX, TX submission, or timer wakeup
    /// 4. Attempts TX when a queued packet is ready and duty cycle allows
    ///
    /// # Arguments
    /// - `tx_in`: Channel from which TX requests are received (Mesh → Dispatcher)
    /// - `rx_out`: Channel to which received packets are delivered (Dispatcher → Mesh)
    pub async fn run(
        &mut self,
        tx_in: &Channel<CriticalSectionRawMutex, TxRequest, TX_Q>,
        rx_out: &Channel<CriticalSectionRawMutex, RxPacket, RX_Q>,
    ) -> ! {
        let mut rx_buf = [0u8; MAX_TRANS_UNIT];

        loop {
            // 1. Drain pending TX submissions into the priority queue.
            while let Ok(req) = tx_in.try_receive() {
                // Drop if queue is full — same as C pool exhaustion.
                let _ = self.tx_queue.push(req);
            }

            // 2. Deliver any RX-delayed packets that are now ready.
            let now = Instant::now();
            while let Some(rx_pkt) = self.rx_delay_queue.pop_ready(now) {
                // Try to deliver; drop if mesh layer can't keep up.
                let _ = rx_out.try_send(rx_pkt);
            }

            // 3. Attempt TX if something is ready and duty cycle allows.
            self.maybe_transmit(&mut rx_buf).await;

            // 4. Compute the next wake time for timer-based scheduling.
            let next_wake = self.next_wake_time();

            // 5. Wait for: radio RX | TX submission | timer wakeup.
            match next_wake {
                Some(wake_at) => {
                    match select3(
                        self.radio.recv(&mut rx_buf),
                        tx_in.receive(),
                        Timer::at(wake_at),
                    )
                    .await
                    {
                        Either3::First(result) => {
                            self.handle_rx(result, &rx_buf, rx_out).await;
                        }
                        Either3::Second(tx_req) => {
                            let _ = self.tx_queue.push(tx_req);
                        }
                        Either3::Third(()) => {
                            // Timer fired — re-evaluate queues at top of loop.
                        }
                    }
                }
                None => {
                    // Nothing scheduled — pure RX + wait for TX submission.
                    match select(self.radio.recv(&mut rx_buf), tx_in.receive()).await {
                        Either::First(result) => {
                            self.handle_rx(result, &rx_buf, rx_out).await;
                        }
                        Either::Second(tx_req) => {
                            let _ = self.tx_queue.push(tx_req);
                        }
                    }
                }
            }
        }
    }

    /// Handle a received packet from the radio.
    async fn handle_rx(
        &mut self,
        result: Result<meshcore_radio::radio::RecvResult, meshcore_radio::radio::RadioError>,
        rx_buf: &[u8; MAX_TRANS_UNIT],
        rx_out: &Channel<CriticalSectionRawMutex, RxPacket, RX_Q>,
    ) {
        let recv_result = match result {
            Ok(r) => r,
            Err(_) => return,
        };

        let mut packet = Packet::new();
        if !packet.read_from(&rx_buf[..recv_result.len]) {
            return; // Malformed packet — discard.
        }

        // Update stats.
        let is_flood = packet
            .route_type()
            .map(|rt| rt.is_flood())
            .unwrap_or(false);
        if is_flood {
            self.stats.recv_flood += 1;
        } else {
            self.stats.recv_direct += 1;
        }

        let rx_packet = RxPacket {
            packet,
            rssi: recv_result.rssi,
            snr: recv_result.snr,
        };

        // For flood packets, apply SNR-based delay.
        if is_flood {
            let airtime = self.radio.estimate_airtime_ms(recv_result.len);
            if let Some(delay_ms) = calc_rx_delay(recv_result.snr, airtime, &self.config) {
                let deliver_at = Instant::now() + Duration::from_millis(delay_ms as u64);
                // If delay queue is full, deliver immediately.
                if self.rx_delay_queue.push(rx_packet, deliver_at).is_err() {
                    // Queue full — try immediate delivery.
                }
                return;
            }
        }

        // Immediate delivery (direct packets or strong-signal floods).
        let _ = rx_out.try_send(rx_packet);
    }

    /// Attempt to transmit the next ready packet from the TX queue.
    async fn maybe_transmit(&mut self, scratch_buf: &mut [u8; MAX_TRANS_UNIT]) {
        let now = Instant::now();
        self.duty_cycle.refill(now);

        // Peek at what's ready without removing it yet.
        let ready = self.tx_queue.pop_ready(now);
        let entry = match ready {
            Some(e) => e,
            None => return,
        };

        // Check duty cycle.
        let wire_len = entry.packet.wire_len();
        let est_airtime = self.radio.estimate_airtime_ms(wire_len);
        if !self.duty_cycle.can_transmit(est_airtime) {
            // Not enough budget — put it back.
            let _ = self.tx_queue.push_entry(entry);
            return;
        }

        // CAD — wait for clear channel.
        if !self.wait_for_clear_channel().await {
            // CAD timeout — transmit anyway (matches C behavior).
        }

        // Serialize and transmit.
        let len = entry.packet.write_to(scratch_buf);
        let tx_start = Instant::now();
        let tx_result = self.radio.send(&scratch_buf[..len]).await;
        let actual_airtime = (Instant::now() - tx_start).as_millis() as u32;

        match tx_result {
            Ok(()) => {
                self.duty_cycle.deduct(actual_airtime);
                self.stats.total_air_time_ms += actual_airtime;

                // Update per-type stats.
                let is_flood = entry
                    .packet
                    .route_type()
                    .map(|rt| rt.is_flood())
                    .unwrap_or(false);
                if is_flood {
                    self.stats.sent_flood += 1;
                } else {
                    self.stats.sent_direct += 1;
                }
            }
            Err(_) => {
                // TX failed — packet is lost. Could re-queue with backoff,
                // but for now match C behavior (log and drop).
            }
        }
    }

    /// Wait for the channel to be clear (CAD — Channel Activity Detection).
    ///
    /// Returns `true` if channel is clear, `false` if timeout (force TX).
    async fn wait_for_clear_channel(&mut self) -> bool {
        let deadline = Instant::now() + Duration::from_millis(self.config.cad_timeout_ms as u64);

        loop {
            match self.radio.channel_active().await {
                Ok(false) => return true,  // Channel clear.
                Ok(true) => {}             // Channel busy — retry.
                Err(_) => return true,     // Error — assume clear.
            }

            if Instant::now() >= deadline {
                return false; // Timeout — force TX.
            }

            // Random backoff before retry.
            let delay = self.rng.next_u32(
                self.config.cad_retry_min_ms,
                self.config.cad_retry_max_ms,
            );
            Timer::after(Duration::from_millis(delay as u64)).await;
        }
    }

    /// Compute the next time we should wake up for queue processing.
    fn next_wake_time(&self) -> Option<Instant> {
        let tx_time = self.tx_queue.next_ready_time();
        let rx_time = self.rx_delay_queue.next_ready_time();

        match (tx_time, rx_time) {
            (Some(a), Some(b)) => Some(if a < b { a } else { b }),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }
}
