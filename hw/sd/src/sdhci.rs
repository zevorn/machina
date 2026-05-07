//! SD Host Controller Interface MMIO model.
//!
//! This module provides the host-controller side used by board code to
//! connect an [`crate::SdBus`] to memory-mapped controller registers.

use std::sync::{Arc, Mutex};

use machina_core::device_cell::DeviceRegs;
use machina_hw_core::bus::SysBusDeviceState;
use machina_memory::region::MmioOps;

use crate::{SdBus, SdBusHost, SdError, SdRequest};

const REG_BLOCK_SIZE: u64 = 0x04;
const REG_BLOCK_COUNT: u64 = 0x06;
const REG_ARGUMENT: u64 = 0x08;
const REG_COMMAND: u64 = 0x0e;
const REG_RESPONSE0: u64 = 0x10;
const REG_RESPONSE1: u64 = 0x14;
const REG_RESPONSE2: u64 = 0x18;
const REG_RESPONSE3: u64 = 0x1c;
const REG_BUFFER_DATA_PORT: u64 = 0x20;
const REG_PRESENT_STATE: u64 = 0x24;
const REG_SOFTWARE_RESET: u64 = 0x2f;
const REG_NORMAL_INT_STATUS: u64 = 0x30;
const REG_ERROR_INT_STATUS: u64 = 0x32;
const REG_NORMAL_INT_ENABLE: u64 = 0x34;
const REG_NORMAL_INT_SIGNAL_ENABLE: u64 = 0x38;
const REG_HOST_VERSION: u64 = 0xfe;

const PRESENT_BUFFER_WRITE_ENABLE: u32 = 1 << 10;
const PRESENT_BUFFER_READ_ENABLE: u32 = 1 << 11;
const PRESENT_CARD_INSERTED: u32 = 1 << 16;
const PRESENT_CARD_STATE_STABLE: u32 = 1 << 17;
const PRESENT_CARD_DETECT_PIN_LEVEL: u32 = 1 << 18;
const PRESENT_WRITE_PROTECT_PIN_LEVEL: u32 = 1 << 19;

const INT_COMMAND_COMPLETE: u16 = 1 << 0;
const INT_TRANSFER_COMPLETE: u16 = 1 << 1;
const INT_BUFFER_WRITE_READY: u16 = 1 << 4;
const INT_BUFFER_READ_READY: u16 = 1 << 5;
const INT_CARD_INSERTION: u16 = 1 << 6;
const INT_CARD_REMOVAL: u16 = 1 << 7;
const INT_ERROR: u16 = 1 << 15;

const ERR_COMMAND_TIMEOUT: u16 = 1 << 0;

const SOFTWARE_RESET_ALL: u8 = 1 << 0;

const HOST_VERSION_SPEC_3_00: u16 = 0x0002;
const DEFAULT_BLOCK_SIZE: usize = 512;
const CMD_READ_SINGLE_BLOCK: u8 = 17;
const CMD_READ_MULTIPLE_BLOCK: u8 = 18;
const CMD_WRITE_BLOCK: u8 = 24;
const CMD_WRITE_MULTIPLE_BLOCK: u8 = 25;

#[derive(Debug, PartialEq, Eq)]
struct SdhciRegs {
    inserted: bool,
    readonly: bool,
    block_size: u16,
    block_count: u16,
    argument: u32,
    command: u16,
    response: [u32; 4],
    normal_int_status: u16,
    error_int_status: u16,
    normal_int_enable: u16,
    normal_int_signal_enable: u16,
    software_reset: u8,
    data_buffer: Vec<u8>,
    data_offset: usize,
    write_transfer_active: bool,
}

impl SdhciRegs {
    fn new() -> Self {
        Self {
            inserted: false,
            readonly: false,
            block_size: 0,
            block_count: 0,
            argument: 0,
            command: 0,
            response: [0; 4],
            normal_int_status: 0,
            error_int_status: 0,
            normal_int_enable: 0,
            normal_int_signal_enable: 0,
            software_reset: 0,
            data_buffer: Vec::new(),
            data_offset: 0,
            write_transfer_active: false,
        }
    }

