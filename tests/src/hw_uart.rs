use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_hw_char::uart::Uart16550;
use machina_hw_core::bus::SysBus;
use machina_hw_core::chardev::{CharFrontend, Chardev};
use machina_hw_core::irq::{IrqLine, IrqSink};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MemoryRegion;

// -- Test helpers --

struct TestIrqSink {
    levels: Vec<AtomicBool>,
}

impl TestIrqSink {
    fn new(n: usize) -> Self {
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(AtomicBool::new(false));
        }
        Self { levels: v }
    }

    fn level(&self, irq: u32) -> bool {
        self.levels[irq as usize].load(Ordering::Relaxed)
    }
}

impl IrqSink for TestIrqSink {
    fn set_irq(&self, irq: u32, level: bool) {
        if let Some(f) = self.levels.get(irq as usize) {
            f.store(level, Ordering::Relaxed);
        }
    }
}

#[test]
fn test_uart_lsr_initial() {
    let uart = Uart16550::new();
    let lsr = uart.read(5);
    // THR empty (bit 5) and transmitter empty (bit 6).
    assert_ne!(lsr & 0x20, 0, "THRE should be set");
    assert_ne!(lsr & 0x40, 0, "TEMT should be set");
}

#[test]
fn test_uart_write_thr() {
    let uart = Uart16550::new();
    uart.write(0, 0x41); // write 'A'
    let lsr = uart.read(5);
    assert_ne!(lsr & 0x20, 0, "THRE should remain set");
}

#[test]
fn test_uart_receive() {
    let uart = Uart16550::new();
    uart.receive(0x42); // push 'B'
    let lsr = uart.read(5);
    assert_ne!(lsr & 0x01, 0, "DR should be set");
}

#[test]
fn test_uart_read_rbr() {
    let uart = Uart16550::new();
    uart.receive(0x42);
    let ch = uart.read(0);
    assert_eq!(ch, 0x42);

    // After reading, DR should be cleared (FIFO empty).
    let lsr = uart.read(5);
    assert_eq!(lsr & 0x01, 0, "DR should be cleared");
}

#[test]
fn test_uart_dlab() {
    let uart = Uart16550::new();

    // Set DLAB.
    uart.write(3, 0x80);

    // Write DLL and DLM.
    uart.write(0, 0x0C); // DLL = 12
    uart.write(1, 0x00); // DLM = 0

    // Read them back.
    assert_eq!(uart.read(0), 0x0C);
    assert_eq!(uart.read(1), 0x00);

    // Clear DLAB, verify normal register access.
    uart.write(3, 0x00);
    uart.write(1, 0x01); // IER = enable RX
    assert_eq!(uart.read(1), 0x01);
}

#[test]
fn test_uart_fifo() {
    let uart = Uart16550::new();

    // Push multiple bytes.
    uart.receive(0x61); // 'a'
    uart.receive(0x62); // 'b'
    uart.receive(0x63); // 'c'

    // Read them in order.
    assert_eq!(uart.read(0), 0x61);
    assert_eq!(uart.read(0), 0x62);
    assert_eq!(uart.read(0), 0x63);

    // FIFO empty now.
    let lsr = uart.read(5);
    assert_eq!(lsr & 0x01, 0, "DR should be cleared");
}

#[test]
fn test_uart_irq_on_receive() {
    let uart = Uart16550::new();

    // Enable RX available interrupt.
    uart.write(1, 0x01); // IER bit 0

    // No data yet, no IRQ.
    assert!(!uart.irq_pending());

    // Receive a byte -- should raise IRQ.
    uart.receive(0x55);
    assert!(uart.irq_pending());

    // IIR should indicate RX data available (0x04).
    let iir = uart.read(2);
    assert_eq!(iir, 0x04);

    // Read the byte -- IRQ should clear.
    let _ = uart.read(0);
    assert!(!uart.irq_pending());
}

