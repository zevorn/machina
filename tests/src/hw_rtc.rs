use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_i2c::{I2cEvent, I2cSlave};
use machina_hw_rtc::ds1338::Ds1338;
use machina_hw_rtc::goldfish_rtc::{GoldfishRtc, GoldfishRtcMmio};
use machina_hw_rtc::ls7a_rtc::{Ls7aRtc, Ls7aRtcMmio};
use machina_hw_rtc::pl031::{Pl031, Pl031Mmio};
use machina_memory::region::MmioOps;

// -- Test helpers --

struct TestIrqSink {
    level: AtomicBool,
}

impl TestIrqSink {
    fn new() -> Self {
        Self {
            level: AtomicBool::new(false),
        }
    }

    fn level(&self) -> bool {
        self.level.load(Ordering::Relaxed)
    }
}

impl IrqSink for TestIrqSink {
    fn set_irq(&self, _irq: u32, level: bool) {
        self.level.store(level, Ordering::Relaxed);
    }
}

// -- PL031 tests --

#[test]
fn test_pl031_defaults() {
    let pl031 = Arc::new(Pl031::new());
    let mmio = Pl031Mmio(Arc::clone(&pl031));

    assert_eq!(mmio.read(0x00, 4), 0); // DR
    assert_eq!(mmio.read(0x04, 4), 0); // MR
    assert_eq!(mmio.read(0x08, 4), 0); // LR
    assert_eq!(mmio.read(0x0C, 4), 1); // CR = always 1
    assert_eq!(mmio.read(0x10, 4), 0); // IMSC
    assert_eq!(mmio.read(0x14, 4), 0); // RIS
    assert_eq!(mmio.read(0x18, 4), 0); // MIS
    assert_eq!(mmio.read(0x1C, 4), 0); // ICR (write-only, reads 0)
}

#[test]
fn test_pl031_id_registers() {
    let pl031 = Arc::new(Pl031::new());
    let mmio = Pl031Mmio(Arc::clone(&pl031));

    assert_eq!(mmio.read(0xFE0, 4), 0x31);
    assert_eq!(mmio.read(0xFE4, 4), 0x10);
    assert_eq!(mmio.read(0xFE8, 4), 0x14);
    assert_eq!(mmio.read(0xFEC, 4), 0x00);
    assert_eq!(mmio.read(0xFF0, 4), 0x0D);
    assert_eq!(mmio.read(0xFF4, 4), 0xF0);
    assert_eq!(mmio.read(0xFF8, 4), 0x05);
    assert_eq!(mmio.read(0xFFC, 4), 0xB1);
}

#[test]
fn test_pl031_write_lr_sets_time() {
    let pl031 = Arc::new(Pl031::new());
    let mmio = Pl031Mmio(Arc::clone(&pl031));

    mmio.write(0x08, 4, 100); // LR = 100
    assert_eq!(mmio.read(0x00, 4), 100); // DR = LR
    assert_eq!(mmio.read(0x08, 4), 100); // LR stored
}

#[test]
fn test_pl031_match_alarm_irq() {
    let pl031 = Arc::new(Pl031::new());
    let mmio = Pl031Mmio(Arc::clone(&pl031));
    let sink = Arc::new(TestIrqSink::new());
    pl031.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    // Set time
    mmio.write(0x08, 4, 0); // LR = 0
                            // Set match to 10
    mmio.write(0x04, 4, 10); // MR = 10
                             // Enable interrupt
    mmio.write(0x10, 4, 1); // IMSC = 1

    assert!(!sink.level());

    // Tick 10 seconds
    pl031.tick(10);

    assert!(sink.level(), "IRQ should fire when DR reaches MR");
}

#[test]
fn test_pl031_icr_clears_interrupt() {
    let pl031 = Arc::new(Pl031::new());
    let mmio = Pl031Mmio(Arc::clone(&pl031));
    let sink = Arc::new(TestIrqSink::new());
    pl031.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    mmio.write(0x08, 4, 0); // LR = 0
    mmio.write(0x04, 4, 5); // MR = 5
    mmio.write(0x10, 4, 1); // IMSC = 1

    pl031.tick(5);
    assert!(sink.level());

    // Write ICR to clear
    mmio.write(0x1C, 4, 1);
    assert!(!sink.level());
    // RIS should be 0 after clear
    assert_eq!(mmio.read(0x14, 4), 0);
}

