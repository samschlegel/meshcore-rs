//! RAK4631 SX1262 smoke test — LED checkpoints + LoRa TX.
//!
//! LED patterns:
//!   1 green blink  = entered main
//!   2 green blinks = SPI + radio init done
//!   3 green blinks = radio configured
//!   green flash    = packet sent
//!   blue steady    = listening
//!   rapid both     = PANIC

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::spim::{self, Spim};
use embassy_nrf::{bind_interrupts, peripherals};
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use lora_phy::iv::GenericSx126xInterfaceVariant;
use lora_phy::sx126x::{self, Sx126x, TcxoCtrlVoltage};
use lora_phy::LoRa;

bind_interrupts!(struct Irqs {
    TWISPI1 => spim::InterruptHandler<peripherals::TWISPI1>;
});

static PANICKED: AtomicBool = AtomicBool::new(false);

// P1 GPIO registers for panic handler (can't use embassy GPIO after panic)
const P1: u32 = 0x5000_0300;
const OUTSET: u32 = P1 + 0x508;
const OUTCLR: u32 = P1 + 0x50C;

#[allow(unsafe_code)]
fn raw_set(pin: u8) {
    unsafe {
        core::ptr::write_volatile(OUTSET as *mut u32, 1 << pin);
    }
}

#[allow(unsafe_code)]
fn raw_clear(pin: u8) {
    unsafe {
        core::ptr::write_volatile(OUTCLR as *mut u32, 1 << pin);
    }
}

fn raw_delay_short() {
    for _ in 0..2_000_000u32 {
        cortex_m::asm::nop();
    }
}

fn raw_delay_long() {
    for _ in 0..8_000_000u32 {
        cortex_m::asm::nop();
    }
}

/// Blink green LED N times (using raw registers, works before/after embassy)
fn checkpoint(n: u8) {
    for _ in 0..n {
        raw_set(3); // green on (active high)
        raw_delay_short();
        raw_clear(3);
        raw_delay_short();
    }
    raw_delay_long();
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    PANICKED.store(true, Ordering::Relaxed);
    loop {
        raw_set(3);
        raw_clear(4);
        raw_delay_short();
        raw_clear(3);
        raw_set(4);
        raw_delay_short();
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    checkpoint(1);

    let p = embassy_nrf::init(embassy_nrf::config::Config::default());

    let mut green = Output::new(p.P1_03, Level::Low, OutputDrive::Standard);
    let mut blue = Output::new(p.P1_04, Level::Low, OutputDrive::Standard);

    // SPI for SX1262: SCK=P1.11, MOSI=P1.12, MISO=P1.13
    let mut spi_config = spim::Config::default();
    spi_config.frequency = spim::Frequency::M8;
    let spi = Spim::new(p.TWISPI1, Irqs, p.P1_11, p.P1_13, p.P1_12, spi_config);

    // CS pin for SPI device wrapper
    let cs = Output::new(p.P1_10, Level::High, OutputDrive::Standard);
    let spi_device = ExclusiveDevice::new(spi, cs, Delay).unwrap();

    // SX1262 control pins
    let reset = Output::new(p.P1_06, Level::High, OutputDrive::Standard);
    let dio1 = Input::new(p.P1_15, Pull::Down);
    let busy = Input::new(p.P1_14, Pull::Down);

    let iv = GenericSx126xInterfaceVariant::new(reset, dio1, busy, None, None).unwrap();

    // RAK4631 uses TCXO at 1.8V on DIO3, and DCDC regulator
    let config = sx126x::Config {
        chip: sx126x::Sx1262,
        tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V8),
        use_dcdc: true,
        rx_boost: false,
    };

    let radio_kind = Sx126x::new(spi_device, iv, config);
    let mut lora = LoRa::new(radio_kind, false, Delay).await.unwrap();

    checkpoint(2);

    // Configure with MeshCore defaults: 869.618 MHz, BW 62.5 kHz, SF 7, CR 4/7
    let mod_params = lora
        .create_modulation_params(
            lora_phy::mod_params::SpreadingFactor::_7,
            lora_phy::mod_params::Bandwidth::_62KHz,
            lora_phy::mod_params::CodingRate::_4_5,
            910_525_000,
        )
        .unwrap();

    let mut tx_params = lora
        .create_tx_packet_params(8, false, true, false, &mod_params)
        .unwrap();

    checkpoint(3);

    // TX loop: send "HELLO" every 3 seconds
    let test_payload = b"HELLO";
    let mut count: u32 = 0;

    loop {
        green.set_high();
        match lora
            .prepare_for_tx(&mod_params, &mut tx_params, 22, test_payload)
            .await
        {
            Ok(()) => {
                let _ = lora.tx().await;
                count += 1;
            }
            Err(_) => {
                // Flash blue rapidly on error
                for _ in 0..5 {
                    blue.set_high();
                    Timer::after_millis(100).await;
                    blue.set_low();
                    Timer::after_millis(100).await;
                }
            }
        }
        green.set_low();

        // Brief blue flash to show we're alive, count blinks = packet count mod 5
        let blinks = ((count % 5) + 1) as u8;
        for _ in 0..blinks {
            blue.set_high();
            Timer::after_millis(100).await;
            blue.set_low();
            Timer::after_millis(100).await;
        }

        Timer::after_millis(3000).await;
    }
}
