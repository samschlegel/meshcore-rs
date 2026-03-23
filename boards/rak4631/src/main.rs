//! RAK4631 Embassy blink — with LED debug checkpoints.
//!
//! LED patterns:
//!   1 green blink  = entered main
//!   2 green blinks = embassy init done
//!   3 green blinks = GPIO configured
//!   alternating    = timer works!
//!   rapid both     = PANIC

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_time::Timer;

static PANICKED: AtomicBool = AtomicBool::new(false);

// P1 GPIO registers for panic handler (can't use embassy GPIO after panic)
const P1: u32 = 0x5000_0300;
const OUTSET: u32 = P1 + 0x508;
const OUTCLR: u32 = P1 + 0x50C;

#[allow(unsafe_code)]
fn raw_set(pin: u8) {
    unsafe { core::ptr::write_volatile(OUTSET as *mut u32, 1 << pin); }
}

#[allow(unsafe_code)]
fn raw_clear(pin: u8) {
    unsafe { core::ptr::write_volatile(OUTCLR as *mut u32, 1 << pin); }
}

fn raw_delay_short() {
    for _ in 0..2_000_000u32 { cortex_m::asm::nop(); }
}

fn raw_delay_long() {
    for _ in 0..8_000_000u32 { cortex_m::asm::nop(); }
}

/// Blink green LED N times (using raw registers, works before/after embassy)
fn checkpoint(n: u8) {
    for _ in 0..n {
        raw_set(3); // green on (active high)
        raw_delay_short();
        raw_clear(3);   // green off
        raw_delay_short();
    }
    raw_delay_long(); // pause between checkpoints
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    PANICKED.store(true, Ordering::Relaxed);
    // Rapid alternating blink = panic
    loop {
        raw_set(3);   raw_clear(4); // green on, blue off
        raw_delay_short();
        raw_clear(3); raw_set(4);   // green off, blue on
        raw_delay_short();
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // Checkpoint 1: entered main

    let mut config = embassy_nrf::config::Config::default();
    //config.time_interrupt_priority = embassy_nrf::interrupt::Priority::P2;
    //config.gpiote_interrupt_priority = embassy_nrf::interrupt::Priority::P2;
    let p = embassy_nrf::init(config);

    let mut green = Output::new(p.P1_03, Level::Low, OutputDrive::Standard);
    let mut blue = Output::new(p.P1_04, Level::Low, OutputDrive::Standard);

    // Checkpoint 3: GPIO configured
    checkpoint(3);

    // Now try the timer (active HIGH: set_high = on, set_low = off)
    loop {
        green.set_high(); // on
        blue.set_low();   // off
        Timer::after_millis(500).await;

        green.set_low();  // off
        blue.set_high();  // on
        Timer::after_millis(500).await;
    }
}