#[test]
fn test_pl031_im_masks_irq() {
    let pl031 = Arc::new(Pl031::new());
    let mmio = Pl031Mmio(Arc::clone(&pl031));
    let sink = Arc::new(TestIrqSink::new());
    pl031.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    mmio.write(0x08, 4, 0); // LR = 0
    mmio.write(0x04, 4, 5); // MR = 5
                            // IM stays at 0 — IRQ masked

    pl031.tick(5);
    assert!(!sink.level(), "IRQ should stay low when masked");
    // But RIS should be set
    assert_eq!(mmio.read(0x14, 4), 1);
    // MIS should be 0 (masked)
    assert_eq!(mmio.read(0x18, 4), 0);
}

#[test]
fn test_pl031_write_cr_ignored() {
    let pl031 = Arc::new(Pl031::new());
    let mmio = Pl031Mmio(Arc::clone(&pl031));

    mmio.write(0x0C, 4, 0xDEAD);
    // CR always reads 1
    assert_eq!(mmio.read(0x0C, 4), 1);
}

#[test]
fn test_pl031_read_only_regs_ignore_write() {
    let pl031 = Arc::new(Pl031::new());
    let mmio = Pl031Mmio(Arc::clone(&pl031));

    // DR, MIS, RIS are read-only
    mmio.write(0x00, 4, 0xFF); // DR
    mmio.write(0x18, 4, 0xFF); // MIS
    mmio.write(0x14, 4, 0xFF); // RIS

    assert_eq!(mmio.read(0x00, 4), 0);
    assert_eq!(mmio.read(0x18, 4), 0);
    assert_eq!(mmio.read(0x14, 4), 0);
}

#[test]
fn test_pl031_reset_runtime() {
    let pl031 = Arc::new(Pl031::new());
    let mmio = Pl031Mmio(Arc::clone(&pl031));

    mmio.write(0x08, 4, 1234); // LR
    mmio.write(0x04, 4, 5678); // MR
    mmio.write(0x10, 4, 1); // IMSC

    pl031.reset_runtime();

    assert_eq!(mmio.read(0x00, 4), 0);
    assert_eq!(mmio.read(0x04, 4), 0);
    assert_eq!(mmio.read(0x08, 4), 0);
    assert_eq!(mmio.read(0x10, 4), 0);
}

// -- Goldfish RTC tests --

#[test]
fn test_goldfish_rtc_defaults() {
    let rtc = Arc::new(GoldfishRtc::new());
    let mmio = GoldfishRtcMmio(Arc::clone(&rtc));

    assert_eq!(mmio.read(0x00, 4), 0); // TIME_LOW
    assert_eq!(mmio.read(0x04, 4), 0); // TIME_HIGH
    assert_eq!(mmio.read(0x08, 4), 0); // ALARM_LOW
    assert_eq!(mmio.read(0x0C, 4), 0); // ALARM_HIGH
    assert_eq!(mmio.read(0x10, 4), 0); // IRQ_ENABLED
    assert_eq!(mmio.read(0x18, 4), 0); // ALARM_STATUS
}

#[test]
fn test_goldfish_rtc_time_read_write() {
    let rtc = Arc::new(GoldfishRtc::new());
    let mmio = GoldfishRtcMmio(Arc::clone(&rtc));

    // Write time low only (high stays at 0)
    mmio.write(0x00, 4, 0x12345678u64);
    // Read TIME_LOW captures high bits into time_high
    let lo = mmio.read(0x00, 4);
    assert_eq!(lo, 0x12345678);
    // TIME_HIGH reads back cached high word from TIME_LOW read
    let hi = mmio.read(0x04, 4);
    assert_eq!(hi, 0); // high bits are 0 since we only wrote low bits

    // Now write time high
    mmio.write(0x04, 4, 0x9ABCDEF0u64);
    // After writing high, read TIME_LOW again to capture the new high
    let _lo = mmio.read(0x00, 4);
    let hi = mmio.read(0x04, 4);
    assert_eq!(hi, 0x9ABCDEF0);
}

