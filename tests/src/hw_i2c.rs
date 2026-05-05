use std::sync::{Arc, Mutex};

use machina_hw_i2c::{I2cBus, I2cError, I2cEvent, I2cSlave, I2C_BROADCAST};

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

    fn set_nack_on(&self, byte: u8) {
        *self.nack_on.lock().unwrap() = Some(byte);
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
