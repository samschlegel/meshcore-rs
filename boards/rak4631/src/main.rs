//! RAK4631 Dispatcher smoke test — validates M3 on real hardware.
//!
//! On boot:
//!   1. Initialize SX1262 via meshcore-radio's Sx1262Radio (Radio trait)
//!   2. Create a Dispatcher that owns the radio
//!   3. Submit a GrpTxt to #meshcore-rs via TX channel
//!   4. Dispatcher handles TX scheduling, duty cycle, CAD
//!   5. Received packets are delivered via RX channel and printed to USB serial
//!
//! LED patterns:
//!   1 green blink  = entered main
//!   2 green blinks = SPI + radio init done
//!   3 green blinks = dispatcher running
//!   green flash    = packet received (from RX channel)
//!   blue on        = dispatcher active
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
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Delay, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::UsbDevice;
use embedded_hal_bus::spi::ExclusiveDevice;
use lora_phy::iv::GenericSx126xInterfaceVariant;
use lora_phy::sx126x::{self, Sx126x, TcxoCtrlVoltage};
use lora_phy::LoRa;
use meshcore_core::crypto::encrypt_then_mac;
use meshcore_core::header::{PacketHeader, PayloadType, PayloadVersion, RouteType};
use meshcore_core::packet::Packet;
use meshcore_dispatch::types::{DispatcherConfig, RxPacket, TxRequest};
use meshcore_dispatch::Dispatcher;
use meshcore_radio::radio::RadioConfig;
use meshcore_radio::rng::Rng;
use meshcore_radio::sx1262::Sx1262Radio;
use meshcore_radio::Radio;
use sha2::{Digest, Sha256};
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    TWISPI1 => spim::InterruptHandler<peripherals::TWISPI1>;
    USBD => embassy_nrf::usb::InterruptHandler<peripherals::USBD>;
    CLOCK_POWER => embassy_nrf::usb::vbus_detect::InterruptHandler;
});

// ---- Static channels for Dispatcher ↔ User task communication ----
static TX_CHANNEL: Channel<CriticalSectionRawMutex, TxRequest, 4> = Channel::new();
static RX_CHANNEL: Channel<CriticalSectionRawMutex, RxPacket, 4> = Channel::new();

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

// ---- Simple RNG using a linear congruential generator ----
// Good enough for CAD jitter; not crypto.
struct SimpleRng {
    state: u32,
}

impl SimpleRng {
    fn new(seed: u32) -> Self {
        Self { state: if seed == 0 { 1 } else { seed } }
    }
}

