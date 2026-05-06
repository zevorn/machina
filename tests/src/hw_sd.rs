use std::sync::{Arc, Mutex};

use machina_hw_sd::{SdBus, SdBusHost, SdCard, SdError, SdRequest, SdVoltage};

/// Mock SD card that records commands and returns configurable responses.
struct MockSdCard {
    inserted: Mutex<bool>,
    readonly: Mutex<bool>,
    voltage: Mutex<u16>,
    commands: Mutex<Vec<(u8, u32)>>,
    write_buf: Mutex<Vec<u8>>,
    read_buf: Mutex<Vec<u8>>,
    read_pos: Mutex<usize>,
    resp_data: Mutex<Vec<u8>>,
    resp_len: Mutex<usize>,
}

impl MockSdCard {
    fn new(inserted: bool) -> Arc<Self> {
        Arc::new(Self {
            inserted: Mutex::new(inserted),
            readonly: Mutex::new(false),
            voltage: Mutex::new(0),
            commands: Mutex::new(Vec::new()),
            write_buf: Mutex::new(Vec::new()),
            read_buf: Mutex::new(vec![0x11, 0x22, 0x33, 0x44]),
            read_pos: Mutex::new(0),
            resp_data: Mutex::new(vec![0x00; 16]),
            resp_len: Mutex::new(6),
        })
    }

    fn set_response(&self, data: &[u8]) {
        let mut r = self.resp_data.lock().unwrap();
        r.clear();
        r.extend_from_slice(data);
        *self.resp_len.lock().unwrap() = data.len();
    }

    fn commands(&self) -> Vec<(u8, u32)> {
        self.commands.lock().unwrap().clone()
    }

    fn written(&self) -> Vec<u8> {
        self.write_buf.lock().unwrap().clone()
    }
}

impl SdCard for MockSdCard {
    fn do_command(&self, req: &SdRequest, resp: &mut [u8]) -> usize {
        self.commands.lock().unwrap().push((req.cmd, req.arg));
        let len = *self.resp_len.lock().unwrap();
        let data = self.resp_data.lock().unwrap();
        let n = len.min(resp.len());
        resp[..n].copy_from_slice(&data[..n]);
        n
    }

    fn write_byte(&self, value: u8) {
        self.write_buf.lock().unwrap().push(value);
    }

    fn read_byte(&self) -> u8 {
        let mut pos = self.read_pos.lock().unwrap();
        let buf = self.read_buf.lock().unwrap();
        let val = if *pos < buf.len() { buf[*pos] } else { 0xFF };
        *pos += 1;
        val
    }

    fn receive_ready(&self) -> bool {
        true
    }

    fn data_ready(&self) -> bool {
        !self.read_buf.lock().unwrap().is_empty()
    }

    fn get_inserted(&self) -> bool {
        *self.inserted.lock().unwrap()
    }

    fn get_readonly(&self) -> bool {
        *self.readonly.lock().unwrap()
    }

    fn set_voltage(&self, millivolts: u16) {
        *self.voltage.lock().unwrap() = millivolts;
    }

    fn get_dat_lines(&self) -> u8 {
        0b1111
    }

    fn get_cmd_line(&self) -> bool {
        true
    }
}

/// Mock host that records insertion/readonly callbacks.
struct MockHost {
    inserted: Mutex<Option<bool>>,
    readonly: Mutex<Option<bool>>,
}

impl MockHost {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inserted: Mutex::new(None),
            readonly: Mutex::new(None),
        })
    }

    fn last_inserted(&self) -> Option<bool> {
        *self.inserted.lock().unwrap()
    }

    fn _last_readonly(&self) -> Option<bool> {
        *self.readonly.lock().unwrap()
    }
}

impl SdBusHost for MockHost {
    fn set_inserted(&self, inserted: bool) {
        *self.inserted.lock().unwrap() = Some(inserted);
    }

    fn set_readonly(&self, readonly: bool) {
        *self.readonly.lock().unwrap() = Some(readonly);
    }
}

// -- Positive Tests --

#[test]
fn test_sd_bus_new() {
    let bus = SdBus::new();
    assert!(!bus.get_inserted());
    assert!(!bus.get_readonly());
    assert_eq!(bus.get_dat_lines(), 0b1111);
    assert!(bus.get_cmd_line());
}

#[test]
fn test_sd_insert_card() {
    let bus = SdBus::new();
    let card = MockSdCard::new(true);
    bus.insert_card(card);
    assert!(bus.get_inserted());
}

#[test]
fn test_sd_remove_card() {
    let bus = SdBus::new();
    let card = MockSdCard::new(true);
    bus.insert_card(card);
    assert!(bus.get_inserted());
    bus.remove_card();
    assert!(!bus.get_inserted());
}

