use std::sync::{Arc, Mutex};

use machina_hw_ssi::{SpiBus, SpiCsPolarity, SpiSlave};

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
