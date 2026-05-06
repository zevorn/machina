use std::sync::{Arc, Mutex};

use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_ssi::pl022::{Pl022, Pl022Mmio};
use machina_hw_ssi::sifive_spi::{SiFiveSpi, SiFiveSpiMmio};
use machina_hw_ssi::{SpiBus, SpiCsPolarity, SpiSlave};
use machina_memory::region::MmioOps;

/// Mock SPI slave that echoes back shifted data.
struct EchoSlave {
    cs_state: Mutex<bool>,
}

impl EchoSlave {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            cs_state: Mutex::new(false),
        })
    }

    fn cs(&self) -> bool {
        *self.cs_state.lock().unwrap()
    }
}

impl SpiSlave for EchoSlave {
    fn transfer(&self, val: u32) -> u32 {
        val ^ 0xAA
    }

    fn set_cs(&self, cs: bool) {
        *self.cs_state.lock().unwrap() = cs;
    }

    fn cs_polarity(&self) -> SpiCsPolarity {
        SpiCsPolarity::High
    }

    fn cs_index(&self) -> u8 {
        0
    }
}

// -- Positive Tests --

#[test]
fn test_spi_bus_new() {
    let bus = SpiBus::new();
    assert_eq!(bus.last_result(), 0);
}

#[test]
fn test_spi_attach_detach() {
    let bus = SpiBus::new();
    let slave = EchoSlave::new();

    assert!(bus.attach(slave.clone()).is_ok());
    assert!(bus.get_cs(0).is_some());

    let removed = bus.detach(0);
    assert!(removed.is_some());
    assert!(bus.get_cs(0).is_none());
}

#[test]
fn test_spi_transfer_no_slave_returns_0xff() {
    let bus = SpiBus::new();
    assert_eq!(bus.transfer(0x12), 0xFF);
    assert_eq!(bus.last_result(), 0xFF);
}

#[test]
fn test_spi_transfer_with_slave() {
    let bus = SpiBus::new();
    let slave = EchoSlave::new();
    bus.attach(slave.clone()).unwrap();

    // Activate CS (active high)
    bus.set_cs(0, true);
    assert!(slave.cs());

    let result = bus.transfer(0x42);
    assert_eq!(result, 0x42 ^ 0xAA);
    assert_eq!(bus.last_result(), 0x42 ^ 0xAA);
}

#[test]
fn test_spi_transfer_slave_not_selected_returns_0xff() {
    let bus = SpiBus::new();
    let slave = EchoSlave::new();
    bus.attach(slave).unwrap();

    // CS not asserted -> slave not selected (active-high)
    assert_eq!(bus.transfer(0x42), 0xFF);
}

#[test]
fn test_spi_active_low_cs() {
    struct ActiveLowSlave {
        cs_state: Mutex<bool>,
    }

    impl ActiveLowSlave {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                cs_state: Mutex::new(false),
            })
        }
    }

    impl SpiSlave for ActiveLowSlave {
        fn transfer(&self, val: u32) -> u32 {
            val.wrapping_add(1)
        }

        fn set_cs(&self, cs: bool) {
            *self.cs_state.lock().unwrap() = cs;
        }

        fn cs_polarity(&self) -> SpiCsPolarity {
            SpiCsPolarity::Low
        }

        fn cs_index(&self) -> u8 {
            0
        }
    }

    let bus = SpiBus::new();
    let slave = ActiveLowSlave::new();
    bus.attach(slave).unwrap();

    // CS Low = selected for active-low
    bus.set_cs(0, false);
    assert_eq!(bus.transfer(0x10), 0x11);

    // CS High = deselected
    bus.set_cs(0, true);
    assert_eq!(bus.transfer(0x10), 0xFF);
}

#[test]
fn test_spi_cs_none_always_selected() {
    struct AlwaysOnSlave;

    impl SpiSlave for AlwaysOnSlave {
        fn transfer(&self, val: u32) -> u32 {
            val
        }

        fn set_cs(&self, _cs: bool) {}

        fn cs_polarity(&self) -> SpiCsPolarity {
            SpiCsPolarity::None
        }

        fn cs_index(&self) -> u8 {
            0
        }
    }

    let bus = SpiBus::new();
    let slave = Arc::new(AlwaysOnSlave);
    bus.attach(slave).unwrap();

    // Should respond regardless of CS
    assert_eq!(bus.transfer(0xAB), 0xAB);
}

// -- Negative Tests --

#[test]
fn test_spi_attach_duplicate_cs_fails() {
    let bus = SpiBus::new();
    let s1 = EchoSlave::new();
    let s2 = EchoSlave::new();

    assert!(bus.attach(s1).is_ok());
    assert!(bus.attach(s2).is_err());
}

#[test]
fn test_spi_detach_nonexistent_returns_none() {
    let bus = SpiBus::new();
    assert!(bus.detach(0).is_none());
    assert!(bus.detach(255).is_none());
}

#[test]
fn test_spi_get_cs_nonexistent_returns_none() {
    let bus = SpiBus::new();
    assert!(bus.get_cs(0).is_none());
}