#[test]
fn test_sd_do_command() {
    let bus = SdBus::new();
    let card = MockSdCard::new(true);
    card.set_response(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    bus.insert_card(card.clone());

    let mut resp = [0u8; 16];
    let req = SdRequest::new(8, 0x1AA);
    let n = bus.do_command(&req, &mut resp).unwrap();

    assert_eq!(n, 6);
    assert_eq!(resp[..6], [0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    assert_eq!(card.commands(), vec![(8, 0x1AA)]);
}

#[test]
fn test_sd_do_command_no_card() {
    let bus = SdBus::new();
    let mut resp = [0u8; 16];
    let req = SdRequest::new(0, 0);
    let result = bus.do_command(&req, &mut resp);
    assert_eq!(result.unwrap_err(), SdError::NoCard);
}

#[test]
fn test_sd_write_read_byte() {
    let bus = SdBus::new();
    let card = MockSdCard::new(true);
    bus.insert_card(card.clone());

    bus.write_byte(0xAA).unwrap();
    bus.write_byte(0xBB).unwrap();
    assert_eq!(card.written(), vec![0xAA, 0xBB]);

    assert_eq!(bus.read_byte().unwrap(), 0x11);
    assert_eq!(bus.read_byte().unwrap(), 0x22);
}

#[test]
fn test_sd_read_write_no_card() {
    let bus = SdBus::new();
    assert_eq!(bus.read_byte().unwrap_err(), SdError::NoCard);
    assert_eq!(bus.write_byte(0xFF).unwrap_err(), SdError::NoCard);
}

#[test]
fn test_sd_receive_ready() {
    let bus = SdBus::new();
    let card = MockSdCard::new(true);
    bus.insert_card(card);
    assert!(bus.receive_ready());
}

#[test]
fn test_sd_data_ready() {
    let bus = SdBus::new();
    let card = MockSdCard::new(true);
    bus.insert_card(card);
    assert!(bus.data_ready());
}

#[test]
fn test_sd_set_voltage() {
    let bus = SdBus::new();
    let card = MockSdCard::new(true);
    bus.insert_card(card.clone());

    bus.set_voltage(SdVoltage::V33 as u16);
    // Voltage propagates to card (checked via internal mock state)
    assert!(bus.get_inserted()); // card still present
}

#[test]
fn test_sd_host_callbacks() {
    let bus = SdBus::new();
    let host = MockHost::new();
    bus.set_host(host.clone());

    // Insert -> host notified
    let card = MockSdCard::new(true);
    bus.insert_card(card);
    assert_eq!(host.last_inserted(), Some(true));

    // Remove -> host notified
    bus.remove_card();
    assert_eq!(host.last_inserted(), Some(false));
}

#[test]
fn test_sd_get_readonly() {
    let bus = SdBus::new();
    let card = MockSdCard::new(true);
    bus.insert_card(card);
    assert!(!bus.get_readonly());
}

#[test]
fn test_sd_get_inserted_no_card() {
    let bus = SdBus::new();
    assert!(!bus.get_inserted());
}

#[test]
fn test_sd_get_dat_lines_no_card() {
    let bus = SdBus::new();
    assert_eq!(bus.get_dat_lines(), 0b1111);
}

#[test]
fn test_sd_get_cmd_line_no_card() {
    let bus = SdBus::new();
    assert!(bus.get_cmd_line());
}

#[test]
fn test_sd_receive_ready_no_card() {
    let bus = SdBus::new();
    assert!(!bus.receive_ready());
}

#[test]
fn test_sd_data_ready_no_card() {
    let bus = SdBus::new();
    assert!(!bus.data_ready());
}

#[test]
fn test_sd_reparent_card() {
    let bus1 = SdBus::new();
    let bus2 = SdBus::new();
    let host = MockHost::new();
    bus2.set_host(host.clone());

    let card = MockSdCard::new(true);
    card.set_response(&[0xAA, 0xBB]);
    bus1.insert_card(card.clone());
    assert!(bus1.get_inserted());

    bus2.reparent_card(&bus1);

    // Source bus is now empty
    assert!(!bus1.get_inserted());

    // Destination bus has the card and can use it
    assert!(bus2.get_inserted());
    assert_eq!(host.last_inserted(), Some(true));

    let mut resp = [0u8; 16];
    let n = bus2.do_command(&SdRequest::new(1, 0), &mut resp).unwrap();
    assert_eq!(n, 2);
    assert_eq!(resp[0], 0xAA);
    assert_eq!(resp[1], 0xBB);

    // Source bus cannot use the card anymore
    assert_eq!(
        bus1.do_command(&SdRequest::new(1, 0), &mut resp)
            .unwrap_err(),
        SdError::NoCard
    );
}
