//! SPI-to-SD bridge.
//!
//! This module exposes an SD bus through the [`machina_hw_ssi::SpiSlave`]
//! trait.  The initial implementation covers command framing and response
//! delivery used by SPI-mode SD probes.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::mdev::{MDeviceError, MDeviceState};
use machina_hw_ssi::{SpiCsPolarity, SpiSlave};

use crate::{SdBus, SdRequest};

const PULL_UP: u32 = 0xff;
const COMMAND_START_MASK: u8 = 0xc0;
const COMMAND_START: u8 = 0x40;
const COMMAND_LEN: usize = 6;
const DATA_START_TOKEN: u8 = 0xfe;
const DATA_RESPONSE_ACCEPTED: u32 = 0x05;
const DEFAULT_BLOCK_LEN: usize = 512;
const CMD_READ_SINGLE_BLOCK: u8 = 17;
const CMD_WRITE_SINGLE_BLOCK: u8 = 24;

#[derive(Debug, Default)]
enum WriteState {
    #[default]
    Idle,
    WaitingToken,
    Receiving {
        data: Vec<u8>,
    },
    ReceivingCrc {
        data: Vec<u8>,
        crc_seen: usize,
    },
}

enum WriteStep {
    Pending,
    Complete(Vec<u8>),
}

#[derive(Debug, Default)]
struct SsiSdRegs {
    selected: bool,
    command: Vec<u8>,
    response: VecDeque<u8>,
    response_delay: usize,
    write_state: WriteState,
}

pub struct SsiSd {
    state: Mutex<MDeviceState>,
    regs: Mutex<SsiSdRegs>,
    sd_bus: Mutex<Option<Arc<SdBus>>>,
    cs_index: u8,
}

impl SsiSd {
    #[must_use]
    pub fn new(cs_index: u8) -> Self {
        Self::new_named("ssi-sd", cs_index)
    }

    #[must_use]
    pub fn new_named(local_id: &str, cs_index: u8) -> Self {
        Self {
            state: Mutex::new(MDeviceState::new(local_id)),
            regs: Mutex::new(SsiSdRegs::default()),
            sd_bus: Mutex::new(None),
            cs_index,
        }
    }

    pub fn realize(self: &Arc<Self>) -> Result<(), MDeviceError> {
        self.state.lock().unwrap().mark_realized()
    }

    pub fn unrealize(self: &Arc<Self>) -> Result<(), MDeviceError> {
        self.state.lock().unwrap().mark_unrealized()
    }

    pub fn realized(&self) -> bool {
        self.state.lock().unwrap().is_realized()
    }

    pub fn with_mdevice<T>(&self, f: impl FnOnce(&MDeviceState) -> T) -> T {
        let guard = self.state.lock().unwrap();
        f(&guard)
    }

    pub fn object_info(&self) -> MObjectInfo {
        self.state.lock().unwrap().object_info()
    }

    pub fn connect_sd_bus(&self, bus: Arc<SdBus>) {
        *self.sd_bus.lock().unwrap() = Some(bus);
    }

    pub fn reset_runtime(&self) {
        *self.regs.lock().unwrap() = SsiSdRegs::default();
    }

    fn dispatch_packet(&self, packet: &[u8]) -> Vec<u8> {
        let Some(bus) = self.sd_bus.lock().unwrap().clone() else {
            return vec![PULL_UP as u8];
        };
        let cmd = packet[0] & 0x3f;
        let arg =
            u32::from_be_bytes([packet[1], packet[2], packet[3], packet[4]]);
        let mut response = [0; 16];
        match bus.do_command(&SdRequest::new(cmd, arg), &mut response) {
            Ok(0) => vec![0],
            Ok(n) => {
                let mut out = response[..n].to_vec();
                if cmd == CMD_READ_SINGLE_BLOCK && bus.data_ready() {
                    out.push(DATA_START_TOKEN);
                    let mut block = vec![PULL_UP as u8; DEFAULT_BLOCK_LEN];
                    if bus.read_data(&mut block).is_ok() {
                        out.extend_from_slice(&block);
                        out.extend_from_slice(&[PULL_UP as u8; 2]);
                    }
                } else if cmd == CMD_WRITE_SINGLE_BLOCK && bus.receive_ready() {
                    self.regs.lock().unwrap().write_state =
                        WriteState::WaitingToken;
                }
                out
            }
            Err(_) => vec![PULL_UP as u8],
        }
    }

