use std::sync::{Arc, Mutex};

use machina_hw_i2c::eeprom_at24c::{At24cEeprom, At24cEepromConfig};
use machina_hw_i2c::smbus_eeprom::SmbusEeprom;
use machina_hw_i2c::{I2cBus, I2cError, I2cEvent, I2cSlave, I2C_BROADCAST};
use machina_hw_storage::MemBackend;

/// Mock I2C slave with configurable address and recorded events.
struct MockI2cSlave {
    addr: u8,
    events: Mutex<Vec<I2cEvent>>,
    send_data: Mutex<Vec<u8>>,
    recv_data: Mutex<Vec<u8>>,
    recv_pos: Mutex<usize>,
    nack_on: Mutex<Option<u8>>,
    nack_all: Mutex<bool>,
}

impl MockI2cSlave {
    fn new(addr: u8) -> Arc<Self> {
        Arc::new(Self {
            addr,
            events: Mutex::new(Vec::new()),
            send_data: Mutex::new(Vec::new()),
            recv_data: Mutex::new(vec![0x42, 0x43, 0x44]),
            recv_pos: Mutex::new(0),
            nack_on: Mutex::new(None),
            nack_all: Mutex::new(false),
        })
    }

    fn set_nack_all(&self, nack: bool) {
        *self.nack_all.lock().unwrap() = nack;
    }

    fn events(&self) -> Vec<I2cEvent> {
        self.events.lock().unwrap().clone()
    }

    fn sent(&self) -> Vec<u8> {
        self.send_data.lock().unwrap().clone()
    }
}

impl I2cSlave for MockI2cSlave {
    fn address(&self) -> u8 {
        self.addr
    }

    fn event(&self, event: I2cEvent) -> Result<(), I2cError> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }

    fn send(&self, data: u8) -> Result<(), I2cError> {
        if *self.nack_all.lock().unwrap() {
            return Err(I2cError::Nack);
        }
        if *self.nack_on.lock().unwrap() == Some(data) {
            return Err(I2cError::Nack);
        }
        self.send_data.lock().unwrap().push(data);
        Ok(())
    }

    fn recv(&self) -> u8 {
        let mut pos = self.recv_pos.lock().unwrap();
        let data = self.recv_data.lock().unwrap();
        if *pos < data.len() {
            let val = data[*pos];
            *pos += 1;
            val
        } else {
            0xFF
        }
    }
}

// -- Positive Tests --

#[test]
fn test_at24c_lifecycle_and_mom_identity() {
    let eeprom = Arc::new(
        At24cEeprom::new(
            MemBackend::new(vec![0xff; 256], false),
            At24cEepromConfig::default(),
        )
        .unwrap(),
    );
    assert!(!eeprom.realized());
    eeprom.with_mdevice(|device| assert_eq!(device.local_id(), "at24c"));
    assert_eq!(eeprom.object_info().local_id, "at24c");

    eeprom.realize().unwrap();
    assert!(eeprom.realized());
    let err = eeprom.realize().unwrap_err();
    assert!(
        err.to_string().contains("already realized"),
        "unexpected second-realize error: {err}"
    );

    eeprom.unrealize().unwrap();
    assert!(!eeprom.realized());
    let err = eeprom.unrealize().unwrap_err();
    assert!(
        err.to_string().contains("not realized"),
        "unexpected second-unrealize error: {err}"
    );
}

#[test]
fn test_at24c_sets_pointer_and_reads_sequential_bytes() {
    let bus = I2cBus::new();
    let eeprom = Arc::new(
        At24cEeprom::new(
            MemBackend::new((0..=0xff).collect(), false),
            At24cEepromConfig::default(),
        )
        .unwrap(),
    );
    bus.attach(eeprom).unwrap();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x10).unwrap();
    bus.start_transfer(0x50, true).unwrap();

    assert_eq!(bus.recv(), 0x10);
    assert_eq!(bus.recv(), 0x11);
    assert_eq!(bus.recv(), 0x12);

    bus.end_transfer();
}

