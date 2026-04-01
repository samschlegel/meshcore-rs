//! RAK4631 channel command responder — M4a firmware.
//!
//! Listens on the `#meshcore-rs` channel and responds to commands:
//!   - `!help`   → lists available commands
//!   - `!ping`   → replies with `pong`
//!   - `!status` → replies with uptime and dispatcher stats
//!   - `!path`   → echoes path hashes from the incoming packet
//!
//! On boot:
//!   1. Initialize SX1262 via meshcore-radio's Sx1262Radio (Radio trait)
//!   2. Create a Dispatcher that owns the radio
//!   3. Submit a GrpTxt greeting to #meshcore-rs via TX channel
//!   4. RX loop decodes GrpTxt, matches commands, sends responses
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
use core::sync::atomic::{compiler_fence, Ordering};

use embassy_executor::Spawner;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::spim::{self, Spim};
use embassy_nrf::usb::vbus_detect::HardwareVbusDetect;
use embassy_nrf::usb::Driver as UsbDriver;
use embassy_nrf::{bind_interrupts, peripherals};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embassy_time::{Delay, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::UsbDevice;
use embedded_hal_bus::spi::ExclusiveDevice;
use lora_phy::iv::GenericSx126xInterfaceVariant;
use lora_phy::sx126x::{self, Sx126x, TcxoCtrlVoltage};
use lora_phy::LoRa;
use meshcore_core::dedup::PacketDedup;
use meshcore_core::grp_txt::{decode_grp_txt, encode_grp_txt, matches_channel};
use meshcore_core::header::PayloadType;
use meshcore_dispatch::types::{DispatcherConfig, RxPacket, TxRequest};
use meshcore_dispatch::Dispatcher;
use meshcore_radio::radio::RadioConfig;
use meshcore_radio::rng::Rng;
use meshcore_radio::rtc::RtcClock;
use meshcore_radio::sx1262::Sx1262Radio;
use meshcore_radio::VolatileRtcClock;
use meshcore_radio::Radio;
use sha2::{Digest, Sha256};
use static_cell::StaticCell;

/// Build-time UNIX epoch, baked in by build.rs.
/// Gives the RTC a reasonable starting point even on first boot.
const BUILD_EPOCH: u32 = const_parse_u32(env!("BUILD_EPOCH"));

const fn const_parse_u32(s: &str) -> u32 {
    let bytes = s.as_bytes();
    let mut result: u32 = 0;
    let mut i = 0;
    while i < bytes.len() {
        result = result * 10 + (bytes[i] - b'0') as u32;
        i += 1;
    }
    result
}

type SharedRtc = Mutex<CriticalSectionRawMutex, VolatileRtcClock>;

bind_interrupts!(struct Irqs {
    TWISPI1 => spim::InterruptHandler<peripherals::TWISPI1>;
    USBD => embassy_nrf::usb::InterruptHandler<peripherals::USBD>;
    CLOCK_POWER => embassy_nrf::usb::vbus_detect::InterruptHandler;
});

// ---- Static channels for Dispatcher ↔ User task communication ----
static TX_CHANNEL: Channel<CriticalSectionRawMutex, TxRequest, 4> = Channel::new();
static RX_CHANNEL: Channel<CriticalSectionRawMutex, RxPacket, 4> = Channel::new();

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
    for _ in 0..500_000u32 {
        cortex_m::asm::nop();
    }
}

fn raw_delay_long() {
    for _ in 0..2_000_000u32 {
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
    compiler_fence(Ordering::SeqCst);
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
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
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

async fn cdc_write(cdc: &mut CdcAcmClass<'static, MyUsbDriver>, data: &[u8]) {
    for chunk in data.chunks(64) {
        let _ = cdc.write_packet(chunk).await;
    }
}

struct FmtBuf<const N: usize> {
    buf: [u8; N],
    pos: usize,
}

impl<const N: usize> FmtBuf<N> {
    fn new() -> Self {
        Self { buf: [0u8; N], pos: 0 }
    }
    fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.pos]
    }
    fn push(&mut self, b: u8) {
        if self.pos < N {
            self.buf[self.pos] = b;
            self.pos += 1;
        }
    }
}

impl<const N: usize> core::fmt::Write for FmtBuf<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let n = bytes.len().min(N - self.pos);
        self.buf[self.pos..self.pos + n].copy_from_slice(&bytes[..n]);
        self.pos += n;
        Ok(())
    }
}

/// Node name used as sender in channel messages.
const NODE_NAME: &[u8] = b"rak4631";