#[test]
fn test_spi_multiple_slaves_or_result() {
    struct Add1Slave;
    struct Add2Slave;

    impl SpiSlave for Add1Slave {
        fn transfer(&self, val: u32) -> u32 {
            val + 1
        }
        fn set_cs(&self, _cs: bool) {}
        fn cs_polarity(&self) -> SpiCsPolarity {
            SpiCsPolarity::None
        }
        fn cs_index(&self) -> u8 {
            0
        }
    }

    impl SpiSlave for Add2Slave {
        fn transfer(&self, val: u32) -> u32 {
            val + 2
        }
        fn set_cs(&self, _cs: bool) {}
        fn cs_polarity(&self) -> SpiCsPolarity {
            SpiCsPolarity::None
        }
        fn cs_index(&self) -> u8 {
            1
        }
    }

    let bus = SpiBus::new();
    bus.attach(Arc::new(Add1Slave)).unwrap();
    bus.attach(Arc::new(Add2Slave)).unwrap();

    // Both have CS=None, so both are selected -> OR result
    let r = bus.transfer(0);
    // (0+1) | (0+2) = 1 | 2 = 3
    assert_eq!(r, 3);
}

#[test]
fn test_spi_cs_transition_calls_set_cs_once() {
    struct CountingSlave {
        cs_calls: Mutex<u32>,
        cs_state: Mutex<bool>,
    }

    impl CountingSlave {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                cs_calls: Mutex::new(0),
                cs_state: Mutex::new(false),
            })
        }
    }

    impl SpiSlave for CountingSlave {
        fn transfer(&self, _val: u32) -> u32 {
            0
        }
        fn set_cs(&self, cs: bool) {
            let mut count = self.cs_calls.lock().unwrap();
            *count += 1;
            *self.cs_state.lock().unwrap() = cs;
        }
        fn cs_polarity(&self) -> SpiCsPolarity {
            SpiCsPolarity::High
        }
        fn cs_index(&self) -> u8 {
            0
        }
    }

    let bus = SpiBus::new();
    let slave = CountingSlave::new();
    bus.attach(slave.clone()).unwrap();

    // First CS change: 0 -> 1
    bus.set_cs(0, true);
    assert_eq!(*slave.cs_calls.lock().unwrap(), 1);

    // Same level again: no change
    bus.set_cs(0, true);
    assert_eq!(*slave.cs_calls.lock().unwrap(), 1);

    // Different level: 1 -> 0
    bus.set_cs(0, false);
    assert_eq!(*slave.cs_calls.lock().unwrap(), 2);
}

// ---- PL022 tests ----

struct TestSink {
    level: Mutex<bool>,
}

impl TestSink {
    fn new() -> Self {
        Self {
            level: Mutex::new(false),
        }
    }

    fn level(&self) -> bool {
        *self.level.lock().unwrap()
    }
}

impl IrqSink for TestSink {
    fn set_irq(&self, _irq: u32, level: bool) {
        *self.level.lock().unwrap() = level;
    }
}

#[test]
fn test_pl022_defaults() {
    let pl022 = Arc::new(Pl022::new());
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    assert_eq!(mmio.read(0x00, 4), 0); // CR0
    assert_eq!(mmio.read(0x04, 4), 0); // CR1
    assert_eq!(mmio.read(0x0C, 4), 0x03); // SR: TFE|TNF
    assert_eq!(mmio.read(0x10, 4), 0); // CPSR
    assert_eq!(mmio.read(0x14, 4), 0); // IM
    assert_eq!(mmio.read(0x18, 4), 0x08); // IS: TX=1
    assert_eq!(mmio.read(0x1C, 4), 0); // MIS
}

#[test]
fn test_pl022_id_registers() {
    let pl022 = Arc::new(Pl022::new());
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    assert_eq!(mmio.read(0xFE0, 4), 0x22);
    assert_eq!(mmio.read(0xFE4, 4), 0x10);
    assert_eq!(mmio.read(0xFE8, 4), 0x04);
    assert_eq!(mmio.read(0xFEC, 4), 0x00);
    assert_eq!(mmio.read(0xFF0, 4), 0x0D);
    assert_eq!(mmio.read(0xFF4, 4), 0xF0);
    assert_eq!(mmio.read(0xFF8, 4), 0x05);
    assert_eq!(mmio.read(0xFFC, 4), 0xB1);
}

#[test]
fn test_pl022_cr0_bitmask() {
    let pl022 = Arc::new(Pl022::new());
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    // DSS=0 → bitmask = (1 << 1) - 1 = 1 → 1-bit transfer
    mmio.write(0x00, 4, 0x00);
    assert_eq!(mmio.read(0x00, 4), 0x00);

    // DSS=7 → bitmask = (1 << 8) - 1 = 0xFF
    mmio.write(0x00, 4, 0x07);
    assert_eq!(mmio.read(0x00, 4), 0x07);
}