#[test]
fn test_at24c_writes_bytes_after_pointer() {
    let bus = I2cBus::new();
    let eeprom = Arc::new(
        At24cEeprom::new(
            MemBackend::new(vec![0xff; 256], false),
            At24cEepromConfig::default(),
        )
        .unwrap(),
    );
    bus.attach(eeprom).unwrap();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x20).unwrap();
    bus.send(0xaa).unwrap();
    bus.send(0xbb).unwrap();
    bus.send(0xcc).unwrap();
    bus.end_transfer();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x20).unwrap();
    bus.start_transfer(0x50, true).unwrap();

    assert_eq!(bus.recv(), 0xaa);
    assert_eq!(bus.recv(), 0xbb);
    assert_eq!(bus.recv(), 0xcc);
    assert_eq!(bus.recv(), 0xff);

    bus.end_transfer();
}

#[test]
fn test_at24c_write_pointer_wraps_at_eeprom_size() {
    let bus = I2cBus::new();
    let eeprom = Arc::new(
        At24cEeprom::new(
            MemBackend::new(vec![0xff; 8], false),
            At24cEepromConfig {
                size: 8,
                page_size: 4,
                ..At24cEepromConfig::default()
            },
        )
        .unwrap(),
    );
    bus.attach(eeprom).unwrap();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x06).unwrap();
    bus.send(0xaa).unwrap();
    bus.send(0xbb).unwrap();
    bus.send(0xcc).unwrap();
    bus.end_transfer();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x06).unwrap();
    bus.start_transfer(0x50, true).unwrap();

    assert_eq!(bus.recv(), 0xaa);
    assert_eq!(bus.recv(), 0xbb);
    assert_eq!(bus.recv(), 0xcc);
    assert_eq!(bus.recv(), 0xff);

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x00).unwrap();
    bus.start_transfer(0x50, true).unwrap();

    assert_eq!(bus.recv(), 0xcc);
    assert_eq!(bus.recv(), 0xff);

    bus.end_transfer();
}

#[test]
fn test_at24c_writes_increment_across_page_boundary() {
    let bus = I2cBus::new();
    let eeprom = Arc::new(
        At24cEeprom::new(
            MemBackend::new(vec![0xff; 256], false),
            At24cEepromConfig {
                page_size: 4,
                ..At24cEepromConfig::default()
            },
        )
        .unwrap(),
    );
    bus.attach(eeprom).unwrap();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x06).unwrap();
    bus.send(0xaa).unwrap();
    bus.send(0xbb).unwrap();
    bus.send(0xcc).unwrap();
    bus.end_transfer();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x06).unwrap();
    bus.start_transfer(0x50, true).unwrap();

    assert_eq!(bus.recv(), 0xaa);
    assert_eq!(bus.recv(), 0xbb);
    assert_eq!(bus.recv(), 0xcc);
    assert_eq!(bus.recv(), 0xff);

    bus.end_transfer();
}

#[test]
fn test_at24c_readonly_nacks_data_write() {
    let bus = I2cBus::new();
    let eeprom = Arc::new(
        At24cEeprom::new(
            MemBackend::new(vec![0x55; 256], true),
            At24cEepromConfig::default(),
        )
        .unwrap(),
    );
    bus.attach(eeprom).unwrap();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x20).unwrap();
    assert_eq!(bus.send(0xaa).unwrap_err(), I2cError::Nack);
    bus.end_transfer();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x20).unwrap();
    bus.start_transfer(0x50, true).unwrap();

    assert_eq!(bus.recv(), 0x55);

    bus.end_transfer();
}

#[test]
fn test_smbus_eeprom_reads_byte_selected_by_command() {
    let bus = I2cBus::new();
    let eeprom = Arc::new(
        SmbusEeprom::new(0x54, MemBackend::new((0..=0xff).collect(), false))
            .unwrap(),
    );
    bus.attach(eeprom).unwrap();

    bus.start_transfer(0x54, false).unwrap();
    bus.send(0x33).unwrap();
    bus.start_transfer(0x54, true).unwrap();

    assert_eq!(bus.recv(), 0x33);
    assert_eq!(bus.recv(), 0x34);

    bus.end_transfer();
}