impl Rng for SimpleRng {
    fn random(&mut self, dest: &mut [u8]) {
        for byte in dest.iter_mut() {
            // LCG: state = state * 1103515245 + 12345
            self.state = self.state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            *byte = (self.state >> 16) as u8;
        }
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

struct SmallBuf {
    buf: [u8; 64],
    pos: usize,
}

impl SmallBuf {
    fn new() -> Self {
        Self { buf: [0u8; 64], pos: 0 }
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

/// User task: submits TX on boot, then reads RX packets and prints to USB serial.
#[embassy_executor::task]
async fn user_task(mut cdc: CdcAcmClass<'static, MyUsbDriver>) {
    // Give USB a moment to enumerate
    Timer::after_millis(500).await;

    cdc_write(&mut cdc, b"\r\n=== RAK4631 Dispatcher Test ===\r\n").await;
    cdc_write(&mut cdc, b"910.525 MHz / SF7 / BW62.5kHz / CR4_5 / preamble=16\r\n").await;
    cdc_write(&mut cdc, b"Using meshcore-dispatch Dispatcher\r\n\r\n").await;

    // ---- Build GrpTxt packet for #meshcore-rs ----
    let channel_name = b"#meshcore-rs";
    let mut channel_secret = [0u8; 32];
    {
        let mut hasher = Sha256::new();
        hasher.update(channel_name);
        let hash = hasher.finalize();
        channel_secret[..16].copy_from_slice(&hash[..16]);
    }

    let channel_hash = {
        let mut hasher = Sha256::new();
        hasher.update(&channel_secret[..16]);
        let result = hasher.finalize();
        result[0]
    };

    let message = b"meshcore-rs: hello from Dispatcher!";
    let timestamp: u32 = (embassy_time::Instant::now().as_millis() / 1000) as u32;
    let mut plaintext = [0u8; 128];
    plaintext[..4].copy_from_slice(&timestamp.to_le_bytes());
    plaintext[4] = 0x00;
    plaintext[5..5 + message.len()].copy_from_slice(message);
    let plaintext_len = 5 + message.len();

    let mut encrypted = [0u8; 184];
    let enc_len = encrypt_then_mac(&channel_secret, &mut encrypted, &plaintext[..plaintext_len]);

    let mut grp_payload = [0u8; 184];
    grp_payload[0] = channel_hash;
    grp_payload[1..1 + enc_len].copy_from_slice(&encrypted[..enc_len]);
    let payload_len = 1 + enc_len;

    let header_byte: u8 = PacketHeader {
        route_type: RouteType::Flood,
        payload_type: PayloadType::GrpTxt,
        version: PayloadVersion::V1,
    }
    .into();

    let mut grp_pkt = Packet::new();
    grp_pkt.header = header_byte;
    grp_pkt.set_path_hash_size_and_count(1, 0);
    let _ = grp_pkt.payload.extend_from_slice(&grp_payload[..payload_len]);

    // Submit TX request via channel — Dispatcher will handle scheduling + CAD + duty cycle
    let mut sb = SmallBuf::new();
    let _ = write!(sb, "TX GrpTxt #meshcore-rs: len={} ch=0x{:02x}\r\n", grp_pkt.wire_len(), channel_hash);
    cdc_write(&mut cdc, sb.as_bytes()).await;

    TX_CHANNEL
        .send(TxRequest {
            packet: grp_pkt,
            priority: 0,
            delay_ms: 0,
        })
        .await;

    cdc_write(&mut cdc, b"TX queued. Listening for RX...\r\n\r\n").await;

    // ---- RX loop: read from channel, print to USB ----
    let mut pkt_count: u32 = 0;

    loop {
        let rx_pkt = RX_CHANNEL.receive().await;
        pkt_count += 1;

        let pkt = &rx_pkt.packet;

        // Header line
        let mut sb = SmallBuf::new();
        let _ = write!(
            sb,
            "[{}] rssi={:.0} snr={:.1}",
            pkt_count, rx_pkt.rssi, rx_pkt.snr
        );
        cdc_write(&mut cdc, sb.as_bytes()).await;

        // Parse header
        let mut sb = SmallBuf::new();
        match pkt.parsed_header() {
            Ok(hdr) => {
                let _ = write!(
                    sb,
                    " {:?}/{:?} v{}",
                    hdr.route_type, hdr.payload_type, hdr.version as u8
                );
            }
            Err(_) => {
                let _ = write!(sb, " hdr=0x{:02x}(invalid)", pkt.header);
            }
        }
        cdc_write(&mut cdc, sb.as_bytes()).await;

        let mut sb = SmallBuf::new();
        let _ = write!(
            sb,
            " path={}/{} payload={}B",
            pkt.path_hash_count(),
            pkt.path_hash_size(),
            pkt.payload.len()
        );
        cdc_write(&mut cdc, sb.as_bytes()).await;

        if pkt.has_transport_codes() {
            let mut sb = SmallBuf::new();
            let _ = write!(
                sb,
                " tc=[{:04x},{:04x}]",
                pkt.transport_codes[0], pkt.transport_codes[1]
            );
            cdc_write(&mut cdc, sb.as_bytes()).await;
        }
        cdc_write(&mut cdc, b"\r\n").await;

        // Hex dump
        let payload = pkt.payload.as_slice();
        let mut hex_buf = [0u8; 128];
        let mut pos = 0;
        for &b in b"  hex: " {
            hex_buf[pos] = b;
            pos += 1;
        }
        for &b in &payload[..payload.len().min(32)] {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            hex_buf[pos] = HEX[(b >> 4) as usize];
            hex_buf[pos + 1] = HEX[(b & 0xf) as usize];
            hex_buf[pos + 2] = b' ';
            pos += 3;
        }
        if payload.len() > 32 {
            for &b in b"..." {
                hex_buf[pos] = b;
                pos += 1;
            }
        }
        hex_buf[pos] = b'\r';
        hex_buf[pos + 1] = b'\n';
        pos += 2;
        cdc_write(&mut cdc, &hex_buf[..pos]).await;

        cdc_write(&mut cdc, b"\r\n").await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    checkpoint(1);

    let p = embassy_nrf::init(embassy_nrf::config::Config::default());

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

    let mut usb_config = embassy_usb::Config::new(0x1209, 0x0001);
    usb_config.manufacturer = Some("meshcore-rs");
    usb_config.product = Some("RAK4631 Dispatcher");
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
    let cdc = CdcAcmClass::new(&mut builder, cdc_state, 64);

    let usb = builder.build();
    spawner.spawn(usb_task(usb).unwrap());

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
        rx_boost: true,
    };

    let radio_kind = Sx126x::new(spi_device, iv, config);
    let lora = LoRa::new(radio_kind, false, Delay).await.unwrap();

    checkpoint(2);

    // Wrap in our Radio trait
    let mut radio = Sx1262Radio::new(lora);

    // Configure: 910.525 MHz, BW 62.5 kHz, SF 7, CR 4/5, 16-symbol preamble
    let radio_config = RadioConfig {
        frequency_mhz: 910.525,
        bandwidth_khz: 62.5,
        spreading_factor: 7,
        coding_rate: 5,
        tx_power: 20,
        preamble_symbols: 16,
    };
    radio.configure(&radio_config).await.unwrap();

    // Create Dispatcher
    let rng = SimpleRng::new(0xDEAD_BEEF);
    let mut dispatcher = Dispatcher::<_, _, 4, 4>::new(
        radio,
        rng,
        DispatcherConfig::default(),
    );

    // Spawn user task (handles TX submission + RX printing)
    spawner.spawn(user_task(cdc).unwrap());

    checkpoint(3);
    blue.set_high(); // Blue = dispatcher running

    // Run the dispatcher event loop (never returns).
    // This owns the radio and handles all TX/RX scheduling.
    dispatcher.run(&TX_CHANNEL, &RX_CHANNEL).await;
}