#[test]
fn test_uart_tx_to_chardev() {
    let mut bus = SysBus::new("sysbus0");
    let mut address_space =
        AddressSpace::new(MemoryRegion::container("system", u64::MAX));
    let uart = Arc::new(Uart16550::new_named("uart0"));
    let buf_ref = Arc::new(Mutex::new(Vec::<u8>::new()));

    struct SharedChardev {
        buf: Arc<Mutex<Vec<u8>>>,
    }
    impl Chardev for SharedChardev {
        fn read(&mut self) -> Option<u8> {
            None
        }
        fn write(&mut self, data: u8) {
            self.buf.lock().unwrap().push(data);
        }
        fn can_read(&self) -> bool {
            false
        }
    }
    let shared_buf = Arc::clone(&buf_ref);
    let chardev = SharedChardev { buf: shared_buf };
    let fe = CharFrontend::new(Box::new(chardev));

    {
        uart.attach_to_bus(&mut bus).unwrap();
        let region = MemoryRegion::io(
            "uart0",
            0x100,
            Arc::new(machina_hw_char::uart::Uart16550Mmio(Arc::clone(&uart))),
        );
        uart.register_mmio(region, GPA::new(0x1000_0000)).unwrap();
        uart.attach_chardev(fe).unwrap();
        let rx_cb: Arc<Mutex<dyn FnMut(u8) + Send>> =
            Arc::new(Mutex::new(move |_byte: u8| {}));
        uart.realize_onto(&mut bus, &mut address_space, rx_cb)
            .unwrap();
    }

    // Write 'A' to THR.
    uart.write(0, 0x41);
    let got = buf_ref.lock().unwrap().clone();
    assert_eq!(got, vec![0x41], "chardev should receive 'A'");

    // Write another byte.
    uart.write(0, 0x42);
    let got = buf_ref.lock().unwrap().clone();
    assert_eq!(got, vec![0x41, 0x42], "chardev should receive both bytes");
}

#[test]
fn test_uart_rx_irq_line() {
    let mut bus = SysBus::new("sysbus0");
    let mut address_space =
        AddressSpace::new(MemoryRegion::container("system", u64::MAX));
    let uart = Arc::new(Uart16550::new_named("uart0"));

    // Create test IRQ sink and attach line.
    let sink = Arc::new(TestIrqSink::new(16));
    let irq_num = 10u32;
    let line = IrqLine::new(Arc::clone(&sink) as Arc<dyn IrqSink>, irq_num);
    {
        uart.attach_to_bus(&mut bus).unwrap();
        let region = MemoryRegion::io(
            "uart0",
            0x100,
            Arc::new(machina_hw_char::uart::Uart16550Mmio(Arc::clone(&uart))),
        );
        uart.register_mmio(region, GPA::new(0x1000_0000)).unwrap();
        uart.attach_irq(line).unwrap();
        let rx_cb: Arc<Mutex<dyn FnMut(u8) + Send>> =
            Arc::new(Mutex::new(move |_byte: u8| {}));
        uart.realize_onto(&mut bus, &mut address_space, rx_cb)
            .unwrap();
    }

    // Enable RX available interrupt.
    uart.write(1, 0x01);

    // IRQ line should be low before data arrives.
    assert!(!sink.level(irq_num), "IRQ should be low before receive");

    // Receive a byte -- IRQ should assert.
    uart.receive(0x55);
    assert!(sink.level(irq_num), "IRQ should be raised after receive");

    // Read the byte -- IRQ should deassert.
    let _ = uart.read(0);
    assert!(
        !sink.level(irq_num),
        "IRQ should be lowered after reading RBR"
    );
}

#[test]
fn test_uart_chardev_property_set_and_get() {
    let uart = Uart16550::new_named("uart0");
    assert_eq!(uart.chardev_property(), None);
    uart.set_chardev_property("/machine/chardev/uart0").unwrap();
    assert_eq!(
        uart.chardev_property().as_deref(),
        Some("/machine/chardev/uart0")
    );
}

