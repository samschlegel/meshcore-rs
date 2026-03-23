//! Real-time clock trait for wall-clock timestamps.
//!
//! Monotonic time (millis since boot) is provided by `embassy_time::Instant`
//! and does not need a custom trait. This trait covers only wall-clock (UNIX
//! epoch) time, used for packet timestamps and advertisements.
//!
//! Reference: `MeshCore.h` RTCClock class in the C implementation.

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
