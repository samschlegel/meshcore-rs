//! TX airtime duty cycle tracking.
//!
//! Implements the MeshCore C Dispatcher duty cycle logic:
//! - `duty_cycle = 1.0 / (1.0 + airtime_budget_factor)`
//! - Budget refills over time at `duty_cycle` rate
//! - Budget is deducted after each transmission
//! - Transmission allowed only if budget >= est_airtime/2 AND >= min_reserve

use embassy_time::Instant;

use crate::types::DispatcherConfig;

/// Divisor applied to estimated airtime when checking if TX is allowed.
/// Budget must be at least `est_airtime / MIN_TX_BUDGET_AIRTIME_DIV`.
const MIN_TX_BUDGET_AIRTIME_DIV: u32 = 2;

/// Tracks TX airtime budget with a rolling duty cycle window.
///
/// Matches the MeshCore C Dispatcher duty cycle logic:
/// - `duty_cycle = 1.0 / (1.0 + airtime_budget_factor)`
/// - Budget refills over time at `duty_cycle` rate
/// - Budget is deducted after each transmission
/// - Transmission is allowed only if `budget >= est_airtime/2` AND `>= min_reserve`
pub struct DutyCycleTracker {
    /// Remaining TX budget in milliseconds.
    budget_ms: u32,
    /// Timestamp of last budget refill.
    last_refill: Instant,
    /// Maximum budget (`duty_cycle_window_ms * duty_cycle`).
    max_budget_ms: u32,
    /// Duty cycle ratio (0.0 to 1.0). Computed from `airtime_budget_factor`.
    duty_cycle: f32,
    /// Minimum TX budget reserve before allowing TX.
    min_reserve_ms: u32,
}

impl DutyCycleTracker {
    /// Create a new tracker from config. Starts with full budget.
    pub fn new(config: &DispatcherConfig) -> Self {
        let duty_cycle = 1.0 / (1.0 + config.airtime_budget_factor);
        let max_budget_ms = (config.duty_cycle_window_ms as f32 * duty_cycle) as u32;
        Self {
            budget_ms: max_budget_ms,
            last_refill: Instant::from_millis(0),
            max_budget_ms,
            duty_cycle,
            min_reserve_ms: config.min_tx_budget_reserve_ms,
        }
    }

    /// Refill the budget based on elapsed time since last refill.
    /// Call this before checking [`can_transmit`](Self::can_transmit).
    pub fn refill(&mut self, now: Instant) {
        let elapsed_ms = (now - self.last_refill).as_millis();
        let refill = (elapsed_ms as f32 * self.duty_cycle) as u32;
        self.budget_ms = self.max_budget_ms.min(self.budget_ms.saturating_add(refill));
        self.last_refill = now;
    }

    /// Check if there's enough budget to transmit a packet with the given
    /// estimated airtime. Caller should call [`refill`](Self::refill) first.
    pub fn can_transmit(&self, est_airtime_ms: u32) -> bool {
        self.budget_ms >= est_airtime_ms / MIN_TX_BUDGET_AIRTIME_DIV
            && self.budget_ms >= self.min_reserve_ms
    }

    /// Deduct actual airtime from the budget after a successful transmission.
    pub fn deduct(&mut self, actual_airtime_ms: u32) {
        self.budget_ms = self.budget_ms.saturating_sub(actual_airtime_ms);
    }

    /// Get the remaining budget in milliseconds.
    pub fn remaining_ms(&self) -> u32 {
        self.budget_ms
    }

    /// Get the maximum budget in milliseconds.
    pub fn max_budget_ms(&self) -> u32 {
        self.max_budget_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_with_full_budget() {
        let tracker = DutyCycleTracker::new(&DispatcherConfig::default());
        // duty_cycle = 1/(1+1) = 0.5, max = 3_600_000 * 0.5 = 1_800_000
        assert_eq!(tracker.remaining_ms(), 1_800_000);
        assert_eq!(tracker.max_budget_ms(), 1_800_000);
    }

    #[test]
    fn can_transmit_with_full_budget() {
        let tracker = DutyCycleTracker::new(&DispatcherConfig::default());
        assert!(tracker.can_transmit(500));
        assert!(tracker.can_transmit(1000));
    }

    #[test]
    fn deduct_reduces_budget() {
        let mut tracker = DutyCycleTracker::new(&DispatcherConfig::default());
        let before = tracker.remaining_ms();
        tracker.deduct(1000);
        assert_eq!(tracker.remaining_ms(), before - 1000);
    }

    #[test]
    fn deduct_saturates_at_zero() {
        let mut tracker = DutyCycleTracker::new(&DispatcherConfig::default());
        tracker.deduct(tracker.remaining_ms() + 1000);
        assert_eq!(tracker.remaining_ms(), 0);
    }

    #[test]
    fn can_transmit_false_when_depleted() {
        let mut tracker = DutyCycleTracker::new(&DispatcherConfig::default());
        tracker.deduct(tracker.remaining_ms());
        assert!(!tracker.can_transmit(500));
    }

    #[test]
    fn refill_restores_budget() {
        let mut tracker = DutyCycleTracker::new(&DispatcherConfig::default());
        tracker.deduct(100_000);
        let after_deduct = tracker.remaining_ms();

        // Advance 60 seconds => refill = 60_000 * 0.5 = 30_000
        tracker.refill(Instant::from_millis(60_000));
        assert_eq!(tracker.remaining_ms(), after_deduct + 30_000);
    }

    #[test]
    fn refill_caps_at_max() {
        let mut tracker = DutyCycleTracker::new(&DispatcherConfig::default());
        let max = tracker.max_budget_ms();

        tracker.refill(Instant::from_millis(60_000));
        assert_eq!(tracker.remaining_ms(), max);
    }

    #[test]
    fn can_transmit_checks_min_reserve() {
        let mut tracker = DutyCycleTracker::new(&DispatcherConfig::default());
        // Drain budget to 50 ms (below min_reserve of 100 ms)
        tracker.deduct(tracker.remaining_ms() - 50);
        assert_eq!(tracker.remaining_ms(), 50);
        // budget=50 < min_reserve=100
        assert!(!tracker.can_transmit(100));
    }

    #[test]
    fn custom_config() {
        let config = DispatcherConfig {
            airtime_budget_factor: 3.0,
            ..Default::default()
        };
        let tracker = DutyCycleTracker::new(&config);
        // duty_cycle = 1/(1+3) = 0.25, max = 3_600_000 * 0.25 = 900_000
        assert_eq!(tracker.max_budget_ms(), 900_000);
        assert_eq!(tracker.remaining_ms(), 900_000);
    }
}
