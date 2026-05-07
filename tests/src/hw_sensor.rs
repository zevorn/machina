use std::sync::Arc;

use machina_hw_i2c::{I2cBus, I2cSlave};
use machina_hw_sensor::{Tmp105, Tmp421};

fn attach_tmp105(address: u8) -> (I2cBus, Arc<Tmp105>) {
    let bus = I2cBus::new();
    let dev = Tmp105::new(address);
    bus.attach(Arc::clone(&dev) as Arc<dyn I2cSlave>).unwrap();
    (bus, dev)
}

fn attach_tmp421(address: u8) -> (I2cBus, Arc<Tmp421>) {
    let bus = I2cBus::new();
    let dev = Tmp421::new(address);
    bus.attach(Arc::clone(&dev) as Arc<dyn I2cSlave>).unwrap();
    (bus, dev)
}

fn attach_tmp421_device(dev: Arc<Tmp421>) -> (I2cBus, Arc<Tmp421>) {
    let bus = I2cBus::new();
    bus.attach(Arc::clone(&dev) as Arc<dyn I2cSlave>).unwrap();
    (bus, dev)
}

fn read_bytes(bus: &I2cBus, address: u8, pointer: u8, len: usize) -> Vec<u8> {
    bus.start_transfer(address, false).unwrap();
    bus.send(pointer).unwrap();
    bus.start_transfer(address, true).unwrap();
    let mut bytes = Vec::with_capacity(len);
    for _ in 0..len {
        bytes.push(bus.recv());
    }
    bus.end_transfer();
    bytes
}

fn write_bytes(bus: &I2cBus, address: u8, pointer: u8, bytes: &[u8]) {
    bus.start_transfer(address, false).unwrap();
    bus.send(pointer).unwrap();
    for &byte in bytes {
        bus.send(byte).unwrap();
    }
    bus.end_transfer();
}

#[test]
fn test_tmp105_lifecycle_and_mom_identity() {
    let dev = Tmp105::new(0x48);
    assert!(!dev.realized());
    dev.with_mdevice(|device| assert_eq!(device.local_id(), "tmp105"));
    assert_eq!(dev.object_info().local_id, "tmp105");

    dev.realize().unwrap();
    assert!(dev.realized());
    let err = dev.realize().unwrap_err();
    assert!(
        err.to_string().contains("already realized"),
        "unexpected second-realize error: {err}"
    );

    dev.unrealize().unwrap();
    assert!(!dev.realized());
    let err = dev.unrealize().unwrap_err();
    assert!(
        err.to_string().contains("not realized"),
        "unexpected second-unrealize error: {err}"
    );
}

#[test]
fn test_tmp105_defaults_and_temperature_read() {
    let (bus, dev) = attach_tmp105(0x48);

    assert_eq!(dev.address(), 0x48);
    assert_eq!(read_bytes(&bus, 0x48, 0x00, 3), vec![0x00, 0x00, 0xff]);
    assert_eq!(read_bytes(&bus, 0x48, 0x01, 2), vec![0x00, 0x00]);
    assert_eq!(read_bytes(&bus, 0x48, 0x02, 2), vec![0x4b, 0x00]);
    assert_eq!(read_bytes(&bus, 0x48, 0x03, 2), vec![0x50, 0x00]);

    dev.set_temperature_millicelsius(25_000).unwrap();
    assert_eq!(read_bytes(&bus, 0x48, 0x00, 2), vec![0x19, 0x00]);
}

#[test]
fn test_tmp105_config_masks_one_shot_and_limit_write() {
    let (bus, _dev) = attach_tmp105(0x49);

    write_bytes(&bus, 0x49, 0x01, &[0xff]);
    assert_eq!(read_bytes(&bus, 0x49, 0x01, 2), vec![0x7f, 0x00]);

    write_bytes(&bus, 0x49, 0x02, &[0x12, 0x30]);
    write_bytes(&bus, 0x49, 0x03, &[0x45, 0x60]);
    assert_eq!(read_bytes(&bus, 0x49, 0x02, 2), vec![0x12, 0x30]);
    assert_eq!(read_bytes(&bus, 0x49, 0x03, 2), vec![0x45, 0x60]);
}

