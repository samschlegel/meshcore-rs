//! RAK4631 SX1262 RX smoke test — USB serial output + LED indication.
//!
//! LED patterns:
//!   1 green blink  = entered main
//!   2 green blinks = SPI + radio init done
//!   3 green blinks = radio configured, entering RX
//!   green flash    = packet received
//!   blue on        = listening (in RX mode)
//!   rapid both     = PANIC

#![no_std]
#![no_main]

use core::fmt::Write as FmtWrite;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::spim::{self, Spim};
use embassy_nrf::usb::vbus_detect::HardwareVbusDetect;
use embassy_nrf::usb::Driver as UsbDriver;
use embassy_nrf::{bind_interrupts, peripherals};
use embassy_time::{Delay, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::UsbDevice;
use embedded_hal_bus::spi::ExclusiveDevice;
use lora_phy::iv::GenericSx126xInterfaceVariant;
use lora_phy::sx126x::{self, Sx126x, TcxoCtrlVoltage};
use lora_phy::{LoRa, RxMode};
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    TWISPI1 => spim::InterruptHandler<peripherals::TWISPI1>;
    USBD => embassy_nrf::usb::InterruptHandler<peripherals::USBD>;
    CLOCK_POWER => embassy_nrf::usb::vbus_detect::InterruptHandler;
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

fn checkpoint(n: u8) {
    for _ in 0..n {
        raw_set(3);
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

type MyUsbDriver = UsbDriver<'static, HardwareVbusDetect>;

#[embassy_executor::task]
async fn usb_task(mut device: UsbDevice<'static, MyUsbDriver>) -> ! {
    device.run().await
}

/// Write data to CDC in 64-byte chunks
async fn cdc_write(cdc: &mut CdcAcmClass<'static, MyUsbDriver>, data: &[u8]) {
    for chunk in data.chunks(64) {
        let _ = cdc.write_packet(chunk).await;
    }
}

/// Format a small message (up to 64 bytes) into a buffer and return the used slice
struct SmallBuf {
    buf: [u8; 64],
    pos: usize,
}

impl SmallBuf {
    fn new() -> Self {
        Self {
            buf: [0u8; 64],
            pos: 0,
        }
    }
    fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.pos]
    }
}

impl core::fmt::Write for SmallBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let n = bytes.len().min(self.buf.len() - self.pos);
        self.buf[self.pos..self.pos + n].copy_from_slice(&bytes[..n]);
        self.pos += n;
        Ok(())
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    checkpoint(1);

    let p = embassy_nrf::init(embassy_nrf::config::Config::default());

    let mut green = Output::new(p.P1_03, Level::Low, OutputDrive::Standard);
    let mut blue = Output::new(p.P1_04, Level::Low, OutputDrive::Standard);

    // ---- USB CDC ACM setup ----
    let usb_driver = UsbDriver::new(p.USBD, Irqs, HardwareVbusDetect::new(Irqs));

    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static MSOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

    let config_desc = CONFIG_DESC.init([0u8; 256]);
    let bos_desc = BOS_DESC.init([0u8; 256]);
    let msos_desc = MSOS_DESC.init([0u8; 256]);
    let control_buf = CONTROL_BUF.init([0u8; 64]);

    let mut usb_config = embassy_usb::Config::new(0x1209, 0x0001); // pid.codes test VID/PID
    usb_config.manufacturer = Some("meshcore-rs");
    usb_config.product = Some("RAK4631 LoRa RX");
    usb_config.serial_number = Some("0001");
    usb_config.max_power = 100;
    usb_config.max_packet_size_0 = 64;

    let mut builder = embassy_usb::Builder::new(
        usb_driver,
        usb_config,
        config_desc,
        bos_desc,
        msos_desc,
        control_buf,
    );

    static CDC_STATE: StaticCell<State<'static>> = StaticCell::new();
    let cdc_state = CDC_STATE.init(State::new());
    let mut cdc = CdcAcmClass::new(&mut builder, cdc_state, 64);

    let usb = builder.build();
    spawner.spawn(usb_task(usb).unwrap());

    // Give USB a moment to enumerate
    Timer::after_millis(1000).await;

    // ---- SPI + SX1262 setup ----
    let mut spi_config = spim::Config::default();
    spi_config.frequency = spim::Frequency::M8;
    let spi = Spim::new(p.TWISPI1, Irqs, p.P1_11, p.P1_13, p.P1_12, spi_config);

    let cs = Output::new(p.P1_10, Level::High, OutputDrive::Standard);
    let spi_device = ExclusiveDevice::new(spi, cs, Delay).unwrap();

    let reset = Output::new(p.P1_06, Level::High, OutputDrive::Standard);
    let dio1 = Input::new(p.P1_15, Pull::Down);
    let busy = Input::new(p.P1_14, Pull::Down);

    let iv = GenericSx126xInterfaceVariant::new(reset, dio1, busy, None, None).unwrap();

    let config = sx126x::Config {
        chip: sx126x::Sx1262,
        tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V8),
        use_dcdc: true,
        rx_boost: true, // boost RX gain for better sensitivity
    };

    let radio_kind = Sx126x::new(spi_device, iv, config);
    let mut lora = LoRa::new(radio_kind, false, Delay).await.unwrap();

    checkpoint(2);

    // Configure: 910.525 MHz, BW 62.5 kHz, SF 7, CR 4/5
    let mod_params = lora
        .create_modulation_params(
            lora_phy::mod_params::SpreadingFactor::_7,
            lora_phy::mod_params::Bandwidth::_62KHz,
            lora_phy::mod_params::CodingRate::_4_5,
            910_525_000,
        )
        .unwrap();

    let rx_pkt_params = lora
        .create_rx_packet_params(8, false, 255, true, false, &mod_params)
        .unwrap();

    checkpoint(3);

    // Write startup banner to USB serial
    cdc_write(&mut cdc, b"\r\n=== RAK4631 LoRa RX ===\r\n").await;
    cdc_write(&mut cdc, b"910.525 MHz / SF7 / BW62.5kHz / CR4_5\r\n").await;
    cdc_write(&mut cdc, b"Listening...\r\n\r\n").await;

    // ---- RX loop ----
    let mut rx_buf = [0u8; 255];
    let mut pkt_count: u32 = 0;

    loop {
        blue.set_high(); // blue = listening

        lora.prepare_for_rx(RxMode::Continuous, &mod_params, &rx_pkt_params)
            .await
            .unwrap();

        match lora.rx(&rx_pkt_params, &mut rx_buf).await {
            Ok((len, status)) => {
                pkt_count += 1;
                blue.set_low();
                green.set_high();

                let data = &rx_buf[..len as usize];

                // Header line
                let mut sb = SmallBuf::new();
                let _ = write!(
                    sb,
                    "[{}] len={} rssi={} snr={}\r\n",
                    pkt_count, len, status.rssi, status.snr
                );
                cdc_write(&mut cdc, sb.as_bytes()).await;

                // Hex dump — build in a larger stack buffer, send chunked
                let mut hex_buf = [0u8; 776]; // "  hex: " + 255*3 + "\r\n"
                let mut pos = 0;
                for &b in b"  hex: " {
                    hex_buf[pos] = b;
                    pos += 1;
                }
                for &b in data {
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    hex_buf[pos] = HEX[(b >> 4) as usize];
                    hex_buf[pos + 1] = HEX[(b & 0xf) as usize];
                    hex_buf[pos + 2] = b' ';
                    pos += 3;
                }
                hex_buf[pos] = b'\r';
                hex_buf[pos + 1] = b'\n';
                pos += 2;
                cdc_write(&mut cdc, &hex_buf[..pos]).await;

                // ASCII representation
                let mut asc_buf = [0u8; 266]; // "  asc: " + 255 + "\r\n\r\n"
                let mut pos = 0;
                for &b in b"  asc: " {
                    asc_buf[pos] = b;
                    pos += 1;
                }
                for &b in data {
                    asc_buf[pos] = if b >= 0x20 && b < 0x7f { b } else { b'.' };
                    pos += 1;
                }
                for &b in b"\r\n\r\n" {
                    asc_buf[pos] = b;
                    pos += 1;
                }
                cdc_write(&mut cdc, &asc_buf[..pos]).await;

                // Green flash for each received packet
                Timer::after_millis(100).await;
                green.set_low();
            }
            Err(_e) => {
                blue.set_low();
                // Brief red-ish indication (both LEDs)
                green.set_high();
                blue.set_high();
                Timer::after_millis(50).await;
                green.set_low();
                blue.set_low();

                cdc_write(&mut cdc, b"[RX error]\r\n").await;
                Timer::after_millis(500).await;
            }
        }
    }
}