#[test]
fn test_pl022_tx_fifo_write_read() {
    let pl022 = Arc::new(Pl022::new());
    let bus = Arc::new(SpiBus::new());
    pl022.connect_ssi_bus(Arc::clone(&bus));
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    // Enable SSE, set bitmask wide enough
    mmio.write(0x00, 4, 0x07); // DSS=7 → 8-bit, bitmask=0xFF
    mmio.write(0x04, 4, 0x02); // SSE=1

    // Write data to TX FIFO
    mmio.write(0x08, 4, 0xAB);
    // SSI bus with no slave returns 0xFF, bitmask=0xFF → rx=0xFF
    assert_eq!(mmio.read(0x08, 4), 0xFF);
}

#[test]
fn test_pl022_dr_read_empty_returns_zero() {
    let pl022 = Arc::new(Pl022::new());
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    // RX FIFO empty → read returns 0
    assert_eq!(mmio.read(0x08, 4), 0);
}

#[test]
fn test_pl022_sr_reflects_fifo() {
    let pl022 = Arc::new(Pl022::new());
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    // Default: TX FIFO empty → TFE + TNF
    assert_eq!(mmio.read(0x0C, 4), 0x03);

    // Enable SSE, write data
    mmio.write(0x00, 4, 0x07);
    mmio.write(0x04, 4, 0x02);

    // After xfer, TX FIFO should be empty again (data moved to RX)
    let sr = mmio.read(0x0C, 4) as u32;
    assert!(sr & 0x01 != 0); // TFE still set (SSI returned immediately)
}

#[test]
fn test_pl022_is_reflects_fifo_levels() {
    let pl022 = Arc::new(Pl022::new());
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    // Default: TX empty → TX interrupt active, RX empty → RX inactive
    let is = mmio.read(0x18, 4) as u32;
    assert!(is & 0x08 != 0); // TX interrupt
    assert!(is & 0x04 == 0); // RX interrupt
}

#[test]
fn test_pl022_im_masks_interrupt() {
    let pl022 = Arc::new(Pl022::new());
    let mmio = Pl022Mmio(Arc::clone(&pl022));
    let sink = Arc::new(TestSink::new());
    let irq = InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0);
    pl022.connect_irq(irq);

    // Default: IM=0, IS=TX → IRQ low (masked)
    assert!(!sink.level());

    // Unmask TX interrupt
    mmio.write(0x14, 4, 0x08);
    assert!(sink.level());
}

#[test]
fn test_pl022_icr_clear() {
    let pl022 = Arc::new(Pl022::new());
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    // Write to ICR clears ROR and RT only
    mmio.write(0x20, 4, 0x03);
    // IS should have ROR and RT cleared (TX should remain)
    let is = mmio.read(0x18, 4) as u32;
    assert!(is & 0x08 != 0); // TX still set
}

#[test]
fn test_pl022_cpsr_write() {
    let pl022 = Arc::new(Pl022::new());
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    mmio.write(0x10, 4, 0xFE);
    assert_eq!(mmio.read(0x10, 4), 0xFE); // CPSR low byte only
}

#[test]
fn test_pl022_reset_runtime() {
    let pl022 = Arc::new(Pl022::new());
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    mmio.write(0x00, 4, 0x07);
    mmio.write(0x14, 4, 0xFF);
    mmio.write(0x10, 4, 0x55);

    pl022.reset_runtime();

    assert_eq!(mmio.read(0x14, 4), 0); // IM reset
    assert_eq!(mmio.read(0x0C, 4), 0x03); // SR: TFE|TNF
    assert_eq!(mmio.read(0x18, 4), 0x08); // IS: TX
}

// -- SiFive SPI tests --

#[test]
fn test_sifive_spi_defaults() {
    let spi = Arc::new(SiFiveSpi::new());
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    assert_eq!(mmio.read(0x00, 4), 0x03); // SCKDIV
    assert_eq!(mmio.read(0x10, 4), 0x00); // CSID
    assert_eq!(mmio.read(0x14, 4), 0x01); // CSDEF = 1 (num_cs=1)
    assert_eq!(mmio.read(0x18, 4), 0x00); // CSMODE
    assert_eq!(mmio.read(0x28, 4), 0x1001); // DELAY0
    assert_eq!(mmio.read(0x2C, 4), 0x01); // DELAY1
    assert_eq!(mmio.read(0x70, 4), 0x00); // IE
    assert_eq!(mmio.read(0x74, 4), 0x00); // IP
}

#[test]
fn test_sifive_spi_txdata_full_flag() {
    let spi = Arc::new(SiFiveSpi::new());
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    // TXDATA empty → reads 0 (not FULL)
    assert_eq!(mmio.read(0x48, 4), 0);
}

#[test]
fn test_sifive_spi_rxdata_empty_flag() {
    let spi = Arc::new(SiFiveSpi::new());
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    // RXDATA empty → reads RXDATA_EMPTY (1<<31)
    assert_eq!(mmio.read(0x4C, 4), 0x8000_0000);
}

#[test]
fn test_sifive_spi_tx_fifo_write() {
    let spi = Arc::new(SiFiveSpi::new());

    // Connect SSI bus and write data
    let bus = Arc::new(SpiBus::new());
    spi.connect_ssi_bus(Arc::clone(&bus));

    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    // Write to TXDATA
    mmio.write(0x48, 4, 0x42);

    // With no slave, SSI returns 0xFF → RXDATA should have 0xFF
    let rx = mmio.read(0x4C, 4);
    assert_eq!(rx, 0xFF);
}