#[test]
fn test_uart_realize_via_sysbus_installs_runtime_wiring() {
    struct LoopbackChardev {
        tx: Arc<Mutex<Vec<u8>>>,
        startup_byte: u8,
    }

    impl Chardev for LoopbackChardev {
        fn read(&mut self) -> Option<u8> {
            None
        }

        fn write(&mut self, data: u8) {
            self.tx.lock().unwrap().push(data);
        }

        fn can_read(&self) -> bool {
            false
        }

        fn start_input(&mut self, cb: machina_hw_core::chardev::ByteCb) {
            let startup_byte = self.startup_byte;
            std::thread::spawn(move || {
                cb.lock().unwrap()(startup_byte);
            });
        }
    }

    let mut bus = SysBus::new("sysbus0");
    let mut address_space =
        AddressSpace::new(MemoryRegion::container("system", u64::MAX));
    let uart = Arc::new(Uart16550::new_named("uart0"));
    let tx = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::new(TestIrqSink::new(16));

    {
        uart.set_chardev_property("/machine/chardev/uart0").unwrap();
        uart.attach_to_bus(&mut bus).unwrap();
        let region = MemoryRegion::io(
            "uart0",
            0x100,
            Arc::new(machina_hw_char::uart::Uart16550Mmio(Arc::clone(&uart))),
        );
        uart.register_mmio(region, GPA::new(0x1000_0000)).unwrap();
        uart.attach_irq(IrqLine::new(
            Arc::clone(&sink) as Arc<dyn IrqSink>,
            10,
        ))
        .unwrap();
        uart.attach_chardev(CharFrontend::new(Box::new(LoopbackChardev {
            tx: Arc::clone(&tx),
            startup_byte: 0x51,
        })))
        .unwrap();
        let uart_for_rx = Arc::clone(&uart);
        let rx_cb: Arc<Mutex<dyn FnMut(u8) + Send>> =
            Arc::new(Mutex::new(move |byte: u8| {
                uart_for_rx.receive(byte);
            }));
        uart.realize_onto(&mut bus, &mut address_space, rx_cb)
            .unwrap();
        uart.write(1, 0x01);
    }

    assert!(address_space.is_mapped(GPA::new(0x1000_0000), 4));

    // Poll until the RX byte arrives (start_input spawns a thread).
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let lsr = uart.read(5);
        if lsr & 0x01 != 0 {
            break;
        }
    }

    {
        assert!(uart.realized());
        assert_eq!(uart.read(0), 0x51);
        uart.write(0, 0x41);
    }

    assert_eq!(*tx.lock().unwrap(), vec![0x41]);
    assert_eq!(bus.mappings().len(), 1);
    assert_eq!(bus.mappings()[0].owner, "uart0");
}

#[test]
fn test_uart_unrealize_drops_runtime_wiring() {
    struct SinkChardev {
        tx: Arc<Mutex<Vec<u8>>>,
    }

    impl Chardev for SinkChardev {
        fn read(&mut self) -> Option<u8> {
            None
        }

        fn write(&mut self, data: u8) {
            self.tx.lock().unwrap().push(data);
        }

        fn can_read(&self) -> bool {
            false
        }
    }

    let mut bus = SysBus::new("sysbus0");
    let mut address_space =
        AddressSpace::new(MemoryRegion::container("system", u64::MAX));
    let uart = Arc::new(Uart16550::new_named("uart0"));
    let tx = Arc::new(Mutex::new(Vec::new()));

    {
        uart.attach_to_bus(&mut bus).unwrap();
        let region = MemoryRegion::io(
            "uart0",
            0x100,
            Arc::new(machina_hw_char::uart::Uart16550Mmio(Arc::clone(&uart))),
        );
        uart.register_mmio(region, GPA::new(0x1000_0000)).unwrap();
        uart.attach_chardev(CharFrontend::new(Box::new(SinkChardev {
            tx: Arc::clone(&tx),
        })))
        .unwrap();
        let rx_cb: Arc<Mutex<dyn FnMut(u8) + Send>> =
            Arc::new(Mutex::new(move |_byte: u8| {}));
        uart.realize_onto(&mut bus, &mut address_space, rx_cb)
            .unwrap();
        uart.write(0, 0x41);
        uart.unrealize_from(&mut bus, &mut address_space).unwrap();
        uart.write(0, 0x42);
    }

    assert_eq!(*tx.lock().unwrap(), vec![0x41]);
    assert!(!address_space.is_mapped(GPA::new(0x1000_0000), 4));
    assert!(bus.mappings().is_empty());
}

// -- PL011 tests --

use machina_hw_char::pl011::{Pl011, Pl011Mmio, PL011_IRQ_COMBINED};
use machina_hw_core::irq::InterruptSource;
use machina_memory::region::MmioOps;

const PL011_INT_RX: u64 = 1 << 4;
const PL011_INT_TX: u64 = 1 << 5;

