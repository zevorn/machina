//! SD Host Controller Interface MMIO model.
//!
//! This module provides the host-controller side used by board code to
//! connect an [`crate::SdBus`] to memory-mapped controller registers.

use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRegs;
use machina_hw_core::bus::SysBusDeviceState;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MmioOps;

use crate::{SdBus, SdBusHost, SdError, SdRequest};

const REG_DMA_ADDRESS: u64 = 0x00;
const REG_BLOCK_SIZE: u64 = 0x04;
const REG_BLOCK_COUNT: u64 = 0x06;
const REG_ARGUMENT: u64 = 0x08;
const REG_TRANSFER_MODE: u64 = 0x0c;
const REG_COMMAND: u64 = 0x0e;
const REG_RESPONSE0: u64 = 0x10;
const REG_RESPONSE_END: u64 = REG_RESPONSE0 + 0x10;
const REG_BUFFER_DATA_PORT: u64 = 0x20;
const REG_PRESENT_STATE: u64 = 0x24;
const REG_HOST_CONTROL: u64 = 0x28;
const REG_POWER_CONTROL: u64 = 0x29;
const REG_BLOCK_GAP_CONTROL: u64 = 0x2a;
const REG_CLOCK_CONTROL: u64 = 0x2c;
const REG_TIMEOUT_CONTROL: u64 = 0x2e;
const REG_SOFTWARE_RESET: u64 = 0x2f;
const REG_NORMAL_INT_STATUS: u64 = 0x30;
const REG_ERROR_INT_STATUS: u64 = 0x32;
const REG_NORMAL_INT_ENABLE: u64 = 0x34;
const REG_NORMAL_INT_SIGNAL_ENABLE: u64 = 0x38;
const REG_HOST_CONTROL2: u64 = 0x3e;
const REG_CAPABILITIES: u64 = 0x40;
const REG_MAX_CURRENT: u64 = 0x48;
const REG_HOST_VERSION: u64 = 0xfe;
const SNPS_VENDOR_REG_START: u64 = 0x100;
const SNPS_VENDOR_REG_END: u64 = 0x1000;
const SNPS_VENDOR_REG_WORDS: usize = (SNPS_VENDOR_REG_END / 4) as usize;
const DWC_MSHC_PHY_CNFG: u64 = 0x300;
const DWC_MSHC_PHY_PWRGOOD: u32 = 1 << 1;

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

const TRANSFER_MODE_DMA_ENABLE: u16 = 1 << 0;
const CAPABILITIES_BASE_CLOCK_SHIFT: u32 = 8;
const CAPABILITIES_BASE_CLOCK_MHZ: u32 = 100;
const CAPABILITIES_CAN_DO_HISPD: u32 = 1 << 21;
const CAPABILITIES_CAN_DO_SDMA: u32 = 1 << 22;
const CAPABILITIES_CAN_VDD_330: u32 = 1 << 24;
const CLOCK_INTERNAL_ENABLE: u16 = 1 << 0;
const CLOCK_INTERNAL_STABLE: u16 = 1 << 1;
const HOST_VERSION_SPEC_3_00: u16 = 0x0002;
const DEFAULT_BLOCK_SIZE: usize = 512;
const CMD_ALL_SEND_CID: u8 = 2;
const CMD_SWITCH_FUNC: u8 = 6;
const CMD_SEND_CSD: u8 = 9;
const CMD_SEND_CID: u8 = 10;
const CMD_STOP_TRANSMISSION: u8 = 12;
const CMD_SEND_STATUS: u8 = 13;
const CMD_APP_CMD: u8 = 55;
const CMD_READ_SINGLE_BLOCK: u8 = 17;
const CMD_READ_MULTIPLE_BLOCK: u8 = 18;
const CMD_WRITE_BLOCK: u8 = 24;
const CMD_WRITE_MULTIPLE_BLOCK: u8 = 25;
const ACMD_SEND_SCR: u8 = 51;

