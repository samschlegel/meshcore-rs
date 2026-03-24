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
use meshcore_core::crypto::encrypt_then_mac;
use meshcore_core::header::{PacketHeader, PayloadType, PayloadVersion, RouteType};
use meshcore_core::packet::Packet;
use sha2::{Digest, Sha256};
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

    // MeshCore uses 16-symbol preamble (not the default 8)
    let rx_pkt_params = lora
        .create_rx_packet_params(16, false, 255, true, false, &mod_params)
        .unwrap();

    let mut tx_pkt_params = lora
        .create_tx_packet_params(16, false, true, false, &mod_params)
        .unwrap();

    checkpoint(3);

    // Write startup banner to USB serial
    cdc_write(&mut cdc, b"\r\n=== RAK4631 LoRa TX/RX ===\r\n").await;
    cdc_write(&mut cdc, b"910.525 MHz / SF7 / BW62.5kHz / CR4_5 / preamble=16\r\n").await;

    // ---- TX: send GrpTxt to #meshcore-rs channel on startup ----
    //
    // Hashtag channel key derivation: SHA256("#meshcore-rs") → first 16 bytes
    // Any MeshCore device with the #meshcore-rs channel will receive this.
    let channel_name = b"#meshcore-rs";
    let mut channel_secret = [0u8; 32]; // 16 bytes active, rest zero
    {
        let mut hasher = Sha256::new();
        hasher.update(channel_name);
        let hash = hasher.finalize();
        channel_secret[..16].copy_from_slice(&hash[..16]);
    }

    // Channel hash = first byte of SHA256(channel_secret[0..16])
    let channel_hash = {
        let mut hasher = Sha256::new();
        hasher.update(&channel_secret[..16]);
        let result = hasher.finalize();
        result[0]
    };

    // Plaintext: [timestamp(4B LE)][txt_type(1B)][message]
    // Message format: "sender_name: text"
    let message = b"meshcore-rs: hello from Rust!";
    // No RTC — use seconds since boot as a placeholder timestamp
    let timestamp: u32 = (embassy_time::Instant::now().as_millis() / 1000) as u32;
    let mut plaintext = [0u8; 128];
    plaintext[..4].copy_from_slice(&timestamp.to_le_bytes());
    plaintext[4] = 0x00; // txt_type=0 (plain text), attempt=0
    plaintext[5..5 + message.len()].copy_from_slice(message);
    let plaintext_len = 5 + message.len();

    // Encrypt-then-MAC: output = [MAC(2B)][ciphertext]
    let mut encrypted = [0u8; 184];
    let enc_len = encrypt_then_mac(&channel_secret, &mut encrypted, &plaintext[..plaintext_len]);

    // GrpTxt payload: [channel_hash(1B)][MAC(2B)][ciphertext]
    let mut grp_payload = [0u8; 184];
    grp_payload[0] = channel_hash;
    grp_payload[1..1 + enc_len].copy_from_slice(&encrypted[..enc_len]);
    let payload_len = 1 + enc_len;

    // Build wire packet: Flood + GrpTxt + V1 = 0x15
    let header_byte: u8 = PacketHeader {
        route_type: RouteType::Flood,
        payload_type: PayloadType::GrpTxt,
        version: PayloadVersion::V1,
    }
    .into();

    let mut grp_pkt = Packet::new();
    grp_pkt.header = header_byte;
    grp_pkt.set_path_hash_size_and_count(1, 0);
    let _ = grp_pkt
        .payload
        .extend_from_slice(&grp_payload[..payload_len]);

    let mut wire_buf = [0u8; 255];
    let wire_len = grp_pkt.write_to(&mut wire_buf);

    cdc_write(&mut cdc, b"TX GrpTxt #meshcore-rs: ").await;
    let mut sb = SmallBuf::new();
    let _ = write!(sb, "len={} ch=0x{:02x}\r\n", wire_len, channel_hash);
    cdc_write(&mut cdc, sb.as_bytes()).await;
    cdc_write(&mut cdc, b"  key=SHA256(\"#meshcore-rs\")[0..16]\r\n").await;

    green.set_high();
    lora.prepare_for_tx(&mod_params, &mut tx_pkt_params, 20, &wire_buf[..wire_len])
        .await
        .unwrap();
    lora.tx().await.unwrap();
    green.set_low();

    cdc_write(&mut cdc, b"TX done. Listening...\r\n\r\n").await;

    // ---- RX loop ----
    // Prepare once — in continuous mode the radio stays in RX after each packet.
    // rx() uses DIO1 async wait (GPIOTE interrupt), so the CPU sleeps between packets.
    lora.prepare_for_rx(RxMode::Continuous, &mod_params, &rx_pkt_params)
        .await
        .unwrap();

    let mut rx_buf = [0u8; 255];
    let mut pkt_count: u32 = 0;

    loop {
        blue.set_high(); // blue = listening

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
                let mut asc_buf = [0u8; 266]; // "  asc: " + 255 + "\r\n"
                let mut pos = 0;
                for &b in b"  asc: " {
                    asc_buf[pos] = b;
                    pos += 1;
                }
                for &b in data {
                    asc_buf[pos] = if b >= 0x20 && b < 0x7f { b } else { b'.' };
                    pos += 1;
                }
                for &b in b"\r\n" {
                    asc_buf[pos] = b;
                    pos += 1;
                }
                cdc_write(&mut cdc, &asc_buf[..pos]).await;

                // Parse as MeshCore packet
                let mut pkt = Packet::new();
                if pkt.read_from(data) {
                    pkt.snr = status.snr as i8;
                    let mut sb = SmallBuf::new();
                    match pkt.parsed_header() {
                        Ok(hdr) => {
                            let _ = write!(
                                sb,
                                "  pkt: {:?}/{:?} v{}",
                                hdr.route_type, hdr.payload_type, hdr.version as u8
                            );
                        }
                        Err(_) => {
                            let _ = write!(sb, "  pkt: hdr=0x{:02x} (invalid)", pkt.header);
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
                } else {
                    cdc_write(&mut cdc, b"  pkt: parse failed\r\n").await;
                }
                cdc_write(&mut cdc, b"\r\n").await;

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