struct Pl011TestSink {
    pub levels: Mutex<Vec<bool>>,
}

impl Pl011TestSink {
    fn new(n: usize) -> Self {
        Self {
            levels: Mutex::new(vec![false; n]),
        }
    }

    fn level(&self, irq: u32) -> bool {
        self.levels.lock().unwrap()[irq as usize]
    }
}

impl IrqSink for Pl011TestSink {
    fn set_irq(&self, irq: u32, level: bool) {
        let mut levels = self.levels.lock().unwrap();
        if let Some(slot) = levels.get_mut(irq as usize) {
            *slot = level;
        }
    }
}

#[test]
fn test_pl011_defaults() {
    let pl011 = Arc::new(Pl011::new());
    let mmio = Pl011Mmio(Arc::clone(&pl011));

    assert_eq!(mmio.read(0x00, 4), 0);
    assert_eq!(mmio.read(0x04, 4), 0);
    let fr = mmio.read(0x18, 4);
    assert_eq!(fr & 0x10, 0x10, "RXFE should be set");
    assert_eq!(fr & 0x80, 0x80, "TXFE should be set");
    assert_eq!(mmio.read(0x30, 4), 0x300);
    assert_eq!(mmio.read(0x34, 4), 0x12);
    assert_eq!(mmio.read(0x38, 4), 0);
    assert_eq!(mmio.read(0xFE0, 4), 0x11);
    assert_eq!(mmio.read(0xFE4, 4), 0x10);
    assert_eq!(mmio.read(0xFE8, 4), 0x14);
    assert_eq!(mmio.read(0xFEC, 4), 0x00);
}

#[test]
fn test_pl011_rx_fifo_and_irq() {
    let pl011 = Arc::new(Pl011::new());
    let sink = Arc::new(Pl011TestSink::new(6));
    let mmio = Pl011Mmio(Arc::clone(&pl011));

    pl011.connect_output(
        PL011_IRQ_COMBINED,
        InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0),
    );

    mmio.write(0x38, 4, PL011_INT_RX);
    assert!(!sink.level(0));
    pl011.receive(0x41);
    assert!(sink.level(0));
    assert_eq!(mmio.read(0x00, 4), 0x41);
    assert!(!sink.level(0));
}

#[test]
fn test_pl011_tx_irq() {
    let pl011 = Arc::new(Pl011::new());
    let sink = Arc::new(Pl011TestSink::new(6));
    let mmio = Pl011Mmio(Arc::clone(&pl011));

    pl011.connect_output(
        PL011_IRQ_COMBINED,
        InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0),
    );

    mmio.write(0x38, 4, PL011_INT_TX);
    mmio.write(0x00, 4, 0x42);
    assert!(sink.level(0));

    mmio.write(0x44, 4, PL011_INT_TX);
    assert!(!sink.level(0));
}

#[test]
fn test_pl011_write_to_flag_register_ignored() {
    let pl011 = Arc::new(Pl011::new());
    let mmio = Pl011Mmio(Arc::clone(&pl011));

    let fr_before = mmio.read(0x18, 4);
    mmio.write(0x18, 4, 0xFFFF_FFFF);
    assert_eq!(mmio.read(0x18, 4), fr_before);
}

#[test]
fn test_pl011_ibrd_fbrd_masked() {
    let pl011 = Arc::new(Pl011::new());
    let mmio = Pl011Mmio(Arc::clone(&pl011));

    mmio.write(0x24, 4, 0xFFFF_FFFF);
    assert_eq!(mmio.read(0x24, 4), 0xFFFF);

    mmio.write(0x28, 4, 0xFFFF_FFFF);
    assert_eq!(mmio.read(0x28, 4), 0x3F);
}

#[test]
fn test_pl011_reset_runtime() {
    let pl011 = Arc::new(Pl011::new());
    let mmio = Pl011Mmio(Arc::clone(&pl011));

    mmio.write(0x24, 4, 0x1234);
    mmio.write(0x38, 4, 0xFF);
    assert_eq!(mmio.read(0x24, 4), 0x1234);

    pl011.reset_runtime();
    assert_eq!(mmio.read(0x24, 4), 0);
    assert_eq!(mmio.read(0x38, 4), 0);
    assert_eq!(mmio.read(0x30, 4), 0x300);
}

