//! Real-time clock trait for wall-clock timestamps.
//!
//! Monotonic time (millis since boot) is provided by `embassy_time::Instant`
//! and does not need a custom trait. This trait covers only wall-clock (UNIX
//! epoch) time, used for packet timestamps and advertisements.
//!
//! Reference: `MeshCore.h` RTCClock class in the C implementation.

use embassy_time::Instant;

/// Wall-clock time source (UNIX epoch seconds).
pub trait RtcClock {
    /// Get the current time as UNIX epoch seconds.
    fn get_time(&self) -> u32;

    /// Set the current time (e.g., from a GPS fix or network sync).
    fn set_time(&mut self, epoch_secs: u32);

    /// Get a unique timestamp — never returns the same value twice.
    ///
    /// If called multiple times within the same second, increments beyond
    /// the real time to ensure uniqueness. Matches the C `getCurrentTimeUnique`.
    fn get_time_unique(&mut self) -> u32 {
        // Default implementation — concrete impls should override to
        // track the last-returned value and ensure uniqueness.
        self.get_time()
    }
}

/// Software RTC that advances from a base epoch using `embassy_time::Instant`.
///
/// Starts from an initial epoch (e.g., build time or NVM-restored value) and
/// tracks elapsed time via the monotonic clock. Survives sleep but not power
/// loss — pair with NVM persistence for that.
///
/// Matches the C `VolatileRTCClock` behavior from `ArduinoHelpers.h`.
pub struct VolatileRtcClock {
    /// UNIX epoch seconds at the moment `base_instant` was captured.
    base_epoch: u32,
    /// Monotonic instant when `base_epoch` was set.
    base_instant: Instant,
    /// Last value returned by `get_time_unique`, for monotonicity.
    last_unique: u32,
}

impl VolatileRtcClock {
    /// Create with an initial UNIX epoch (e.g., build time or NVM-restored value).
    pub fn new(initial_epoch: u32) -> Self {
        Self {
            base_epoch: initial_epoch,
            base_instant: Instant::now(),
            last_unique: 0,
        }
    }
}

impl RtcClock for VolatileRtcClock {
    fn get_time(&self) -> u32 {
        let elapsed_secs = self.base_instant.elapsed().as_secs();
        self.base_epoch.saturating_add(elapsed_secs as u32)
    }

    fn set_time(&mut self, epoch_secs: u32) {
        self.base_epoch = epoch_secs;
        self.base_instant = Instant::now();
    }

    fn get_time_unique(&mut self) -> u32 {
        let t = self.get_time();
        let unique = if t > self.last_unique {
            t
        } else {
            self.last_unique + 1
        };
        self.last_unique = unique;
        unique
    }
}