#[test]
fn test_goldfish_rtc_alarm_irq() {
    let rtc = Arc::new(GoldfishRtc::new());
    let mmio = GoldfishRtcMmio(Arc::clone(&rtc));
    let sink = Arc::new(TestIrqSink::new());
    rtc.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    // Enable IRQ
    mmio.write(0x10, 4, 1);
    assert!(!sink.level());

    // Set alarm to 1000 ns from now (time starts at 0)
    mmio.write(0x08, 4, 1000); // ALARM_LOW = 1000
                               // ALARM_STATUS should be 1 (alarm running)
    assert_eq!(mmio.read(0x18, 4), 1);

    // Tick past the alarm
    rtc.tick(2000);

    assert!(sink.level(), "IRQ should fire when time >= alarm");
}

#[test]
fn test_goldfish_rtc_clear_interrupt() {
    let rtc = Arc::new(GoldfishRtc::new());
    let mmio = GoldfishRtcMmio(Arc::clone(&rtc));
    let sink = Arc::new(TestIrqSink::new());
    rtc.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    mmio.write(0x10, 4, 1);
    mmio.write(0x08, 4, 500);

    rtc.tick(1000);
    assert!(sink.level());

    // Clear interrupt
    mmio.write(0x1C, 4, 0); // CLEAR_INTERRUPT (any write clears)
    assert!(!sink.level());
}

#[test]
fn test_goldfish_rtc_clear_alarm() {
    let rtc = Arc::new(GoldfishRtc::new());
    let mmio = GoldfishRtcMmio(Arc::clone(&rtc));
    let sink = Arc::new(TestIrqSink::new());
    rtc.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    mmio.write(0x10, 4, 1);
    mmio.write(0x08, 4, 500); // Set alarm
    assert_eq!(mmio.read(0x18, 4), 1); // Alarm running

    // Clear alarm before it fires
    mmio.write(0x14, 4, 0); // CLEAR_ALARM
    assert_eq!(mmio.read(0x18, 4), 0); // Alarm not running

    rtc.tick(2000);
    assert!(!sink.level(), "IRQ should not fire after alarm cleared");
}

#[test]
fn test_goldfish_rtc_reset_runtime() {
    let rtc = Arc::new(GoldfishRtc::new());
    let mmio = GoldfishRtcMmio(Arc::clone(&rtc));

    mmio.write(0x08, 4, 1234);
    mmio.write(0x10, 4, 1);

    rtc.reset_runtime();

    assert_eq!(mmio.read(0x08, 4), 0);
    assert_eq!(mmio.read(0x10, 4), 0);
    assert_eq!(mmio.read(0x18, 4), 0);
}

// -- LS7A RTC tests --

#[test]
fn test_ls7a_rtc_defaults() {
    let rtc = Arc::new(Ls7aRtc::new());
    let mmio = Ls7aRtcMmio(Arc::clone(&rtc));

    // With ctrl=0, TOY and RTC are disabled → reads return 0
    assert_eq!(mmio.read(0x2C, 4), 0); // TOYREAD0
    assert_eq!(mmio.read(0x30, 4), 0); // TOYREAD1
    assert_eq!(mmio.read(0x40, 4), 0); // RTCCTRL
    assert_eq!(mmio.read(0x68, 4), 0); // RTCREAD0
    assert_eq!(mmio.read(0x34, 4), 0); // TOYMATCH0
    assert_eq!(mmio.read(0x6C, 4), 0); // RTCMATCH0
}

#[test]
fn test_ls7a_rtc_enable_toy() {
    let rtc = Arc::new(Ls7aRtc::new());
    let mmio = Ls7aRtcMmio(Arc::clone(&rtc));

    // Enable TOY: TOYEN | EO
    let ctrl = (1u32 << 11) | (1 << 8);
    mmio.write(0x40, 4, u64::from(ctrl));

    // With toy_offset=0, encode_toy returns:
    // day=1, mon=1 at offset 0
    let toy0 = mmio.read(0x2C, 4); // TOYREAD0
                                   // sec=0, min=0, hour=0, day=1, mon=1
    let expected: u32 =
        (0 << 4) | (0 << 10) | (0 << 16) | (1 << 21) | (1 << 26);
    assert_eq!(toy0, u64::from(expected));
}