#[test]
fn test_pl011_ris_mis_icr() {
    let pl011 = Arc::new(Pl011::new());
    let mmio = Pl011Mmio(Arc::clone(&pl011));

    mmio.write(0x38, 4, PL011_INT_RX);
    assert_eq!(mmio.read(0x3C, 4), 0);
    assert_eq!(mmio.read(0x40, 4), 0);

    pl011.receive(0x55);
    assert_eq!(mmio.read(0x3C, 4) & PL011_INT_RX, PL011_INT_RX);
    assert_eq!(mmio.read(0x40, 4) & PL011_INT_RX, PL011_INT_RX);

    mmio.write(0x44, 4, PL011_INT_RX);
    assert_eq!(mmio.read(0x3C, 4) & PL011_INT_RX, 0);
}

#[test]
fn test_pl011_loopback() {
    let pl011 = Arc::new(Pl011::new());
    let mmio = Pl011Mmio(Arc::clone(&pl011));

    mmio.write(0x30, 4, 0x380);

    mmio.write(0x00, 4, 0x41);
    assert_eq!(mmio.read(0x00, 4), 0x41);
}

// -- SiFive UART tests --

use machina_hw_char::sifive_uart::{SiFiveUart, SiFiveUartMmio};

#[test]
fn test_sifive_uart_defaults() {
    let uart = Arc::new(SiFiveUart::new());
    let mmio = SiFiveUartMmio(Arc::clone(&uart));

    assert_eq!(mmio.read(0x08, 4), 0); // txfifo
    assert_eq!(mmio.read(0x0C, 4), 0); // rxctrl
    assert_eq!(mmio.read(0x10, 4), 0); // ie
    assert_eq!(mmio.read(0x14, 4), 0); // ip
    assert_eq!(mmio.read(0x18, 4), 0); // div
                                       // RX FIFO empty returns 0x8000_0000
    assert_eq!(mmio.read(0x04, 4), 0x8000_0000);
}

#[test]
fn test_sifive_uart_tx_rx_watermark_irq() {
    let uart = Arc::new(SiFiveUart::new());
    let sink = Arc::new(Pl011TestSink::new(1));
    let mmio = SiFiveUartMmio(Arc::clone(&uart));

    uart.connect_output(InterruptSource::new(
        Arc::clone(&sink) as Arc<dyn IrqSink>,
        0,
    ));

    // Enable TX, set TX watermark to 1
    mmio.write(0x08, 4, 1); // txctrl = TXEN=1, TXCNT=0 (cnt=0 means watermk=1)
                            // Enable RX with watermark = 0
    mmio.write(0x0C, 4, 1); // rxctrl = RXEN=1, RXCNT=0
                            // Enable interrupts
    mmio.write(0x10, 4, 3); // IE_TXWM | IE_RXWM

    // No IRQ initially
    assert!(!sink.level(0));

    // Write TX FIFO
    mmio.write(0x00, 4, 0x41);

    // Receive a byte via RX
    uart.receive(0x42);

    // RX watermark triggered (1 > 0)
    assert!(sink.level(0));
}

#[test]
fn test_sifive_uart_rx_disabled_by_rxctrl() {
    let uart = Arc::new(SiFiveUart::new());
    let mmio = SiFiveUartMmio(Arc::clone(&uart));

    // RXEN=0
    mmio.write(0x0C, 4, 0);

    // Try to receive
    uart.receive(0x41);

    // Should be dropped
    assert_eq!(mmio.read(0x04, 4), 0x8000_0000);
}

#[test]
fn test_sifive_uart_tx_disabled_ignores_write() {
    let uart = Arc::new(SiFiveUart::new());
    let mmio = SiFiveUartMmio(Arc::clone(&uart));

    // TXEN=0, write to TXFIFO should be ignored
    mmio.write(0x00, 4, 0x41);
    assert_eq!(mmio.read(0x00, 4) & 0x8000_0000, 0);
}

#[test]
fn test_sifive_uart_div() {
    let uart = Arc::new(SiFiveUart::new());
    let mmio = SiFiveUartMmio(Arc::clone(&uart));

    mmio.write(0x18, 4, 0xABCD);
    assert_eq!(mmio.read(0x18, 4), 0xABCD);
}