    fn reset_runtime(&mut self) {
        let inserted = self.inserted;
        let readonly = self.readonly;
        *self = Self::new();
        self.inserted = inserted;
        self.readonly = readonly;
    }

    fn present_state(&self) -> u32 {
        let mut value = PRESENT_CARD_STATE_STABLE;
        if !self.data_buffer.is_empty() {
            if self.write_transfer_active {
                value |= PRESENT_BUFFER_WRITE_ENABLE;
            } else {
                value |= PRESENT_BUFFER_READ_ENABLE;
            }
        }
        if self.inserted {
            value |= PRESENT_CARD_INSERTED | PRESENT_CARD_DETECT_PIN_LEVEL;
        }
        if self.inserted && !self.readonly {
            value |= PRESENT_WRITE_PROTECT_PIN_LEVEL;
        }
        value
    }

    fn transfer_block_len(&self) -> usize {
        let len = usize::from(self.block_size & 0x0fff);
        if len == 0 {
            DEFAULT_BLOCK_SIZE
        } else {
            len
        }
    }

    fn transfer_block_count(&self) -> usize {
        let count = usize::from(self.block_count);
        if count == 0 {
            1
        } else {
            count
        }
    }
}

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "std")]
pub struct Sdhci {
    state: Mutex<SysBusDeviceState>,
    regs: DeviceRegs<SdhciRegs>,
    bus: Mutex<Option<Arc<SdBus>>>,
}