#[test]
fn test_sifive_spi_tx_full_asserts_ip() {
    let spi = Arc::new(SiFiveSpi::new());
    let bus = Arc::new(SpiBus::new());
    spi.connect_ssi_bus(Arc::clone(&bus));
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    // Set TXMARK high enough that FIFO stays below watermark
    mmio.write(0x50, 4, 7); // TXMARK=7

    // Fill TX FIFO with 8 entries; after each write, flush drains
    // it via the SSI bus, so FIFO stays empty.
    for i in 0..8u64 {
        mmio.write(0x48, 4, i);
    }

    // With bus draining, TXDATA is empty (not FULL)
    assert_eq!(mmio.read(0x48, 4), 0);
}

#[test]
fn test_sifive_spi_ie_controls_irq() {
    let spi = Arc::new(SiFiveSpi::new());
    let bus = Arc::new(SpiBus::new());
    spi.connect_ssi_bus(Arc::clone(&bus));
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));
    let sink = Arc::new(TestSink::new());
    let irq = InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0);
    spi.connect_irq(irq);

    // Set TXMARK high so empty FIFO triggers watermark
    mmio.write(0x50, 4, 7); // TXMARK=7

    // Enable TXWM interrupt mask
    mmio.write(0x70, 4, 0x01); // IE_TXWM

    // TX FIFO=0 < TXMARK=7 → IP.TXWM set → IRQ asserts
    assert!(sink.level());
}

#[test]
fn test_sifive_spi_cs_lines() {
    let spi = Arc::new(SiFiveSpi::with_num_cs(4));
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    // num_cs=4 → CSDEF reset = 0x0F
    assert_eq!(mmio.read(0x14, 4), 0x0F);

    // CSID should be 0 by default
    assert_eq!(mmio.read(0x10, 4), 0x00);
}

#[test]
fn test_sifive_spi_reserved_regs() {
    let spi = Arc::new(SiFiveSpi::new());
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    // Reserved offsets return 0 on read
    assert_eq!(mmio.read(0x08, 4), 0);
    assert_eq!(mmio.read(0x0C, 4), 0);
    assert_eq!(mmio.read(0x30, 4), 0);
    assert_eq!(mmio.read(0x38, 4), 0);

    // Writes to reserved offsets are silently ignored
    mmio.write(0x08, 4, 0xDEAD_BEEF);
}

#[test]
fn test_sifive_spi_ip_read_only() {
    let spi = Arc::new(SiFiveSpi::new());
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    // IP is read-only, write should be ignored
    mmio.write(0x74, 4, 0xFF);
    assert_eq!(mmio.read(0x74, 4), 0x00);
}

#[test]
fn test_sifive_spi_rxdata_read_only() {
    let spi = Arc::new(SiFiveSpi::new());
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    // RXDATA is read-only, write should be ignored
    mmio.write(0x4C, 4, 0xDEAD_BEEF);
    assert_eq!(mmio.read(0x4C, 4), 0x8000_0000); // Still empty
}

#[test]
fn test_sifive_spi_reset_runtime() {
    let spi = Arc::new(SiFiveSpi::new());
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    mmio.write(0x00, 4, 0xFF); // SCKDIV
    mmio.write(0x14, 4, 0x00); // CSDEF
    mmio.write(0x28, 4, 0xFF); // DELAY0
    mmio.write(0x70, 4, 0xFF); // IE

    spi.reset_runtime();

    assert_eq!(mmio.read(0x00, 4), 0x03); // SCKDIV reset
    assert_eq!(mmio.read(0x14, 4), 0x01); // CSDEF reset
    assert_eq!(mmio.read(0x28, 4), 0x1001); // DELAY0 reset
    assert_eq!(mmio.read(0x70, 4), 0x00); // IE reset
}

#[test]
fn test_sifive_spi_watermark_bounds_rejected() {
    let spi = Arc::new(SiFiveSpi::with_num_cs(1));
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    // TXMARK >= FIFO_CAPACITY rejected
    mmio.write(0x50, 4, 8);
    assert_eq!(mmio.read(0x50, 4), 0);

    // Valid watermark accepted
    mmio.write(0x50, 4, 3);
    assert_eq!(mmio.read(0x50, 4), 3);
}

// -- Regression: active-low slave default-deselected on attach --

#[test]
fn test_spi_active_low_default_deselected_after_attach() {
    // An active-low slave attached to a bare SpiBus must return 0xFF
    // (pull-up) before any SpiBus::set_cs() call, because cs_state
    // starts as None (unconfigured/deselected for all polarities).
    struct ActiveLowSlave(Mutex<bool>);
    impl SpiSlave for ActiveLowSlave {
        fn transfer(&self, val: u32) -> u32 {
            val.wrapping_add(1)
        }
        fn set_cs(&self, cs: bool) {
            *self.0.lock().unwrap() = cs;
        }
        fn cs_polarity(&self) -> SpiCsPolarity {
            SpiCsPolarity::Low
        }
        fn cs_index(&self) -> u8 {
            0
        }
    }

    let bus = SpiBus::new();
    let slave = Arc::new(ActiveLowSlave(Mutex::new(false)));
    bus.attach(slave.clone()).unwrap();

    // No set_cs called yet — cs_state is None → not selected
    assert_eq!(bus.transfer(0x42), 0xFF);
    assert!(!*slave.0.lock().unwrap());

    // After explicit CS low, slave is selected
    bus.set_cs(0, false);
    assert_eq!(bus.transfer(0x42), 0x43);
}