/// Derive channel secret and hash from a channel name.
/// Secret = SHA256(name)[0..16] zero-extended to 32 bytes.
/// Hash = SHA256(secret[0..16])[0].
fn derive_channel_key(name: &[u8]) -> ([u8; 32], u8) {
    let mut secret = [0u8; 32];
    let hash = Sha256::new().chain_update(name).finalize();
    secret[..16].copy_from_slice(&hash[..16]);

    let channel_hash = Sha256::new().chain_update(&secret[..16]).finalize()[0];
    (secret, channel_hash)
}

/// Format a command response message as `"<node>: <body>"`.
fn format_response<const N: usize>(body: &[u8]) -> FmtBuf<N> {
    let mut buf = FmtBuf::<N>::new();
    for &b in NODE_NAME {
        buf.push(b);
    }
    buf.push(b':');
    buf.push(b' ');
    for &b in body {
        buf.push(b);
    }
    buf
}

/// User task: sends greeting on boot, then handles channel commands.
#[embassy_executor::task]
async fn user_task(mut cdc: CdcAcmClass<'static, MyUsbDriver>, rtc: &'static SharedRtc) {
    // Give USB a moment to enumerate
    Timer::after_millis(500).await;

    cdc_write(&mut cdc, b"\r\n=== RAK4631 Channel Responder ===\r\n").await;
    cdc_write(
        &mut cdc,
        b"910.525 MHz / SF7 / BW62.5kHz / CR4_5 / preamble=16\r\n",
    )
    .await;

    // Print RTC status
    {
        let epoch = rtc.lock().await.get_time();
        let mut sb = FmtBuf::<80>::new();
        let _ = write!(sb, "RTC epoch: {} (build: {})\r\n", epoch, BUILD_EPOCH);
        cdc_write(&mut cdc, sb.as_bytes()).await;
    }

    let (channel_secret, channel_hash) = derive_channel_key(b"#meshcore-rs");

    {
        let mut sb = FmtBuf::<64>::new();
        let _ = write!(sb, "Channel: #meshcore-rs (hash=0x{:02x})\r\n\r\n", channel_hash);
        cdc_write(&mut cdc, sb.as_bytes()).await;
    }

    // Dedup table — 128 entries matches C's SimpleMeshTables
    let mut dedup = PacketDedup::<128>::new();

    // Send boot greeting (pre-register in dedup to prevent self-echo)
    let timestamp = rtc.lock().await.get_time();
    let greeting = format_response::<128>(b"online");
    if let Some(pkt) = encode_grp_txt(&channel_secret, channel_hash, timestamp, greeting.as_bytes()) {
        let mut sb = FmtBuf::<64>::new();
        let _ = write!(sb, "TX greeting: len={}\r\n", pkt.wire_len());
        cdc_write(&mut cdc, sb.as_bytes()).await;

        dedup.has_seen(&pkt); // pre-register to avoid self-echo
        TX_CHANNEL
            .send(TxRequest {
                packet: pkt,
                priority: 0,
                delay_ms: 0,
            })
            .await;
    }

    cdc_write(&mut cdc, b"Listening for commands...\r\n\r\n").await;

    let boot_time = embassy_time::Instant::now();
    let mut pkt_count: u32 = 0;

    loop {
        let rx_pkt = RX_CHANNEL.receive().await;
        pkt_count += 1;

        let pkt = &rx_pkt.packet;

        // Dedup check — skip packets we've already seen
        if dedup.has_seen(pkt) {
            let mut out = FmtBuf::<64>::new();
            let _ = write!(
                out,
                "[{}] dup (flood={} direct={})\r\n",
                pkt_count,
                dedup.flood_dups(),
                dedup.direct_dups()
            );
            cdc_write(&mut cdc, out.as_bytes()).await;
            continue;
        }

        // Log every new packet
        {
            let mut out = FmtBuf::<128>::new();
            let _ = write!(out, "[{}] rssi={:.0} snr={:.1}", pkt_count, rx_pkt.rssi, rx_pkt.snr);
            match pkt.parsed_header() {
                Ok(hdr) => {
                    let _ = write!(out, " {:?}/{:?}", hdr.route_type, hdr.payload_type);
                }
                Err(_) => {
                    let _ = write!(out, " hdr=0x{:02x}", pkt.header);
                }
            }
            let _ = write!(out, " payload={}B\r\n", pkt.payload.len());
            cdc_write(&mut cdc, out.as_bytes()).await;
        }

        // Only process GrpTxt packets for our channel
        let is_grp_txt = pkt
            .payload_type()
            .map(|pt| pt == PayloadType::GrpTxt)
            .unwrap_or(false);
        if !is_grp_txt || !matches_channel(pkt.payload.as_slice(), channel_hash) {
            continue;
        }

        // Decrypt the message
        let mut scratch = [0u8; 256];
        let decoded = match decode_grp_txt(&channel_secret, pkt.payload.as_slice(), &mut scratch) {
            Some(d) => d,
            None => {
                cdc_write(&mut cdc, b"  (decrypt failed)\r\n").await;
                continue;
            }
        };

        // Log the decoded message
        {
            let mut out = FmtBuf::<200>::new();
            let _ = write!(out, "  msg: {}\r\n", decoded.message);
            cdc_write(&mut cdc, out.as_bytes()).await;
        }

        // Extract the command: look for "!cmd" anywhere after the ": " separator.
        let body = match decoded.message.find(": ") {
            Some(pos) => decoded.message[pos + 2..].trim_start(),
            None => decoded.message.trim_start(),
        };

        // Match commands and build response
        let mut resp_buf = FmtBuf::<160>::new();
        let node = core::str::from_utf8(NODE_NAME).unwrap_or("?");
        let has_response = if body.starts_with("!help") {
            let _ = write!(resp_buf, "{}: !help !ping !status !path", node);
            true
        } else if body.starts_with("!ping") {
            for &b in NODE_NAME {
                resp_buf.push(b);
            }
            let _ = write!(resp_buf, ": pong");
            true
        } else if body.starts_with("!status") {
            let uptime_secs = (embassy_time::Instant::now() - boot_time).as_secs();
            let _ = write!(
                resp_buf,
                "{}: up {}s, rx={} tx={}",
                node,
                uptime_secs,
                pkt_count,
                0u32, // TODO: expose dispatcher stats
            );
            true
        } else if body.starts_with("!path") {
            let _ = write!(
                resp_buf,
                "{}: path hashes={}/{}B [",
                node,
                pkt.path_hash_count(),
                pkt.path_hash_size(),
            );
            let hash_size = pkt.path_hash_size() as usize;
            let hash_count = pkt.path_hash_count() as usize;
            let path_bytes = pkt.path.as_slice();
            for i in 0..hash_count {
                if i > 0 {
                    let _ = write!(resp_buf, ",");
                }
                let start = i * hash_size;
                let end = (start + hash_size).min(path_bytes.len());
                for &b in &path_bytes[start..end] {
                    let _ = write!(resp_buf, "{:02x}", b);
                }
            }
            let _ = write!(resp_buf, "]");
            true
        } else {
            false
        };

        if has_response {
            let ts = rtc.lock().await.get_time();
            if let Some(reply) = encode_grp_txt(&channel_secret, channel_hash, ts, resp_buf.as_bytes()) {
                {
                    let mut out = FmtBuf::<200>::new();
                    let _ = write!(
                        out,
                        "  TX reply: {} (len={})\r\n",
                        core::str::from_utf8(resp_buf.as_bytes()).unwrap_or("?"),
                        reply.wire_len()
                    );
                    cdc_write(&mut cdc, out.as_bytes()).await;
                }

                dedup.has_seen(&reply); // pre-register to avoid self-echo
                TX_CHANNEL
                    .send(TxRequest {
                        packet: reply,
                        priority: 0,
                        delay_ms: 0,
                    })
                    .await;
            }
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_nrf::init(embassy_nrf::config::Config::default());

    // Initialize the LEDs
    let _green = Output::new(p.P1_03, Level::Low, OutputDrive::Standard);
    let mut blue = Output::new(p.P1_04, Level::Low, OutputDrive::Standard);

    checkpoint(1);

    // ---- RTC initialization ----
    static RTC: StaticCell<SharedRtc> = StaticCell::new();
    let rtc = RTC.init(Mutex::new(VolatileRtcClock::new(BUILD_EPOCH)));

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
    let mut dispatcher = Dispatcher::<_, _, 4, 4>::new(radio, rng, DispatcherConfig::default());

    // Spawn user task (handles TX submission + RX printing)
    spawner.spawn(user_task(cdc, rtc).unwrap());

    checkpoint(3);
    blue.set_high(); // Blue = dispatcher running

    // Run the dispatcher event loop (never returns).
    // This owns the radio and handles all TX/RX scheduling.
    dispatcher.run(&TX_CHANNEL, &RX_CHANNEL).await;
}