    fn consume_write_byte(regs: &mut SsiSdRegs, byte: u8) -> Option<WriteStep> {
        let state = std::mem::take(&mut regs.write_state);
        match state {
            WriteState::Idle => {
                regs.write_state = WriteState::Idle;
                None
            }
            WriteState::WaitingToken => {
                regs.write_state = if byte == DATA_START_TOKEN {
                    WriteState::Receiving {
                        data: Vec::with_capacity(DEFAULT_BLOCK_LEN),
                    }
                } else {
                    WriteState::WaitingToken
                };
                Some(WriteStep::Pending)
            }
            WriteState::Receiving { mut data } => {
                data.push(byte);
                regs.write_state = if data.len() == DEFAULT_BLOCK_LEN {
                    WriteState::ReceivingCrc { data, crc_seen: 0 }
                } else {
                    WriteState::Receiving { data }
                };
                Some(WriteStep::Pending)
            }
            WriteState::ReceivingCrc { data, crc_seen } => {
                let crc_seen = crc_seen + 1;
                if crc_seen == 2 {
                    regs.write_state = WriteState::Idle;
                    Some(WriteStep::Complete(data))
                } else {
                    regs.write_state =
                        WriteState::ReceivingCrc { data, crc_seen };
                    Some(WriteStep::Pending)
                }
            }
        }
    }

    fn complete_write_data(&self, data: &[u8]) -> u32 {
        let Some(bus) = self.sd_bus.lock().unwrap().clone() else {
            return PULL_UP;
        };
        if bus.write_data(data).is_ok() {
            DATA_RESPONSE_ACCEPTED
        } else {
            PULL_UP
        }
    }
}

impl Default for SsiSd {
    fn default() -> Self {
        Self::new(0)
    }
}

impl SpiSlave for SsiSd {
    fn transfer(&self, val: u32) -> u32 {
        let byte = val as u8;
        let mut packet = None;
        let mut completed_write = None;

        {
            let mut regs = self.regs.lock().unwrap();
            if !regs.selected {
                return PULL_UP;
            }
            if !regs.response.is_empty() {
                if regs.response_delay > 0 {
                    regs.response_delay -= 1;
                    return PULL_UP;
                }
                if let Some(response) = regs.response.pop_front() {
                    return u32::from(response);
                }
            }
            if let Some(step) = Self::consume_write_byte(&mut regs, byte) {
                match step {
                    WriteStep::Pending => return PULL_UP,
                    WriteStep::Complete(data) => {
                        completed_write = Some(data);
                    }
                }
            } else {
                if regs.command.is_empty()
                    && byte & COMMAND_START_MASK != COMMAND_START
                {
                    return PULL_UP;
                }

                regs.command.push(byte);
                if regs.command.len() == COMMAND_LEN {
                    packet = Some(regs.command.clone());
                    regs.command.clear();
                }
            }
        }

        if let Some(data) = completed_write {
            return self.complete_write_data(&data);
        }

        if let Some(packet) = packet {
            let response = self.dispatch_packet(&packet);
            let mut regs = self.regs.lock().unwrap();
            regs.response = VecDeque::from(response);
            regs.response_delay = usize::from(!regs.response.is_empty());
        }

        PULL_UP
    }

    fn set_cs(&self, cs: bool) {
        let selected = !cs;
        let mut regs = self.regs.lock().unwrap();
        if regs.selected != selected {
            regs.command.clear();
            regs.response.clear();
            regs.response_delay = 0;
            regs.write_state = WriteState::Idle;
        }
        regs.selected = selected;
    }

    fn cs_polarity(&self) -> SpiCsPolarity {
        SpiCsPolarity::Low
    }

    fn cs_index(&self) -> u8 {
        self.cs_index
    }
}