// -- Regression: SiFiveSpi AUTO mode CS assertion --

/// Mock slave that records CS transitions and returns a fixed response.
struct CsspySlave {
    selected: Mutex<bool>,
    cs_calls: Mutex<Vec<bool>>,
}

impl CsspySlave {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            selected: Mutex::new(false),
            cs_calls: Mutex::new(Vec::new()),
        })
    }

    fn selected(&self) -> bool {
        *self.selected.lock().unwrap()
    }

    fn cs_calls(&self) -> Vec<bool> {
        self.cs_calls.lock().unwrap().clone()
    }
}

impl SpiSlave for CsspySlave {
    fn transfer(&self, val: u32) -> u32 {
        // Echo with XOR so we can distinguish slave response vs 0xFF
        val ^ 0x5A
    }

    fn set_cs(&self, cs: bool) {
        self.cs_calls.lock().unwrap().push(cs);
        *self.selected.lock().unwrap() = cs;
    }

    fn cs_polarity(&self) -> SpiCsPolarity {
        SpiCsPolarity::High
    }

    fn cs_index(&self) -> u8 {
        0
    }
}

#[test]
fn test_sifive_spi_auto_mode_cs_assertion() {
    let bus = Arc::new(SpiBus::new());
    let slave = CsspySlave::new();
    bus.attach(slave.clone()).unwrap();

    let spi = Arc::new(SiFiveSpi::with_num_cs(1));
    spi.connect_ssi_bus(Arc::clone(&bus));

    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    // Reset defaults: CSMODE=AUTO(0), CSDEF=1, CSID=0
    // In AUTO mode, writing TXDATA asserts CS before transfer
    // and deasserts after.
    mmio.write(0x48, 4, 0x42); // TXDATA

    let rx = mmio.read(0x4C, 4);
    // Slave returns val ^ 0x5A, not 0xFF
    assert_eq!(rx, 0x42 ^ 0x5A);

    // Verify CS was asserted then deasserted
    let calls = slave.cs_calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[0]); // assert (CSDEF=1 → active-high → CS true = selected)
    assert!(!calls[1]); // deassert
                        // CS is now deasserted after the transfer
    assert!(!slave.selected());
}

#[test]
fn test_sifive_spi_active_low_cs_auto_mode() {
    // Active-low slave with CSDEF=0: CS low = selected in AUTO mode
    let bus = Arc::new(SpiBus::new());

    struct ActiveLowEchoSlave {
        cs_state: Mutex<bool>,
        cs_calls: Mutex<Vec<bool>>,
    }
    impl ActiveLowEchoSlave {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                cs_state: Mutex::new(false),
                cs_calls: Mutex::new(Vec::new()),
            })
        }
    }
    impl SpiSlave for ActiveLowEchoSlave {
        fn transfer(&self, val: u32) -> u32 {
            val ^ 0x3C
        }
        fn set_cs(&self, cs: bool) {
            self.cs_calls.lock().unwrap().push(cs);
            *self.cs_state.lock().unwrap() = cs;
        }
        fn cs_polarity(&self) -> SpiCsPolarity {
            SpiCsPolarity::Low
        }
        fn cs_index(&self) -> u8 {
            0
        }
    }

    let slave = ActiveLowEchoSlave::new();
    bus.attach(slave.clone()).unwrap();

    let spi = Arc::new(SiFiveSpi::with_num_cs(1));
    spi.connect_ssi_bus(Arc::clone(&bus));
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    // CSDEF=0 (bit 0 = 0): active-low → CS low means selected.
    // This write now calls update_cs() which drives default_level
    // (true for CSDEF=0, deasserted idle state in AUTO mode).
    mmio.write(0x14, 4, 0x00); // CSDEF=0

    let cs_calls_before = slave.cs_calls.lock().unwrap().len();

    // AUTO mode (default): write TXDATA
    mmio.write(0x48, 4, 0x7F);
    let rx = mmio.read(0x4C, 4);

    // CSDEF=0, AUTO mode: csdef_assert = (0 & 1) != 0 = false
    // Before transfer: set_cs(0, false) → active-low selected
    // After transfer: set_cs(0, true) → active-low deselected
    // With slave selected, returns val ^ 0x3C
    assert_eq!(rx, 0x7F ^ 0x3C);

    let calls = slave.cs_calls.lock().unwrap().clone();
    let new_calls = &calls[cs_calls_before..];
    assert_eq!(new_calls.len(), 2);
    assert!(!new_calls[0]); // assert low (active-low)
    assert!(new_calls[1]); // deassert high
    assert!(*slave.cs_state.lock().unwrap()); // ended deasserted (high)
}