#[test]
fn test_sifive_uart_reset_runtime() {
    let uart = Arc::new(SiFiveUart::new());
    let mmio = SiFiveUartMmio(Arc::clone(&uart));

    mmio.write(0x18, 4, 0x1234);
    mmio.write(0x10, 4, 0xFF);
    assert_eq!(mmio.read(0x18, 4), 0x1234);

    uart.reset_runtime();
    assert_eq!(mmio.read(0x18, 4), 0);
    assert_eq!(mmio.read(0x10, 4), 0);
}

// -- HTIF tests --

use machina_hw_char::riscv_htif::{Htif, HtifMmio};
use std::sync::atomic::AtomicI32;

#[test]
fn test_htif_defaults() {
    let htif = Arc::new(Htif::new());
    let mmio = HtifMmio(Arc::clone(&htif));

    // tohost low/high
    assert_eq!(mmio.read(0, 4), 0);
    assert_eq!(mmio.read(4, 4), 0);
    // fromhost low/high
    assert_eq!(mmio.read(8, 4), 0);
    assert_eq!(mmio.read(12, 4), 0);
}

#[test]
fn test_htif_tohost_two_phase_write() {
    let htif = Arc::new(Htif::new());
    let mmio = HtifMmio(Arc::clone(&htif));

    // device=1 (console), cmd=1 (PUTC), payload='X'=0x58
    // tohost = (1 << 56) | (1 << 48) | 0x58 = 0x0101_0000_0000_0058
    let tohost: u64 = (1 << 56) | (1 << 48) | 0x58;
    let lo = (tohost & 0xFFFF_FFFF) as u32;
    let hi = (tohost >> 32) as u32;

    // Phase 1: write low 32 bits (tohost was 0, so allow_tohost=true)
    mmio.write(0, 4, u64::from(lo));
    assert_eq!(mmio.read(0, 4), u64::from(lo));
    assert_eq!(mmio.read(4, 4), 0);

    // Phase 2: write high 32 bits -> triggers handling
    mmio.write(4, 4, u64::from(hi));
    // After handling, tohost should be cleared
    assert_eq!(mmio.read(0, 4), 0);
    assert_eq!(mmio.read(4, 4), 0);
}

#[test]
fn test_htif_fromhost_two_phase_write() {
    let htif = Arc::new(Htif::new());
    let mmio = HtifMmio(Arc::clone(&htif));

    mmio.write(8, 4, 0xBEEF);
    assert_eq!(mmio.read(8, 4), 0xBEEF);

    mmio.write(12, 4, 0xDEAD);
    assert_eq!(mmio.read(12, 4), 0xDEAD);
    assert_eq!(mmio.read(8, 4), 0xBEEF);
}

#[test]
fn test_htif_console_putc() {
    let htif = Arc::new(Htif::new());
    let mmio = HtifMmio(Arc::clone(&htif));

    // device=1 (console), cmd=1 (PUTC), payload='A'
    // tohost = (1 << 56) | (1 << 48) | 0x41 = 0x0101_0000_0000_0041
    let tohost: u64 = (1 << 56) | (1 << 48) | 0x41;
    let lo = (tohost & 0xFFFF_FFFF) as u32;
    let hi = (tohost >> 32) as u32;

    mmio.write(0, 4, u64::from(lo));
    mmio.write(4, 4, u64::from(hi));

    // fromhost should have response: device+cmd from tohost, resp=0x100|'A'
    let fromhost = mmio.read(8, 4) as u64 | ((mmio.read(12, 4) as u64) << 32);
    let expected = (tohost & 0xFFFF_0000_0000_0000) | 0x141;
    assert_eq!(fromhost, expected);
}

