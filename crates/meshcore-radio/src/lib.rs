#![no_std]
#![deny(unsafe_code)]
#![allow(async_fn_in_trait)] // Embassy uses single-threaded executors; Send bounds not needed

pub mod radio;
pub mod rng;
pub mod rtc;

#[cfg(any(test, feature = "mock"))]
pub mod mock;

#[cfg(feature = "sx1262")]
pub mod sx1262;

pub use radio::{Radio, RadioConfig, RadioError, RecvResult};
pub use rng::Rng;
pub use rtc::RtcClock;