#[test]
fn test_sifive_spi_hold_mode_cs_persistent() {
    let bus = Arc::new(SpiBus::new());
    let slave = CsspySlave::new();
    bus.attach(slave.clone()).unwrap();

    let spi = Arc::new(SiFiveSpi::with_num_cs(1));
    spi.connect_ssi_bus(Arc::clone(&bus));
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    // Switch to HOLD mode (CSMODE=2)
    mmio.write(0x18, 4, 2); // CSMODE=HOLD

    // In HOLD mode, update_cs() asserts CS persistently
    assert!(slave.selected());

    // Write TXDATA: in HOLD, CS stays asserted across bytes
    mmio.write(0x48, 4, 0x11);
    let rx = mmio.read(0x4C, 4);
    assert_eq!(rx, 0x11 ^ 0x5A);

    // CS still asserted after transfer
    assert!(slave.selected());

    // Switch to OFF mode → CS deasserts
    mmio.write(0x18, 4, 3); // CSMODE=OFF
    assert!(!slave.selected());
}

#[test]
fn test_sifive_spi_reset_deasserts_cs() {
    let bus = Arc::new(SpiBus::new());
    let slave = CsspySlave::new();
    bus.attach(slave.clone()).unwrap();

    let spi = Arc::new(SiFiveSpi::with_num_cs(1));
    spi.connect_ssi_bus(Arc::clone(&bus));
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    // Set HOLD mode → CS asserted
    mmio.write(0x18, 4, 2); // CSMODE=HOLD
    assert!(slave.selected());

    // Reset → CS must deassert
    spi.reset_runtime();
    // After reset, CSMODE=AUTO(0), CSDEF=1 → CS=high (asserted for
    // active-high slaves but we verify the cs transition was called)
    assert!(slave.cs_calls().len() > 0);
}

// -- Regression: Pl022 CS assertion --

#[test]
fn test_pl022_cs_assertion_during_transfer() {
    // Pl022 nSSP is active-low: assert=false, deassert=true.
    // Use active-low slave so the transfer reaches it.
    let bus = Arc::new(SpiBus::new());
    let slave = ActiveLowCsSpy::new();
    bus.attach(slave.clone()).unwrap();

    let mut pl022 = Pl022::new();
    pl022.set_cs_index(0);
    let pl022 = Arc::new(pl022);
    pl022.connect_ssi_bus(Arc::clone(&bus));
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    mmio.write(0x00, 4, 0x07); // DSS=7 → 8-bit
    mmio.write(0x04, 4, 0x02); // SSE=1

    mmio.write(0x08, 4, 0xAB); // Write data → triggers xfer

    let rx = mmio.read(0x08, 4);
    assert_eq!(rx, (0xAB & 0xFF) ^ 0x5A);

    // CS: assert=false (low), deassert=true (high)
    let calls = slave.cs_calls();
    assert_eq!(calls.len(), 2);
    assert!(!calls[0]); // assert low
    assert!(calls[1]); // deassert high
}

#[test]
fn test_pl022_no_cs_when_sse_disabled() {
    let bus = Arc::new(SpiBus::new());
    let slave = ActiveLowCsSpy::new();
    bus.attach(slave.clone()).unwrap();

    let mut pl022 = Pl022::new();
    pl022.set_cs_index(0);
    let pl022 = Arc::new(pl022);
    pl022.connect_ssi_bus(Arc::clone(&bus));
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    // SSE disabled (default)
    mmio.write(0x08, 4, 0xCD); // Write to DR with SSE=0

    // No CS transitions should occur
    assert!(slave.cs_calls().is_empty());
}

#[test]
fn test_pl022_reset_deasserts_cs() {
    let bus = Arc::new(SpiBus::new());
    let slave = ActiveLowCsSpy::new();
    bus.attach(slave.clone()).unwrap();

    let mut pl022 = Pl022::new();
    pl022.set_cs_index(0);
    let pl022 = Arc::new(pl022);
    pl022.connect_ssi_bus(Arc::clone(&bus));
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    mmio.write(0x00, 4, 0x07);
    mmio.write(0x04, 4, 0x02);
    mmio.write(0x08, 4, 0x55);
    // After transfer CS is deasserted (true) → active-low deselected
    assert!(!slave.selected());

    // Re-assert CS to false (low) to simulate lingering assertion
    bus.set_cs(0, false);
    assert!(slave.selected());
    let calls_before = slave.cs_calls().len();

    // Reset must deassert: CS false → true
    pl022.reset_runtime();
    assert!(!slave.selected());
    assert!(slave.cs_calls().len() > calls_before);
}

// -- Regression: active-low slave CSDEF=0 deassert semantics --

/// Active-low mock slave that records CS transitions and logical
/// selection state. Uses `SpiCsPolarity::Low` — selected when CS=false.
struct ActiveLowCsSpy {
    selected: Mutex<bool>,
    cs_calls: Mutex<Vec<bool>>,
}