#[test]
fn test_ls7a_rtc_write_toy_time() {
    let rtc = Arc::new(Ls7aRtc::new());
    let mmio = Ls7aRtcMmio(Arc::clone(&rtc));

    // Enable TOY
    let ctrl = (1u32 << 11) | (1 << 8);
    mmio.write(0x40, 4, u64::from(ctrl));

    // Write TOY with sec=30, min=15, hour=10, day=5, mon=3
    let val: u32 = (30 << 4) | (15 << 10) | (10 << 16) | (5 << 21) | (3 << 26);
    mmio.write(0x24, 4, u64::from(val)); // TOYWRITE0

    // Read back should match
    let toy0 = mmio.read(0x2C, 4);
    assert_eq!(toy0, u64::from(val));
}

#[test]
fn test_ls7a_rtc_rtc_counter() {
    let rtc = Arc::new(Ls7aRtc::new());
    let mmio = Ls7aRtcMmio(Arc::clone(&rtc));

    // Enable RTC
    let ctrl = (1u32 << 13) | (1 << 8);
    mmio.write(0x40, 4, u64::from(ctrl));

    assert_eq!(mmio.read(0x68, 4), 0); // RTCREAD0 starts at 0

    // Write initial RTC count
    mmio.write(0x64, 4, 100); // RTCWRTIE0 = 100
    assert_eq!(mmio.read(0x68, 4), 100);

    // Tick advances RTC
    rtc.tick(1); // 1 second = 32768 ticks
    assert_eq!(mmio.read(0x68, 4), 100 + 32768);
}

#[test]
fn test_ls7a_rtc_toy_match_irq() {
    let rtc = Arc::new(Ls7aRtc::new());
    let mmio = Ls7aRtcMmio(Arc::clone(&rtc));
    let sink = Arc::new(TestIrqSink::new());
    rtc.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    // Enable TOY
    let ctrl = (1u32 << 11) | (1 << 8);
    mmio.write(0x40, 4, u64::from(ctrl));

    assert!(!sink.level());

    // Set match to current time (toy_offset=0 gives day=1,mon=1,sec=0,...)
    let cur_toy = mmio.read(0x2C, 4);
    mmio.write(0x34, 4, cur_toy); // TOYMATCH0 = current TOY value

    // IRQ should fire immediately since match equals current
    assert!(sink.level());
}

#[test]
fn test_ls7a_rtc_rtc_match_irq() {
    let rtc = Arc::new(Ls7aRtc::new());
    let mmio = Ls7aRtcMmio(Arc::clone(&rtc));
    let sink = Arc::new(TestIrqSink::new());
    rtc.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    // Enable RTC
    let ctrl = (1u32 << 13) | (1 << 8);
    mmio.write(0x40, 4, u64::from(ctrl));

    assert!(!sink.level());

    // Set RTC match to 100 (current is 0)
    mmio.write(0x6C, 4, 100); // RTCMATCH0 = 100

    // Tick to advance past match
    rtc.tick(1); // 32768 ticks, which is > 100

    assert!(sink.level(), "IRQ should fire when RTC ticks >= match");
}

#[test]
fn test_ls7a_rtc_reset_runtime() {
    let rtc = Arc::new(Ls7aRtc::new());
    let mmio = Ls7aRtcMmio(Arc::clone(&rtc));

    let ctrl = (1u32 << 11) | (1 << 8);
    mmio.write(0x40, 4, u64::from(ctrl));
    mmio.write(0x34, 4, 1234); // TOYMATCH0

    rtc.reset_runtime();

    assert_eq!(mmio.read(0x40, 4), 0); // ctrl cleared
    assert_eq!(mmio.read(0x34, 4), 0); // match cleared
}

#[test]
fn test_ls7a_rtc_toy_disabled_ignores_write() {
    let rtc = Arc::new(Ls7aRtc::new());
    let mmio = Ls7aRtcMmio(Arc::clone(&rtc));

    // TOY disabled (ctrl=0), try to write
    mmio.write(0x24, 4, 0x12345678); // TOYWRITE0
                                     // Read should still be 0
    assert_eq!(mmio.read(0x2C, 4), 0);
}

// -- DS1338 tests --

#[test]
fn test_ds1338_i2c_address() {
    let ds = Ds1338::new(0x68);
    assert_eq!(ds.address(), 0x68);
}