#[test]
fn test_smbus_eeprom_lifecycle_and_mom_identity() {
    let eeprom = Arc::new(
        SmbusEeprom::new(0x54, MemBackend::new(vec![0xff; 256], false))
            .unwrap(),
    );
    assert!(!eeprom.realized());
    eeprom.with_mdevice(|device| assert_eq!(device.local_id(), "smbus-eeprom"));
    assert_eq!(eeprom.object_info().local_id, "smbus-eeprom");

    eeprom.realize().unwrap();
    assert!(eeprom.realized());
    let err = eeprom.realize().unwrap_err();
    assert!(
        err.to_string().contains("already realized"),
        "unexpected second-realize error: {err}"
    );

    eeprom.unrealize().unwrap();
    assert!(!eeprom.realized());
    let err = eeprom.unrealize().unwrap_err();
    assert!(
        err.to_string().contains("not realized"),
        "unexpected second-unrealize error: {err}"
    );
}

#[test]
fn test_smbus_eeprom_writes_byte_after_command() {
    let bus = I2cBus::new();
    let eeprom = Arc::new(
        SmbusEeprom::new(0x54, MemBackend::new(vec![0xff; 256], false))
            .unwrap(),
    );
    bus.attach(eeprom).unwrap();

    bus.start_transfer(0x54, false).unwrap();
    bus.send(0x44).unwrap();
    bus.send(0x5a).unwrap();
    bus.end_transfer();

    bus.start_transfer(0x54, false).unwrap();
    bus.send(0x44).unwrap();
    bus.start_transfer(0x54, true).unwrap();

    assert_eq!(bus.recv(), 0x5a);

    bus.end_transfer();
}

#[test]
fn test_i2c_bus_new() {
    let bus = I2cBus::new();
    assert_eq!(bus.slave_count(), 0);
    assert!(!bus.busy());
}

#[test]
fn test_i2c_attach_detach() {
    let bus = I2cBus::new();
    let dev = MockI2cSlave::new(0x50);
    assert!(bus.attach(dev.clone()).is_ok());
    assert_eq!(bus.slave_count(), 1);

    let removed = bus.detach(0x50);
    assert!(removed.is_some());
    assert_eq!(bus.slave_count(), 0);
}

#[test]
fn test_i2c_start_send() {
    let bus = I2cBus::new();
    let dev = MockI2cSlave::new(0x50);
    bus.attach(dev.clone()).unwrap();

    assert!(bus.start_transfer(0x50, false).is_ok());
    let events = dev.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], I2cEvent::StartSend);
}

#[test]
fn test_i2c_start_recv() {
    let bus = I2cBus::new();
    let dev = MockI2cSlave::new(0x50);
    bus.attach(dev.clone()).unwrap();

    assert!(bus.start_transfer(0x50, true).is_ok());
    let events = dev.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], I2cEvent::StartRecv);
}

#[test]
fn test_i2c_send_byte() {
    let bus = I2cBus::new();
    let dev = MockI2cSlave::new(0x50);
    bus.attach(dev.clone()).unwrap();

    bus.start_transfer(0x50, false).unwrap();
    assert!(bus.send(0xA5).is_ok());
    assert_eq!(dev.sent(), vec![0xA5]);
}

#[test]
fn test_i2c_recv_byte() {
    let bus = I2cBus::new();
    let dev = MockI2cSlave::new(0x50);
    bus.attach(dev.clone()).unwrap();

    bus.start_transfer(0x50, true).unwrap();
    assert_eq!(bus.recv(), 0x42);
    assert_eq!(bus.recv(), 0x43);
    assert_eq!(bus.recv(), 0x44);
}