#[test]
fn test_tmp105_reset_restores_defaults() {
    let (bus, dev) = attach_tmp105(0x4a);

    write_bytes(&bus, 0x4a, 0x01, &[0x7f]);
    write_bytes(&bus, 0x4a, 0x02, &[0x12, 0x30]);
    dev.set_temperature_millicelsius(10_000).unwrap();

    dev.reset_runtime();

    assert_eq!(read_bytes(&bus, 0x4a, 0x00, 2), vec![0x00, 0x00]);
    assert_eq!(read_bytes(&bus, 0x4a, 0x01, 1), vec![0x00]);
    assert_eq!(read_bytes(&bus, 0x4a, 0x02, 2), vec![0x4b, 0x00]);
}

#[test]
fn test_tmp421_defaults_ids_and_temperature_read() {
    let (bus, dev) = attach_tmp421(0x4c);

    assert_eq!(dev.address(), 0x4c);
    assert_eq!(read_bytes(&bus, 0x4c, 0xfe, 2), vec![0x55, 0x00]);
    assert_eq!(read_bytes(&bus, 0x4c, 0xff, 2), vec![0x21, 0x00]);
    assert_eq!(read_bytes(&bus, 0x4c, 0x09, 1), vec![0x00]);
    assert_eq!(read_bytes(&bus, 0x4c, 0x0a, 1), vec![0x1c]);
    assert_eq!(read_bytes(&bus, 0x4c, 0x0b, 1), vec![0x07]);
    assert_eq!(read_bytes(&bus, 0x4c, 0x08, 1), vec![0x00]);
    assert_eq!(read_bytes(&bus, 0x4c, 0x00, 2), vec![0x00, 0x00]);
    assert_eq!(read_bytes(&bus, 0x4c, 0x10, 2), vec![0x00, 0x00]);
}

#[test]
fn test_tmp421_lifecycle_and_mom_identity() {
    let dev = Tmp421::new(0x4c);
    assert!(!dev.realized());
    dev.with_mdevice(|device| assert_eq!(device.local_id(), "tmp421"));
    assert_eq!(dev.object_info().local_id, "tmp421");

    dev.realize().unwrap();
    assert!(dev.realized());
    let err = dev.realize().unwrap_err();
    assert!(
        err.to_string().contains("already realized"),
        "unexpected second-realize error: {err}"
    );

    dev.unrealize().unwrap();
    assert!(!dev.realized());
    let err = dev.unrealize().unwrap_err();
    assert!(
        err.to_string().contains("not realized"),
        "unexpected second-unrealize error: {err}"
    );
}

#[test]
fn test_tmp421_model_variants_report_device_id_and_default_config2() {
    let (bus422, _tmp422) = attach_tmp421_device(Tmp421::new_tmp422(0x4d));
    assert_eq!(read_bytes(&bus422, 0x4d, 0xff, 1), vec![0x22]);
    assert_eq!(read_bytes(&bus422, 0x4d, 0x0a, 1), vec![0x3c]);

    let (bus423, _tmp423) = attach_tmp421_device(Tmp421::new_tmp423(0x4e));
    assert_eq!(read_bytes(&bus423, 0x4e, 0xff, 1), vec![0x23]);
    assert_eq!(read_bytes(&bus423, 0x4e, 0x0a, 1), vec![0x7c]);

    write_bytes(&bus423, 0x4e, 0x0a, &[0x00]);
    write_bytes(&bus423, 0x4e, 0xfc, &[0x00]);
    assert_eq!(read_bytes(&bus423, 0x4e, 0x0a, 1), vec![0x7c]);
}

#[test]
fn test_tmp421_writable_config_rate_and_reset_register() {
    let (bus, _dev) = attach_tmp421(0x4d);

    write_bytes(&bus, 0x4d, 0x09, &[0x44]);
    write_bytes(&bus, 0x4d, 0x0a, &[0x55]);
    write_bytes(&bus, 0x4d, 0x0b, &[0x06]);
    assert_eq!(read_bytes(&bus, 0x4d, 0x09, 1), vec![0x44]);
    assert_eq!(read_bytes(&bus, 0x4d, 0x0a, 1), vec![0x55]);
    assert_eq!(read_bytes(&bus, 0x4d, 0x0b, 1), vec![0x06]);

    write_bytes(&bus, 0x4d, 0xfc, &[0x00]);
    assert_eq!(read_bytes(&bus, 0x4d, 0x09, 1), vec![0x00]);
    assert_eq!(read_bytes(&bus, 0x4d, 0x0a, 1), vec![0x1c]);
    assert_eq!(read_bytes(&bus, 0x4d, 0x0b, 1), vec![0x07]);
}