#[test]
fn test_ds1338_time_capture_on_start_recv() {
    let ds = Arc::new(Ds1338::new(0x68));
    let _e = ds.event(I2cEvent::StartRecv);

    // After START_RECV, time should be captured to nvram[0..6]
    // At offset=0 (epoch), sec=0, min=0, hour=0, wday=4 (Thursday),
    // mday=1, mon=1, year=70
    assert_eq!(ds.recv(), 0); // nvram[0] = BCD sec (0)
    assert_eq!(ds.recv(), 0); // nvram[1] = BCD min (0)
    assert_eq!(ds.recv(), 0); // nvram[2] = BCD hour (0)
    let wday = ds.recv();
    assert!(wday >= 1 && wday <= 7);
    assert_eq!(ds.recv(), 0x01); // nvram[4] = BCD mday=1
    assert_eq!(ds.recv(), 0x01); // nvram[5] = BCD mon=1
}

#[test]
fn test_ds1338_register_pointer_wraps() {
    let ds = Arc::new(Ds1338::new(0x68));
    let _e = ds.event(I2cEvent::StartRecv);

    // Read 64 bytes (full NVRAM)
    for _ in 0..64 {
        let _ = ds.recv();
    }
    // After wrap, time is re-captured, reading starts from offset 0 again
    assert_eq!(ds.recv(), 0); // BCD sec = 0
}

#[test]
fn test_ds1338_send_set_register_pointer() {
    let ds = Arc::new(Ds1338::new(0x68));

    // First send is address pointer (must send START_SEND first)
    ds.event(I2cEvent::StartSend).unwrap();
    ds.send(0x05).unwrap(); // set ptr to 5 (month register)

    // Now recv should read from offset 5
    // Need START_RECV first to capture time
    ds.event(I2cEvent::StartRecv).unwrap();
    assert_eq!(ds.recv(), 0x01); // BCD month = 1 (January at epoch)
}

#[test]
fn test_ds1338_set_time_register() {
    let ds = Ds1338::new(0x68);

    // START_SEND followed by address byte
    ds.event(I2cEvent::StartSend).unwrap();
    ds.send(0).unwrap(); // set ptr to 0 (seconds)

    // Write BCD 0x30 = 30 seconds
    ds.send(0x30).unwrap();

    // The offset should now be 30 seconds
    assert_eq!(ds.get_offset(), 30);
}

#[test]
fn test_ds1338_set_control_register() {
    let ds = Ds1338::new(0x68);

    // Set register pointer to 7 (control register)
    ds.event(I2cEvent::StartSend).unwrap();
    ds.send(7).unwrap();

    // Write control value (bits 2,3,6 are filtered)
    ds.send(0xFF).unwrap();

    // Read back control register
    ds.event(I2cEvent::StartSend).unwrap();
    ds.send(7).unwrap();
    ds.event(I2cEvent::StartRecv).unwrap();
    // bits 2,3,6 forced to 0, other bits preserved within mask
    let ctrl = ds.recv();
    assert_eq!(ctrl & 0x04, 0); // bit 2 = 0
    assert_eq!(ctrl & 0x08, 0); // bit 3 = 0
    assert_eq!(ctrl & 0x40, 0); // bit 6 = 0
}

#[test]
fn test_ds1338_i2c_start_send_flags() {
    let ds = Ds1338::new(0x68);

    // After START_SEND, first byte is address
    ds.event(I2cEvent::StartSend).unwrap();
    // Send address byte
    ds.send(0x03).unwrap();
    // Now send data byte
    ds.send(0x04).unwrap(); // nvram[3] = wday

    // After another START_SEND, first byte is again address
    ds.event(I2cEvent::StartSend).unwrap();
    ds.send(0x03).unwrap(); // set ptr to 3
                            // START_RECV captures time and reads
    ds.event(I2cEvent::StartRecv).unwrap();
    let _ = ds.recv(); // ptr=0 (after capture, ptr advances)
    let _ = ds.recv(); // ptr=1
    let _ = ds.recv(); // ptr=2
}

#[test]
fn test_ds1338_nvram_write_read() {
    let ds = Arc::new(Ds1338::new(0x68));

    // Write to user NVRAM (offset 8+)
    ds.event(I2cEvent::StartSend).unwrap();
    ds.send(10).unwrap(); // set ptr to 10
    ds.send(0xAB).unwrap();

    // Read back
    ds.event(I2cEvent::StartSend).unwrap();
    ds.send(10).unwrap();
    ds.event(I2cEvent::StartRecv).unwrap();
    assert_eq!(ds.recv(), 0xAB);
}