#[test]
fn test_i2c_end_transfer_sends_finish() {
    let bus = I2cBus::new();
    let dev = MockI2cSlave::new(0x50);
    bus.attach(dev.clone()).unwrap();

    bus.start_transfer(0x50, false).unwrap();
    bus.end_transfer();

    let events = dev.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], I2cEvent::StartSend);
    assert_eq!(events[1], I2cEvent::Finish);

    assert!(!bus.busy());
}

#[test]
fn test_i2c_nack_sends_nack_event() {
    let bus = I2cBus::new();
    let dev = MockI2cSlave::new(0x50);
    bus.attach(dev.clone()).unwrap();

    bus.start_transfer(0x50, false).unwrap();
    bus.nack();

    let events = dev.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1], I2cEvent::Nack);
}

#[test]
fn test_i2c_broadcast() {
    let bus = I2cBus::new();
    let d1 = MockI2cSlave::new(0x10);
    let d2 = MockI2cSlave::new(0x20);
    bus.attach(d1.clone()).unwrap();
    bus.attach(d2.clone()).unwrap();

    assert!(bus.start_transfer(I2C_BROADCAST, false).is_ok());

    // Both devices receive the start event
    assert_eq!(d1.events().len(), 1);
    assert_eq!(d2.events().len(), 1);
    assert_eq!(d1.events()[0], I2cEvent::StartSend);
    assert_eq!(d2.events()[0], I2cEvent::StartSend);
}

#[test]
fn test_i2c_broadcast_recv_returns_0xff() {
    let bus = I2cBus::new();
    let d1 = MockI2cSlave::new(0x10);
    bus.attach(d1.clone()).unwrap();

    bus.start_transfer(I2C_BROADCAST, true).unwrap();
    // Broadcast recv returns 0xFF (can't read from multiple devices)
    assert_eq!(bus.recv(), 0xFF);
}

// -- Negative Tests --

#[test]
fn test_i2c_start_transfer_no_device() {
    let bus = I2cBus::new();
    assert_eq!(
        bus.start_transfer(0x50, false).unwrap_err(),
        I2cError::NoDevice
    );
}

#[test]
fn test_i2c_send_without_start() {
    let bus = I2cBus::new();
    // No transfer started -> no device addressed
    assert_eq!(bus.send(0x00).unwrap_err(), I2cError::NoDevice);
}

#[test]
fn test_i2c_attach_duplicate_address_fails() {
    let bus = I2cBus::new();
    let d1 = MockI2cSlave::new(0x50);
    let d2 = MockI2cSlave::new(0x50);
    bus.attach(d1).unwrap();
    assert!(bus.attach(d2).is_err());
}

#[test]
fn test_i2c_broadcast_can_have_multiple() {
    // Broadcast slaves can coexist at the same address
    let bus = I2cBus::new();
    let d1 = MockI2cSlave::new(I2C_BROADCAST);
    let d2 = MockI2cSlave::new(I2C_BROADCAST);
    assert!(bus.attach(d1).is_ok());
    assert!(bus.attach(d2).is_ok());
    assert_eq!(bus.slave_count(), 2);
}

#[test]
fn test_i2c_recv_without_device_returns_0xff() {
    let bus = I2cBus::new();
    assert_eq!(bus.recv(), 0xFF);
}

#[test]
fn test_i2c_detach_nonexistent_returns_none() {
    let bus = I2cBus::new();
    assert!(bus.detach(0x50).is_none());
}

#[test]
fn test_i2c_busy_after_transfer() {
    let bus = I2cBus::new();
    let dev = MockI2cSlave::new(0x50);
    bus.attach(dev).unwrap();

    assert!(!bus.busy());
    bus.start_transfer(0x50, false).unwrap();
    assert!(bus.busy());
    bus.end_transfer();
    assert!(!bus.busy());
}

#[test]
fn test_i2c_directed_send_nack_returns_error() {
    let bus = I2cBus::new();
    let dev = MockI2cSlave::new(0x50);
    dev.set_nack_all(true);
    bus.attach(dev.clone()).unwrap();

    bus.start_transfer(0x50, false).unwrap();
    let result = bus.send(0x10);
    assert_eq!(result.unwrap_err(), I2cError::Nack);
    // Device should not have recorded the byte
    assert!(dev.sent().is_empty());
}