impl Sdhci {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("sdhci")
    }

    #[must_use]
    pub fn new_named(local_id: &str) -> Self {
        Self {
            state: Mutex::new(SysBusDeviceState::new(local_id)),
            regs: DeviceRegs::new(SdhciRegs::new()),
            bus: Mutex::new(None),
        }
    }

    pub fn connect_bus(&self, bus: Arc<SdBus>) {
        *self.bus.lock().unwrap() = Some(bus);
    }

    pub fn reset_runtime(&self) {
        self.regs.lock().reset_runtime();
    }

    #[must_use]
    pub fn present_state(&self) -> u32 {
        self.regs.lock().present_state()
    }

    fn read_reg(&self, offset: u64, size: u32) -> u64 {
        if !valid_mmio_access(offset, size) {
            return 0;
        }

        if offset == REG_BUFFER_DATA_PORT {
            return self.read_data_port(size);
        }

        let regs = self.regs.lock();
        let value = match offset {
            REG_BLOCK_SIZE => u64::from(regs.block_size),
            REG_BLOCK_COUNT => u64::from(regs.block_count),
            REG_ARGUMENT => u64::from(regs.argument),
            REG_COMMAND => u64::from(regs.command),
            REG_RESPONSE0 => u64::from(regs.response[0]),
            REG_RESPONSE1 => u64::from(regs.response[1]),
            REG_RESPONSE2 => u64::from(regs.response[2]),
            REG_RESPONSE3 => u64::from(regs.response[3]),
            REG_PRESENT_STATE => u64::from(regs.present_state()),
            REG_SOFTWARE_RESET => u64::from(regs.software_reset),
            REG_NORMAL_INT_STATUS => u64::from(regs.normal_int_status),
            REG_ERROR_INT_STATUS => u64::from(regs.error_int_status),
            REG_NORMAL_INT_ENABLE => u64::from(regs.normal_int_enable),
            REG_NORMAL_INT_SIGNAL_ENABLE => {
                u64::from(regs.normal_int_signal_enable)
            }
            REG_HOST_VERSION => u64::from(HOST_VERSION_SPEC_3_00),
            _ => 0,
        };
        mask_for_size(size) & value
    }

    fn write_reg(&self, offset: u64, size: u32, value: u64) {
        if !valid_mmio_access(offset, size) {
            return;
        }

        let value = value & mask_for_size(size);
        if offset == REG_BUFFER_DATA_PORT {
            self.write_data_port(size, value);
            return;
        }

        let mut regs = self.regs.lock();
        match offset {
            REG_BLOCK_SIZE => {
                regs.block_size = value as u16;
            }
            REG_BLOCK_COUNT => {
                regs.block_count = value as u16;
            }
            REG_ARGUMENT => {
                regs.argument = value as u32;
            }
            REG_COMMAND => {
                regs.command = value as u16;
                let command = regs.command;
                let argument = regs.argument;
                drop(regs);
                self.dispatch_command(command, argument);
            }
            REG_SOFTWARE_RESET => {
                regs.software_reset = value as u8;
                if regs.software_reset & SOFTWARE_RESET_ALL != 0 {
                    regs.reset_runtime();
                    regs.software_reset = 0;
                }
            }
            REG_NORMAL_INT_STATUS => {
                regs.normal_int_status &= !(value as u16);
                if regs.error_int_status != 0 {
                    regs.normal_int_status |= INT_ERROR;
                }
            }
            REG_ERROR_INT_STATUS => {
                regs.error_int_status &= !(value as u16);
                if regs.error_int_status == 0 {
                    regs.normal_int_status &= !INT_ERROR;
                }
            }
            REG_NORMAL_INT_ENABLE => {
                regs.normal_int_enable = value as u16;
            }
            REG_NORMAL_INT_SIGNAL_ENABLE => {
                regs.normal_int_signal_enable = value as u16;
            }
            _ => {}
        }
    }

    fn dispatch_command(&self, command: u16, argument: u32) {
        let cmd = ((command >> 8) & 0x3f) as u8;
        let Some(bus) = self.bus.lock().unwrap().clone() else {
            self.record_command_error(SdError::NoCard);
            return;
        };
        let mut response = [0; 16];
        match bus.do_command(&SdRequest::new(cmd, argument), &mut response) {
            Ok(_) => {
                let (block_len, block_count) = {
                    let regs = self.regs.lock();
                    (regs.transfer_block_len(), regs.transfer_block_count())
                };
                let mut read_buffer = None;
                let mut write_buffer_len = None;

                if is_read_data_command(cmd) && bus.data_ready() {
                    let transfer_len =
                        block_len * transfer_blocks(cmd, block_count);
                    let mut data = vec![0; transfer_len];
                    if bus.read_data(&mut data).is_ok() {
                        read_buffer = Some(data);
                    }
                } else if is_write_data_command(cmd) && bus.receive_ready() {
                    write_buffer_len =
                        Some(block_len * transfer_blocks(cmd, block_count));
                }

                let mut regs = self.regs.lock();
                regs.response = response_words(&response);
                regs.normal_int_status |= INT_COMMAND_COMPLETE;
                if let Some(data) = read_buffer {
                    regs.data_buffer = data;
                    regs.data_offset = 0;
                    regs.write_transfer_active = false;
                    regs.normal_int_status |= INT_BUFFER_READ_READY;
                    regs.normal_int_status &= !INT_TRANSFER_COMPLETE;
                } else if let Some(len) = write_buffer_len {
                    regs.data_buffer = vec![0; len];
                    regs.data_offset = 0;
                    regs.write_transfer_active = true;
                    regs.normal_int_status |= INT_BUFFER_WRITE_READY;
                    regs.normal_int_status &= !INT_TRANSFER_COMPLETE;
                }
            }
            Err(err) => {
                self.record_command_error(err);
            }
        }
    }

    fn record_command_error(&self, _err: SdError) {
        let mut regs = self.regs.lock();
        regs.error_int_status |= ERR_COMMAND_TIMEOUT;
        regs.normal_int_status |= INT_ERROR;
    }

    fn read_data_port(&self, size: u32) -> u64 {
        let len = (size as usize).min(8);
        let mut bytes = [0; 8];
        let mut regs = self.regs.lock();
        if regs.write_transfer_active || regs.data_buffer.is_empty() {
            return 0;
        }

        for byte in bytes.iter_mut().take(len) {
            if regs.data_offset >= regs.data_buffer.len() {
                break;
            }
            *byte = regs.data_buffer[regs.data_offset];
            regs.data_offset += 1;
        }
        if regs.data_offset >= regs.data_buffer.len() {
            regs.data_buffer.clear();
            regs.data_offset = 0;
            regs.normal_int_status &= !INT_BUFFER_READ_READY;
            regs.normal_int_status |= INT_TRANSFER_COMPLETE;
        }
        u64::from_le_bytes(bytes) & mask_for_size(size)
    }

    fn write_data_port(&self, size: u32, value: u64) {
        let Some(bus) = self.bus.lock().unwrap().clone() else {
            return;
        };
        let len = (size as usize).min(8);
        let bytes = value.to_le_bytes();
        let mut chunk = Vec::new();
        let mut completed = false;

        {
            let mut regs = self.regs.lock();
            if !regs.write_transfer_active || regs.data_buffer.is_empty() {
                return;
            }
            let remaining = regs.data_buffer.len() - regs.data_offset;
            let n = len.min(remaining);
            chunk.extend_from_slice(&bytes[..n]);
            regs.data_offset += n;
            if regs.data_offset >= regs.data_buffer.len() {
                regs.data_buffer.clear();
                regs.data_offset = 0;
                regs.write_transfer_active = false;
                regs.normal_int_status &= !INT_BUFFER_WRITE_READY;
                regs.normal_int_status |= INT_TRANSFER_COMPLETE;
                completed = true;
            }
        }

        if bus.write_data(&chunk).is_err() && completed {
            let mut regs = self.regs.lock();
            regs.normal_int_status &= !INT_TRANSFER_COMPLETE;
        }
    }
}

