//! Random number generation trait.
//!
//! Reference: `Utils.h` RNG class in the C implementation.

/// Random number generator for crypto seeds and backoff jitter.
///
/// Synchronous — no timing dependency, just fills a buffer with random bytes.
pub trait Rng {
    /// Fill `dest` with random bytes.
    fn random(&mut self, dest: &mut [u8]);

    /// Generate a random u32 in the range `[min, max)`.
    fn next_u32(&mut self, min: u32, max: u32) -> u32 {
        if min >= max {
            return min;
        }
        let mut buf = [0u8; 4];
        self.random(&mut buf);
        let val = u32::from_le_bytes(buf);
        min + (val % (max - min))
    }
}