#[test]
fn test_htif_console_getc() {
    let htif = Arc::new(Htif::new());
    let mmio = HtifMmio(Arc::clone(&htif));

    // device=1 (console), cmd=0 (GETC)
    let tohost: u64 = (1 << 56) | (0 << 48);
    let lo = (tohost & 0xFFFF_FFFF) as u32;
    let hi = (tohost >> 32) as u32;

    mmio.write(0, 4, u64::from(lo));
    mmio.write(4, 4, u64::from(hi));

    // tohost should be cleared (indicating we read)
    assert_eq!(mmio.read(0, 4), 0);
    assert_eq!(mmio.read(4, 4), 0);

    // fromhost should still be 0 (no response until receive())
    assert_eq!(mmio.read(8, 4), 0);
    assert_eq!(mmio.read(12, 4), 0);

    // Now receive a byte from chardev
    htif.receive(0x51);

    // fromhost should have resp=0x100|0x51 in low 16 bits
    let fromhost = mmio.read(8, 4) as u64 | ((mmio.read(12, 4) as u64) << 32);
    assert_eq!(fromhost & 0xFFFF, 0x151);
    // device+cmd bits preserved from pending_read (which was tohost value)
    assert_eq!((fromhost >> 56) & 0xFF, 1); // device=1
    assert_eq!((fromhost >> 48) & 0xFF, 0); // cmd=0
}

#[test]
fn test_htif_system_exit() {
    let htif = Arc::new(Htif::new());
    let mmio = HtifMmio(Arc::clone(&htif));

    let exit_code = Arc::new(AtomicI32::new(-1));
    let ec = Arc::clone(&exit_code);
    htif.set_exit_callback(Box::new(move |code| {
        ec.store(code, Ordering::Relaxed);
    }));

    // device=0 (system), cmd=0 (syscall), payload with bit0=1 (exit)
    // exit_code = 42, payload = (42 << 1) | 1 = 85
    let tohost: u64 = 85;
    let lo = (tohost & 0xFFFF_FFFF) as u32;
    let hi = (tohost >> 32) as u32;

    mmio.write(0, 4, u64::from(lo));
    mmio.write(4, 4, u64::from(hi));

    assert_eq!(exit_code.load(Ordering::Relaxed), 42);
}

#[test]
fn test_htif_tohost_reject_when_busy() {
    let htif = Arc::new(Htif::new());
    let mmio = HtifMmio(Arc::clone(&htif));

    // First write sets tohost non-zero
    mmio.write(0, 4, 0xDEAD);

    // Second write to offset 0 should set allow_tohost=false
    // since tohost is non-zero
    mmio.write(0, 4, 0xBEEF);
    // The value stored is still set but allow_tohost is false
    // Now write high bits - should NOT trigger handle because
    // allow_tohost is false
    mmio.write(4, 4, 0xCAFE);
    // tohost should have both parts since allow_tohost was false
    // and high bits were combined
    // Actually allow_tohost was false so the write to offset 4 is
    // ignored. But since we wrote 0xBEEF last to low bits, and
    // allow_tohost=false means write to offset 4 is a no-op,
    // tohost should still be 0xBEEF (just the low write, unhandled).
    assert_ne!(mmio.read(0, 4), 0);
}

#[test]
fn test_htif_reset_runtime() {
    let htif = Arc::new(Htif::new());
    let mmio = HtifMmio(Arc::clone(&htif));

    // Write tohost
    mmio.write(0, 4, 0x1234);
    mmio.write(4, 4, 0x5678);
    // Write fromhost
    mmio.write(8, 4, 0xABCD);
    mmio.write(12, 4, 0xEF01);

    htif.reset_runtime();

    assert_eq!(mmio.read(0, 4), 0);
    assert_eq!(mmio.read(4, 4), 0);
    assert_eq!(mmio.read(8, 4), 0);
    assert_eq!(mmio.read(12, 4), 0);
}

#[test]
fn test_htif_unknown_device() {
    let htif = Arc::new(Htif::new());
    let mmio = HtifMmio(Arc::clone(&htif));

    // device=2 (unknown), cmd=0
    let tohost: u64 = 2u64 << 56;
    let lo = (tohost & 0xFFFF_FFFF) as u32;
    let hi = (tohost >> 32) as u32;

    mmio.write(0, 4, u64::from(lo));
    mmio.write(4, 4, u64::from(hi));

    // fromhost should have response with resp=0 (no handler)
    let fromhost = mmio.read(8, 4) as u64 | ((mmio.read(12, 4) as u64) << 32);
    // device+cmd preserved, resp=0
    assert_eq!(fromhost, tohost & 0xFFFF_0000_0000_0000);
    // tohost cleared
    assert_eq!(mmio.read(0, 4), 0);
    assert_eq!(mmio.read(4, 4), 0);
}