impl Default for Sdhci {
    fn default() -> Self {
        Self::new()
    }
}

impl SdBusHost for Sdhci {
    fn set_inserted(&self, inserted: bool) {
        let mut regs = self.regs.lock();
        if regs.inserted == inserted {
            return;
        }
        regs.inserted = inserted;
        if inserted {
            regs.normal_int_status |= INT_CARD_INSERTION;
        } else {
            regs.normal_int_status |= INT_CARD_REMOVAL;
        }
    }

    fn set_readonly(&self, readonly: bool) {
        self.regs.lock().readonly = readonly;
    }
}

pub struct SdhciMmio(pub Arc<Sdhci>);

impl MmioOps for SdhciMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.read_reg(offset, size)
    }

    fn write(&self, offset: u64, size: u32, value: u64) {
        self.0.write_reg(offset, size, value);
    }
}

fn mask_for_size(size: u32) -> u64 {
    match size {
        1 => 0xff,
        2 => 0xffff,
        4 => 0xffff_ffff,
        8 => u64::MAX,
        _ => 0,
    }
}

fn valid_mmio_access(offset: u64, size: u32) -> bool {
    matches!(size, 1 | 2 | 4) && offset.is_multiple_of(u64::from(size))
}

fn is_read_data_command(cmd: u8) -> bool {
    matches!(cmd, CMD_READ_SINGLE_BLOCK | CMD_READ_MULTIPLE_BLOCK)
}

fn is_write_data_command(cmd: u8) -> bool {
    matches!(cmd, CMD_WRITE_BLOCK | CMD_WRITE_MULTIPLE_BLOCK)
}

fn transfer_blocks(cmd: u8, block_count: usize) -> usize {
    if matches!(cmd, CMD_READ_MULTIPLE_BLOCK | CMD_WRITE_MULTIPLE_BLOCK) {
        block_count
    } else {
        1
    }
}

fn response_words(response: &[u8; 16]) -> [u32; 4] {
    [
        u32::from_be_bytes(response[0..4].try_into().unwrap()),
        u32::from_be_bytes(response[4..8].try_into().unwrap()),
        u32::from_be_bytes(response[8..12].try_into().unwrap()),
        u32::from_be_bytes(response[12..16].try_into().unwrap()),
    ]
}