#[test]
fn test_i2c_broadcast_mixed_ack_nack_returns_nack() {
    let bus = I2cBus::new();
    let d1 = MockI2cSlave::new(0x10);
    let d2 = MockI2cSlave::new(0x20);
    d2.set_nack_all(true); // d2 NACKs everything
    let d3 = MockI2cSlave::new(0x30);
    bus.attach(d1.clone()).unwrap();
    bus.attach(d2.clone()).unwrap();
    bus.attach(d3.clone()).unwrap();

    bus.start_transfer(I2C_BROADCAST, false).unwrap();
    // d2 NACKs → overall error even though d1 and d3 would ACK
    let result = bus.send(0x42);
    assert_eq!(result.unwrap_err(), I2cError::Nack);
}

#[test]
fn test_i2c_repeated_start_with_nack_fails() {
    let bus = I2cBus::new();
    let dev = MockI2cSlave::new(0x50);
    dev.set_nack_all(true);
    bus.attach(dev.clone()).unwrap();

    // First transfer works (event precedes send)
    bus.start_transfer(0x50, false).unwrap();
    // Send fails because device NACKs
    assert_eq!(bus.send(0x10).unwrap_err(), I2cError::Nack);
    // Bus is still marked busy until end_transfer
    assert!(bus.busy());
    bus.end_transfer();
    assert!(!bus.busy());
}

// -- Regression: repeated START changes address --

#[test]
fn test_i2c_repeated_start_changes_address() {
    // Repeated START must finish the old device (A) and route new
    // events/bytes to the new device (B).
    let bus = I2cBus::new();
    let dev_a = MockI2cSlave::new(0x30);
    let dev_b = MockI2cSlave::new(0x40);
    bus.attach(dev_a.clone()).unwrap();
    bus.attach(dev_b.clone()).unwrap();

    // First transfer to A
    bus.start_transfer(0x30, false).unwrap();
    assert_eq!(dev_a.events().len(), 1);
    assert_eq!(dev_a.events()[0], I2cEvent::StartSend);
    assert_eq!(dev_b.events().len(), 0);

    // Send a byte to A
    bus.send(0xAA).unwrap();
    assert_eq!(dev_a.sent(), vec![0xAA]);

    // Repeated START to B (recv) — A must receive Finish,
    // B must receive StartRecv
    bus.start_transfer(0x40, true).unwrap();
    assert_eq!(dev_a.events().len(), 2);
    assert_eq!(dev_a.events()[1], I2cEvent::Finish);
    assert_eq!(dev_b.events().len(), 1);
    assert_eq!(dev_b.events()[0], I2cEvent::StartRecv);

    // Subsequent bytes go to B, not A
    assert_eq!(bus.recv(), 0x42);
    assert_eq!(bus.recv(), 0x43);

    bus.end_transfer();
    assert_eq!(dev_b.events().len(), 2);
    assert_eq!(dev_b.events()[1], I2cEvent::Finish);
}

#[test]
fn test_i2c_repeated_start_same_address_no_duplicate_finish() {
    // Repeated START to the same address should not duplicate Finish
    // for a device that was never selected.
    let bus = I2cBus::new();
    let dev = MockI2cSlave::new(0x50);
    bus.attach(dev.clone()).unwrap();

    bus.start_transfer(0x50, false).unwrap();
    assert_eq!(dev.events().len(), 1);
    assert_eq!(dev.events()[0], I2cEvent::StartSend);

    bus.send(0x11).unwrap();

    // Repeated START to same address — device should get a new
    // StartSend but the old transfer should be finished first
    bus.start_transfer(0x50, false).unwrap();
    // Events: StartSend, Finish, StartSend
    assert_eq!(dev.events().len(), 3);
    assert_eq!(dev.events()[1], I2cEvent::Finish);
    assert_eq!(dev.events()[2], I2cEvent::StartSend);

    bus.end_transfer();
}