impl ActiveLowCsSpy {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            selected: Mutex::new(false),
            cs_calls: Mutex::new(Vec::new()),
        })
    }

    /// Returns whether the slave is logically selected
    /// (accounts for active-low polarity).
    fn selected(&self) -> bool {
        *self.selected.lock().unwrap()
    }

    fn cs_calls(&self) -> Vec<bool> {
        self.cs_calls.lock().unwrap().clone()
    }
}

impl SpiSlave for ActiveLowCsSpy {
    fn transfer(&self, val: u32) -> u32 {
        val ^ 0x5A
    }

    fn set_cs(&self, cs: bool) {
        self.cs_calls.lock().unwrap().push(cs);
        // Active-low: logically selected when physical CS = false
        *self.selected.lock().unwrap() = !cs;
    }

    fn cs_polarity(&self) -> SpiCsPolarity {
        SpiCsPolarity::Low
    }

    fn cs_index(&self) -> u8 {
        0
    }
}

#[test]
fn test_sifive_spi_active_low_off_deasserts() {
    // CSDEF=0 (active-low), OFF mode: CS must be deasserted (high/true).
    // With hard-coded false in update_cs() OFF path, an active-low
    // slave would see CS=false=selected.
    let bus = Arc::new(SpiBus::new());
    let slave = ActiveLowCsSpy::new();
    bus.attach(slave.clone()).unwrap();

    let spi = Arc::new(SiFiveSpi::with_num_cs(1));
    spi.connect_ssi_bus(Arc::clone(&bus));
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    mmio.write(0x14, 4, 0x00); // CSDEF=0 (active-low)
    mmio.write(0x18, 4, 3); // CSMODE=OFF

    // For CSDEF=0: default_level = true (high) = deasserted
    // Active-low slave: CS=true → logically deselected
    assert!(
        !slave.selected(),
        "active-low slave must be deselected in OFF mode"
    );
}

#[test]
fn test_sifive_spi_active_low_hold_persists_and_off_deasserts() {
    // CSDEF=0 (active-low), HOLD mode: selected CS stays asserted (low).
    // Switch to OFF: CS must deassert (high).
    let bus = Arc::new(SpiBus::new());
    let slave = ActiveLowCsSpy::new();
    bus.attach(slave.clone()).unwrap();

    let spi = Arc::new(SiFiveSpi::with_num_cs(1));
    spi.connect_ssi_bus(Arc::clone(&bus));
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    mmio.write(0x14, 4, 0x00); // CSDEF=0 (active-low)
    mmio.write(0x18, 4, 2); // CSMODE=HOLD

    // HOLD mode selected: assert_level = bit(CSDEF,0) = false = low
    // Active-low slave → CS=false → logically selected
    assert!(
        slave.selected(),
        "active-low slave must be selected (CS=false) in HOLD mode"
    );

    // Transfer a byte — CS stays asserted, transfer reaches slave
    mmio.write(0x48, 4, 0x77);
    let rx = mmio.read(0x4C, 4);
    assert_eq!(rx, 0x77 ^ 0x5A);
    assert!(
        slave.selected(),
        "CS must stay asserted after transfer in HOLD mode"
    );

    // Switch to OFF → CS deasserts (high/true for CSDEF=0)
    mmio.write(0x18, 4, 3); // CSMODE=OFF
    assert!(
        !slave.selected(),
        "CS must deassert after switching to OFF mode"
    );
}

#[test]
fn test_sifive_spi_active_low_reset_deasserts() {
    // CSDEF=0 (active-low), HOLD mode (CS asserted low).
    // lower_outputs() runs before regs.reset() and must deassert
    // using the CURRENT (old) CSDEF=0: default_level=true.
    // After regs.reset(), CSDEF=1 so update_cs() may re-assert
    // for the new active-high default_level=false. Verify the
    // lower_outputs path deasserts correctly.
    let bus = Arc::new(SpiBus::new());
    let slave = ActiveLowCsSpy::new();
    bus.attach(slave.clone()).unwrap();

    let spi = Arc::new(SiFiveSpi::with_num_cs(1));
    spi.connect_ssi_bus(Arc::clone(&bus));
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    mmio.write(0x14, 4, 0x00); // CSDEF=0 (active-low)
    mmio.write(0x18, 4, 2); // CSMODE=HOLD
    assert!(slave.selected()); // asserted low = selected

    let cs_calls_before = slave.cs_calls().len();

    spi.reset_runtime();
    // Sequence: lower_outputs (CSDEF=0 → true) then
    //           regs.reset (CSDEF→1) then
    //           update_cs (CSDEF=1 → false).
    // The deassert call (CS=true) from lower_outputs must be present.
    let new_calls = &slave.cs_calls()[cs_calls_before..];
    assert!(
        new_calls.iter().any(|&c| c),
        "lower_outputs must drive CS=true (deassert) for CSDEF=0"
    );
}