#[derive(Debug, PartialEq, Eq)]
struct SdhciRegs {
    inserted: bool,
    readonly: bool,
    dma_address: u32,
    block_size: u16,
    block_count: u16,
    argument: u32,
    transfer_mode: u16,
    command: u16,
    response: [u32; 4],
    host_control: u8,
    power_control: u8,
    block_gap_control: u8,
    clock_control: u16,
    timeout_control: u8,
    normal_int_status: u16,
    error_int_status: u16,
    normal_int_enable: u16,
    normal_int_signal_enable: u16,
    host_control2: u16,
    software_reset: u8,
    data_buffer: Vec<u8>,
    data_offset: usize,
    write_transfer_active: bool,
    app_command_pending: bool,
    snps_vendor_regs: Vec<u32>,
}

impl SdhciRegs {
    fn new() -> Self {
        Self {
            inserted: false,
            readonly: false,
            dma_address: 0,
            block_size: 0,
            block_count: 0,
            argument: 0,
            transfer_mode: 0,
            command: 0,
            response: [0; 4],
            host_control: 0,
            power_control: 0,
            block_gap_control: 0,
            clock_control: 0,
            timeout_control: 0,
            normal_int_status: 0,
            error_int_status: 0,
            normal_int_enable: 0,
            normal_int_signal_enable: 0,
            host_control2: 0,
            software_reset: 0,
            data_buffer: Vec::new(),
            data_offset: 0,
            write_transfer_active: false,
            app_command_pending: false,
            snps_vendor_regs: vec![0; SNPS_VENDOR_REG_WORDS],
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
    dma_address_space: Mutex<Option<Arc<AddressSpace>>>,
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
            dma_address_space: Mutex::new(None),
        }
    }

    pub fn connect_bus(&self, bus: Arc<SdBus>) {
        *self.bus.lock().unwrap() = Some(bus);
    }

    pub fn set_dma_address_space(&self, address_space: Arc<AddressSpace>) {
        *self.dma_address_space.lock().unwrap() = Some(address_space);
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
        if (REG_RESPONSE0..REG_RESPONSE_END).contains(&offset) {
            return read_response_reg(&regs, offset, size);
        }

        let value = match offset {
            REG_DMA_ADDRESS => u64::from(regs.dma_address),
            REG_BLOCK_SIZE => u64::from(regs.block_size),
            REG_BLOCK_COUNT => u64::from(regs.block_count),
            REG_ARGUMENT => u64::from(regs.argument),
            REG_TRANSFER_MODE => u64::from(regs.transfer_mode),
            REG_COMMAND => u64::from(regs.command),
            REG_PRESENT_STATE => u64::from(regs.present_state()),
            REG_HOST_CONTROL => u64::from(regs.host_control),
            REG_POWER_CONTROL => u64::from(regs.power_control),
            REG_BLOCK_GAP_CONTROL => u64::from(regs.block_gap_control),
            REG_CLOCK_CONTROL => u64::from(clock_control_value(&regs)),
            REG_TIMEOUT_CONTROL => u64::from(regs.timeout_control),
            REG_SOFTWARE_RESET => u64::from(regs.software_reset),
            REG_NORMAL_INT_STATUS => u64::from(regs.normal_int_status),
            REG_ERROR_INT_STATUS => u64::from(regs.error_int_status),
            REG_NORMAL_INT_ENABLE => u64::from(regs.normal_int_enable),
            REG_NORMAL_INT_SIGNAL_ENABLE => {
                u64::from(regs.normal_int_signal_enable)
            }
            REG_HOST_CONTROL2 => u64::from(regs.host_control2),
            REG_CAPABILITIES => u64::from(default_capabilities()),
            REG_MAX_CURRENT => 0,
            REG_HOST_VERSION => u64::from(HOST_VERSION_SPEC_3_00),
            _ if is_snps_vendor_reg(offset) => {
                return read_snps_vendor_reg(&regs, offset, size);
            }
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
            REG_DMA_ADDRESS => {
                regs.dma_address = value as u32;
            }
            REG_BLOCK_SIZE => {
                regs.block_size = value as u16;
            }
            REG_BLOCK_COUNT => {
                regs.block_count = value as u16;
            }
            REG_ARGUMENT => {
                regs.argument = value as u32;
            }
            REG_TRANSFER_MODE => {
                regs.transfer_mode = value as u16;
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
            REG_HOST_CONTROL => {
                regs.host_control = value as u8;
            }
            REG_POWER_CONTROL => {
                regs.power_control = value as u8;
            }
            REG_BLOCK_GAP_CONTROL => {
                regs.block_gap_control = value as u8;
            }
            REG_CLOCK_CONTROL => {
                regs.clock_control = value as u16;
            }
            REG_TIMEOUT_CONTROL => {
                regs.timeout_control = value as u8;
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
            REG_HOST_CONTROL2 => {
                regs.host_control2 = value as u16;
            }
            _ if is_snps_vendor_reg(offset) => {
                write_snps_vendor_reg(&mut regs, offset, size, value);
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
            Ok(n) => {
                let (
                    block_len,
                    block_count,
                    dma_address,
                    dma_enabled,
                    app_command_pending,
                ) = {
                    let regs = self.regs.lock();
                    (
                        regs.transfer_block_len(),
                        regs.transfer_block_count(),
                        regs.dma_address,
                        regs.transfer_mode & TRANSFER_MODE_DMA_ENABLE != 0,
                        regs.app_command_pending,
                    )
                };
                let dma_address_space = if dma_enabled {
                    self.dma_address_space.lock().unwrap().clone()
                } else {
                    None
                };
                let mut read_buffer = None;
                let mut write_buffer_len = None;
                let mut dma_complete = false;

                if n > 0
                    && is_read_data_command(cmd, app_command_pending)
                    && bus.data_ready()
                {
                    let transfer_len =
                        block_len * transfer_blocks(cmd, block_count);
                    let mut data = vec![0; transfer_len];
                    if bus.read_data(&mut data).is_ok() {
                        if let Some(ref aspace) = dma_address_space {
                            write_dma_buffer(aspace, dma_address, &data);
                            dma_complete = true;
                        } else {
                            read_buffer = Some(data);
                        }
                    }
                } else if n > 0
                    && is_write_data_command(cmd)
                    && bus.receive_ready()
                {
                    let transfer_len =
                        block_len * transfer_blocks(cmd, block_count);
                    if let Some(ref aspace) = dma_address_space {
                        let data =
                            read_dma_buffer(aspace, dma_address, transfer_len);
                        if bus.write_data(&data).is_ok() {
                            dma_complete = true;
                        }
                    } else {
                        write_buffer_len = Some(transfer_len);
                    }
                }

                let mut regs = self.regs.lock();
                regs.app_command_pending = cmd == CMD_APP_CMD && n > 0;
                regs.response = response_words(cmd, &response);
                regs.normal_int_status |= INT_COMMAND_COMPLETE;
                if cmd == CMD_STOP_TRANSMISSION {
                    regs.normal_int_status |= INT_TRANSFER_COMPLETE;
                }
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
                } else if dma_complete {
                    regs.data_buffer.clear();
                    regs.data_offset = 0;
                    regs.write_transfer_active = false;
                    regs.normal_int_status &=
                        !(INT_BUFFER_READ_READY | INT_BUFFER_WRITE_READY);
                    regs.normal_int_status |= INT_TRANSFER_COMPLETE;
                }
            }
            Err(err) => {
                self.regs.lock().app_command_pending = false;
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

fn is_snps_vendor_reg(offset: u64) -> bool {
    (SNPS_VENDOR_REG_START..SNPS_VENDOR_REG_END).contains(&offset)
}

fn read_snps_vendor_reg(regs: &SdhciRegs, offset: u64, size: u32) -> u64 {
    let word_offset = offset & !0x3;
    let index = (word_offset >> 2) as usize;
    let mut word = regs.snps_vendor_regs.get(index).copied().unwrap_or(0);
    if word_offset == DWC_MSHC_PHY_CNFG {
        word |= DWC_MSHC_PHY_PWRGOOD;
    }
    let shift = ((offset & 0x3) * 8) as u32;
    (u64::from(word) >> shift) & mask_for_size(size)
}

fn write_snps_vendor_reg(
    regs: &mut SdhciRegs,
    offset: u64,
    size: u32,
    value: u64,
) {
    let word_offset = offset & !0x3;
    let index = (word_offset >> 2) as usize;
    let Some(word) = regs.snps_vendor_regs.get_mut(index) else {
        return;
    };
    let shift = ((offset & 0x3) * 8) as u32;
    let mask = (mask_for_size(size) as u32) << shift;
    *word = (*word & !mask) | (((value as u32) << shift) & mask);
}

fn clock_control_value(regs: &SdhciRegs) -> u16 {
    if regs.clock_control & CLOCK_INTERNAL_ENABLE != 0 {
        regs.clock_control | CLOCK_INTERNAL_STABLE
    } else {
        regs.clock_control & !CLOCK_INTERNAL_STABLE
    }
}

fn default_capabilities() -> u32 {
    (CAPABILITIES_BASE_CLOCK_MHZ << CAPABILITIES_BASE_CLOCK_SHIFT)
        | CAPABILITIES_CAN_DO_HISPD
        | CAPABILITIES_CAN_DO_SDMA
        | CAPABILITIES_CAN_VDD_330
}

fn is_write_data_command(cmd: u8) -> bool {
    matches!(cmd, CMD_WRITE_BLOCK | CMD_WRITE_MULTIPLE_BLOCK)
}

fn is_read_data_command(cmd: u8, app_command_pending: bool) -> bool {
    matches!(
        cmd,
        CMD_SWITCH_FUNC | CMD_READ_SINGLE_BLOCK | CMD_READ_MULTIPLE_BLOCK
    ) || (app_command_pending && matches!(cmd, CMD_SEND_STATUS | ACMD_SEND_SCR))
}

fn transfer_blocks(cmd: u8, block_count: usize) -> usize {
    if matches!(cmd, CMD_READ_MULTIPLE_BLOCK | CMD_WRITE_MULTIPLE_BLOCK) {
        block_count
    } else {
        1
    }
}

fn read_dma_buffer(
    address_space: &AddressSpace,
    address: u32,
    len: usize,
) -> Vec<u8> {
    let mut data = vec![0; len];
    let mut offset = 0;
    while offset < len {
        let addr = u64::from(address).wrapping_add(offset as u64);
        let width = dma_access_width(address_space, addr, len - offset);
        let value = address_space.read(GPA(addr), width);
        data[offset..offset + width as usize]
            .copy_from_slice(&value.to_le_bytes()[..width as usize]);
        offset += width as usize;
    }
    data
}

fn write_dma_buffer(address_space: &AddressSpace, address: u32, data: &[u8]) {
    let mut offset = 0;
    while offset < data.len() {
        let addr = u64::from(address).wrapping_add(offset as u64);
        let width = dma_access_width(address_space, addr, data.len() - offset);
        let mut bytes = [0; 8];
        bytes[..width as usize]
            .copy_from_slice(&data[offset..offset + width as usize]);
        address_space.write(GPA(addr), width, u64::from_le_bytes(bytes));
        offset += width as usize;
    }
}

fn dma_access_width(
    address_space: &AddressSpace,
    address: u64,
    remaining: usize,
) -> u32 {
    for width in [4, 2, 1] {
        if remaining >= width as usize
            && address & u64::from(width - 1) == 0
            && address_space.is_mapped(GPA(address), width)
        {
            return width;
        }
    }
    1
}

fn read_response_reg(regs: &SdhciRegs, offset: u64, size: u32) -> u64 {
    let mut bytes = [0; 16];
    for (index, word) in regs.response.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    let start = (offset - REG_RESPONSE0) as usize;
    let len = size as usize;
    let mut value = [0; 8];
    value[..len].copy_from_slice(&bytes[start..start + len]);
    u64::from_le_bytes(value) & mask_for_size(size)
}

fn response_words(cmd: u8, response: &[u8; 16]) -> [u32; 4] {
    let words = [
        u32::from_be_bytes(response[0..4].try_into().unwrap()),
        u32::from_be_bytes(response[4..8].try_into().unwrap()),
        u32::from_be_bytes(response[8..12].try_into().unwrap()),
        u32::from_be_bytes(response[12..16].try_into().unwrap()),
    ];

    if matches!(cmd, CMD_ALL_SEND_CID | CMD_SEND_CSD | CMD_SEND_CID) {
        return response_words_long(words);
    }

    words
}

fn response_words_long(words: [u32; 4]) -> [u32; 4] {
    [
        (words[3] >> 8) | ((words[2] & 0xff) << 24),
        (words[2] >> 8) | ((words[1] & 0xff) << 24),
        (words[1] >> 8) | ((words[0] & 0xff) << 24),
        words[0] >> 8,
    ]
}