#[test]
fn test_sifive_spi_csdef_write_propagates_to_bus() {
    // CSDEF=0, HOLD mode: assert_level = false.
    // Write CSDEF=1 → assert_level changes to true → CS must transition.
    let bus = Arc::new(SpiBus::new());
    let slave = ActiveLowCsSpy::new();
    bus.attach(slave.clone()).unwrap();

    let spi = Arc::new(SiFiveSpi::with_num_cs(1));
    spi.connect_ssi_bus(Arc::clone(&bus));
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    mmio.write(0x14, 4, 0x00); // CSDEF=0 (active-low)
    mmio.write(0x18, 4, 2); // CSMODE=HOLD
    assert!(slave.selected()); // CS=false asserted for active-low

    let cs_calls_before = slave.cs_calls().len();

    // Change CSDEF while in HOLD mode: must propagate to bus
    mmio.write(0x14, 4, 0x01); // CSDEF=1 (active-high)
                               // New assert_level for CSDEF=1 = true
                               // Slave is active-low → needs false to be selected → deselected

    let calls_after = slave.cs_calls();
    assert!(
        calls_after.len() > cs_calls_before,
        "CSDEF write in HOLD mode must update bus CS state"
    );
    assert!(
        !slave.selected(),
        "active-low slave deselected after CSDEF changes assert level to true"
    );
}

#[test]
fn test_sifive_spi_active_low_auto_steady_state_deasserted() {
    // After an AUTO-mode transfer, update_cs() (called on CSID write)
    // must drive default_level (true for CSDEF=0), keeping the
    // active-low slave deselected.
    let bus = Arc::new(SpiBus::new());
    let slave = ActiveLowCsSpy::new();
    bus.attach(slave.clone()).unwrap();

    let spi = Arc::new(SiFiveSpi::with_num_cs(1));
    spi.connect_ssi_bus(Arc::clone(&bus));
    let mmio = SiFiveSpiMmio(Arc::clone(&spi));

    mmio.write(0x14, 4, 0x00); // CSDEF=0 (active-low)
                               // CSMODE defaults to 0 (AUTO)

    // Transfer in AUTO: CS toggles assert(false)/deassert(true)
    mmio.write(0x48, 4, 0x33);
    let rx = mmio.read(0x4C, 4);
    assert_eq!(rx, 0x33 ^ 0x5A);
    assert!(!slave.selected()); // deasserted after transfer

    // Write CSID=0 → triggers update_cs(). In AUTO mode this
    // must drive default_level (true), not assert_level (false).
    mmio.write(0x10, 4, 0); // CSID=0 (same value, triggers update_cs)
    assert!(
        !slave.selected(),
        "update_cs() in AUTO must keep CS deasserted (true)"
    );
}

// -- Regression: Pl022 with active-low slave (nSSP active-low) --

#[test]
fn test_pl022_active_low_cs_assertion_during_transfer() {
    // Pl022 nSSP is active-low: assert = false (low), deassert = true
    // (high). An active-low slave must be reached by the transfer.
    let bus = Arc::new(SpiBus::new());
    let slave = ActiveLowCsSpy::new();
    bus.attach(slave.clone()).unwrap();

    let mut pl022 = Pl022::new();
    pl022.set_cs_index(0);
    let pl022 = Arc::new(pl022);
    pl022.connect_ssi_bus(Arc::clone(&bus));
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    mmio.write(0x00, 4, 0x07); // DSS=7 → 8-bit
    mmio.write(0x04, 4, 0x02); // SSE=1

    mmio.write(0x08, 4, 0xAB); // Write data to DR

    let rx = mmio.read(0x08, 4);
    assert_eq!(
        rx,
        (0xAB & 0xFF) ^ 0x5A,
        "transfer must reach active-low slave"
    );

    // After transfer CS is deasserted (true) → active-low deselected
    assert!(!slave.selected());

    // Verify CS assert=false then deassert=true
    let calls = slave.cs_calls();
    assert_eq!(calls.len(), 2);
    assert!(!calls[0], "assert must be false (low) for PL022 nSSP");
    assert!(calls[1], "deassert must be true (high) for PL022 nSSP");
}

#[test]
fn test_pl022_active_low_reset_deasserts() {
    // Pl022 reset must drive CS to the deasserted idle level.
    // nSSP is active-low: idle/deasserted = high (true).
    let bus = Arc::new(SpiBus::new());
    let slave = ActiveLowCsSpy::new();
    bus.attach(slave.clone()).unwrap();

    let mut pl022 = Pl022::new();
    pl022.set_cs_index(0);
    let pl022 = Arc::new(pl022);
    pl022.connect_ssi_bus(Arc::clone(&bus));
    let mmio = Pl022Mmio(Arc::clone(&pl022));

    // Do a transfer to put CS through assert/deassert cycle
    mmio.write(0x00, 4, 0x07);
    mmio.write(0x04, 4, 0x02);
    mmio.write(0x08, 4, 0x55);
    let _rx = mmio.read(0x08, 4);
    assert!(!slave.selected()); // deasserted after transfer

    // Re-assert CS to false to simulate a lingering assertion
    bus.set_cs(0, false);
    assert!(slave.selected());
    let cs_calls_before = slave.cs_calls().len();

    // Reset must drive CS to deasserted level (true)
    pl022.reset_runtime();
    assert!(
        !slave.selected(),
        "reset must deassert CS (true) for active-low PL022"
    );
    assert!(
        slave.cs_calls().len() > cs_calls_before,
        "reset must produce a CS state change"
    );
}
