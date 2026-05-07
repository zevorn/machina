//! PL181-style SD/MMC host controller.
//!
//! This module provides the host-controller side used by board code to
//! connect an [`crate::SdBus`] to memory-mapped controller registers.

use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::irq::InterruptSource;
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

use crate::{SdBus, SdError, SdRequest};

const REG_POWER: u64 = 0x00;
const REG_CLOCK: u64 = 0x04;
const REG_ARGUMENT: u64 = 0x08;
const REG_COMMAND: u64 = 0x0c;
const REG_RESPCMD: u64 = 0x10;
const REG_RESPONSE0: u64 = 0x14;
const REG_RESPONSE1: u64 = 0x18;
const REG_RESPONSE2: u64 = 0x1c;
const REG_RESPONSE3: u64 = 0x20;
const REG_DATATIMER: u64 = 0x24;
const REG_DATALENGTH: u64 = 0x28;
const REG_DATACTRL: u64 = 0x2c;
const REG_DATACNT: u64 = 0x30;
const REG_STATUS: u64 = 0x34;
const REG_CLEAR: u64 = 0x38;
const REG_MASK0: u64 = 0x3c;
const REG_MASK1: u64 = 0x40;
const REG_FIFO: u64 = 0x80;

pub const PL181_IRQ0: u32 = 0;
pub const PL181_IRQ1: u32 = 1;
const PL181_NUM_IRQS: usize = 2;

const COMMAND_INDEX_MASK: u32 = 0x3f;
const COMMAND_RESPONSE: u32 = 1 << 6;
const COMMAND_ENABLE: u32 = 1 << 10;

const DATACTRL_ENABLE: u32 = 1 << 0;
const DATACTRL_DIRECTION: u32 = 1 << 1;

const STATUS_COMMAND_TIMEOUT: u32 = 1 << 2;
const STATUS_COMMAND_RESPONSE_END: u32 = 1 << 6;
const STATUS_COMMAND_SENT: u32 = 1 << 7;
const STATUS_DATA_END: u32 = 1 << 8;
const STATUS_DATA_BLOCK_END: u32 = 1 << 10;
const STATUS_TX_FIFO_EMPTY: u32 = 1 << 18;
const STATUS_RX_FIFO_EMPTY: u32 = 1 << 19;
const STATUS_RX_DATA_AVAILABLE: u32 = 1 << 21;
const RESET_STATUS: u32 = STATUS_TX_FIFO_EMPTY | STATUS_RX_FIFO_EMPTY;
const DEFAULT_BLOCK_LEN: usize = 512;
const CMD_READ_SINGLE_BLOCK: u8 = 17;
const CMD_READ_MULTIPLE_BLOCK: u8 = 18;
const CMD_WRITE_BLOCK: u8 = 24;
const CMD_WRITE_MULTIPLE_BLOCK: u8 = 25;

#[derive(Debug, PartialEq, Eq)]
struct Pl181Regs {
    power: u32,
    clock: u32,
    argument: u32,
    command: u32,
    response_cmd: u32,
    response: [u32; 4],
    data_timer: u32,
    data_length: u32,
    data_ctrl: u32,
    data_count: u32,
    status: u32,
    mask0: u32,
    mask1: u32,
    data_buffer: Vec<u8>,
    data_offset: usize,
    write_transfer_active: bool,
    write_transfer_len: usize,
}

impl Pl181Regs {
    fn new() -> Self {
        Self {
            power: 0,
            clock: 0,
            argument: 0,
            command: 0,
            response_cmd: 0,
            response: [0; 4],
            data_timer: 0,
            data_length: 0,
            data_ctrl: 0,
            data_count: 0,
            status: RESET_STATUS,
            mask0: 0,
            mask1: 0,
            data_buffer: Vec::new(),
            data_offset: 0,
            write_transfer_active: false,
            write_transfer_len: 0,
        }
    }

    fn transfer_len(&self) -> usize {
        if self.data_length == 0 {
            DEFAULT_BLOCK_LEN
        } else {
            self.data_length as usize
        }
    }

    fn read_data_enabled(&self) -> bool {
        self.data_ctrl & DATACTRL_ENABLE != 0
            && self.data_ctrl & DATACTRL_DIRECTION != 0
    }

    fn write_data_enabled(&self) -> bool {
        self.data_ctrl & DATACTRL_ENABLE != 0
            && self.data_ctrl & DATACTRL_DIRECTION == 0
    }
}

pub struct Pl181 {
    state: Mutex<SysBusDeviceState>,
    regs: Mutex<Pl181Regs>,
    bus: Mutex<Option<Arc<SdBus>>>,
    outputs: Mutex<Vec<Option<InterruptSource>>>,
}

impl Pl181 {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("pl181")
    }

    #[must_use]
    pub fn new_named(local_id: &str) -> Self {
        Self {
            state: Mutex::new(SysBusDeviceState::new(local_id)),
            regs: Mutex::new(Pl181Regs::new()),
            bus: Mutex::new(None),
            outputs: Mutex::new({
                let mut v = Vec::with_capacity(PL181_NUM_IRQS);
                v.resize_with(PL181_NUM_IRQS, || None);
                v
            }),
        }
    }

    pub fn attach_to_bus(&self, bus: &mut SysBus) -> Result<(), SysBusError> {
        self.state.lock().unwrap().attach_to_bus(bus)
    }

    pub fn register_mmio(
        &self,
        region: MemoryRegion,
        base: GPA,
    ) -> Result<(), SysBusError> {
        self.state.lock().unwrap().register_mmio(region, base)
    }

    pub fn realize_onto(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.state.lock().unwrap().realize_onto(bus, address_space)
    }

    pub fn unrealize_from(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.state
            .lock()
            .unwrap()
            .unrealize_from(bus, address_space)
    }

    #[must_use]
    pub fn realized(&self) -> bool {
        self.state.lock().unwrap().device().is_realized()
    }

    pub fn with_mdevice<T>(&self, f: impl FnOnce(&dyn MDevice) -> T) -> T {
        let guard = self.state.lock().unwrap();
        f(&*guard)
    }

    #[must_use]
    pub fn object_info(&self) -> MObjectInfo {
        self.state.lock().unwrap().object_info()
    }

    pub fn connect_bus(&self, bus: Arc<SdBus>) {
        *self.bus.lock().unwrap() = Some(bus);
    }

    pub fn connect_output(&self, idx: u32, irq: InterruptSource) {
        let mut outputs = self.outputs.lock().unwrap();
        if (idx as usize) < outputs.len() {
            outputs[idx as usize] = Some(irq);
        }
        drop(outputs);
        self.update_irqs();
    }

    pub fn reset_runtime(&self) {
        *self.regs.lock().unwrap() = Pl181Regs::new();
        self.update_irqs();
    }

    fn read_reg(&self, offset: u64, size: u32) -> u64 {
        if let Some(value) = read_unaligned(self, offset, size) {
            return value;
        }

        if size == 8 {
            let lo = self.read_reg(offset, 4);
            let hi = self.read_reg(offset.wrapping_add(4), 4);
            return lo | (hi << 32);
        }

        if offset == REG_FIFO {
            return self.read_fifo(size);
        }

        let regs = self.regs.lock().unwrap();
        let value = match offset {
            REG_POWER => regs.power,
            REG_CLOCK => regs.clock,
            REG_ARGUMENT => regs.argument,
            REG_COMMAND => regs.command,
            REG_RESPCMD => regs.response_cmd,
            REG_RESPONSE0 => regs.response[0],
            REG_RESPONSE1 => regs.response[1],
            REG_RESPONSE2 => regs.response[2],
            REG_RESPONSE3 => regs.response[3],
            REG_DATATIMER => regs.data_timer,
            REG_DATALENGTH => regs.data_length,
            REG_DATACTRL => regs.data_ctrl,
            REG_DATACNT => regs.data_count,
            REG_STATUS => regs.status,
            REG_CLEAR => 0,
            REG_MASK0 => regs.mask0,
            REG_MASK1 => regs.mask1,
            _ => 0,
        };
        u64::from(value) & mask_for_size(size)
    }

    fn write_reg(&self, offset: u64, size: u32, value: u64) {
        if write_unaligned(self, offset, size, value) {
            return;
        }

        if size == 8 {
            self.write_reg(offset, 4, value);
            self.write_reg(offset.wrapping_add(4), 4, value >> 32);
            return;
        }

        let value = (value & mask_for_size(size)) as u32;
        if offset == REG_FIFO {
            self.write_fifo(size, value);
            return;
        }

        let mut dispatch = None;
        let mut update_irq = false;

        {
            let mut regs = self.regs.lock().unwrap();
            match offset {
                REG_POWER => regs.power = value & 0xff,
                REG_CLOCK => regs.clock = value & 0xff,
                REG_ARGUMENT => regs.argument = value,
                REG_COMMAND => {
                    regs.command = value;
                    if regs.command & COMMAND_ENABLE != 0 {
                        dispatch = Some((regs.command, regs.argument));
                    }
                }
                REG_DATATIMER => regs.data_timer = value,
                REG_DATALENGTH => regs.data_length = value & 0xffff,
                REG_DATACTRL => regs.data_ctrl = value & 0xff,
                REG_CLEAR => {
                    regs.status &= !value;
                    update_irq = true;
                }
                REG_MASK0 => {
                    regs.mask0 = value;
                    update_irq = true;
                }
                REG_MASK1 => {
                    regs.mask1 = value;
                    update_irq = true;
                }
                _ => {}
            }
        }

        if let Some((command, argument)) = dispatch {
            self.dispatch_command(command, argument);
        } else if update_irq {
            self.update_irqs();
        }
    }

    fn dispatch_command(&self, command: u32, argument: u32) {
        let cmd = (command & COMMAND_INDEX_MASK) as u8;
        let Some(bus) = self.bus.lock().unwrap().clone() else {
            self.record_command_error(SdError::NoCard);
            return;
        };

        let mut response = [0; 16];
        match bus.do_command(&SdRequest::new(cmd, argument), &mut response) {
            Ok(n) => {
                let mut read_buffer = None;
                let mut write_transfer_len = None;
                let (read_enabled, write_enabled, transfer_len) = {
                    let regs = self.regs.lock().unwrap();
                    (
                        regs.read_data_enabled(),
                        regs.write_data_enabled(),
                        regs.transfer_len(),
                    )
                };
                if read_enabled && is_read_data_command(cmd) && bus.data_ready()
                {
                    let mut data = vec![0; transfer_len];
                    if bus.read_data(&mut data).is_ok() {
                        read_buffer = Some(data);
                    }
                } else if write_enabled
                    && is_write_data_command(cmd)
                    && bus.receive_ready()
                {
                    write_transfer_len = Some(transfer_len);
                }

                let mut regs = self.regs.lock().unwrap();
                regs.response_cmd = u32::from(cmd);
                regs.response = response_words(&response);
                regs.status &= !STATUS_COMMAND_TIMEOUT;
                if command & COMMAND_RESPONSE != 0 && n > 0 {
                    regs.status |= STATUS_COMMAND_RESPONSE_END;
                } else {
                    regs.status |= STATUS_COMMAND_SENT;
                }
                if let Some(data) = read_buffer {
                    regs.data_buffer = data;
                    regs.data_offset = 0;
                    regs.data_count = regs.data_buffer.len() as u32;
                    regs.status |= STATUS_RX_DATA_AVAILABLE;
                    regs.status &= !(STATUS_RX_FIFO_EMPTY
                        | STATUS_DATA_END
                        | STATUS_DATA_BLOCK_END);
                }
                if let Some(len) = write_transfer_len {
                    regs.data_buffer.clear();
                    regs.data_offset = 0;
                    regs.data_count = len as u32;
                    regs.write_transfer_active = true;
                    regs.write_transfer_len = len;
                    regs.status &= !(STATUS_DATA_END | STATUS_DATA_BLOCK_END);
                }
                drop(regs);
                self.update_irqs();
            }
            Err(err) => self.record_command_error(err),
        }
    }

    fn record_command_error(&self, _err: SdError) {
        let mut regs = self.regs.lock().unwrap();
        regs.status |= STATUS_COMMAND_TIMEOUT;
        drop(regs);
        self.update_irqs();
    }

    fn read_fifo(&self, size: u32) -> u64 {
        let len = (size as usize).min(8);
        let mut bytes = [0; 8];
        let mut regs = self.regs.lock().unwrap();

        for byte in bytes.iter_mut().take(len) {
            if regs.data_offset >= regs.data_buffer.len() {
                break;
            }
            *byte = regs.data_buffer[regs.data_offset];
            regs.data_offset += 1;
        }

        regs.data_count =
            (regs.data_buffer.len().saturating_sub(regs.data_offset)) as u32;
        let update_irq = regs.data_offset >= regs.data_buffer.len()
            && !regs.data_buffer.is_empty();
        if update_irq {
            regs.data_buffer.clear();
            regs.data_offset = 0;
            regs.status &= !STATUS_RX_DATA_AVAILABLE;
            regs.status |=
                STATUS_RX_FIFO_EMPTY | STATUS_DATA_END | STATUS_DATA_BLOCK_END;
        }

        let value = u64::from_le_bytes(bytes) & mask_for_size(size);
        drop(regs);
        if update_irq {
            self.update_irqs();
        }
        value
    }

    fn write_fifo(&self, size: u32, value: u32) {
        let len = (size as usize).min(4);
        let bytes = value.to_le_bytes();
        let mut completed = None;

        {
            let mut regs = self.regs.lock().unwrap();
            if !regs.write_transfer_active {
                return;
            }
            let remaining = regs
                .write_transfer_len
                .saturating_sub(regs.data_buffer.len());
            let n = len.min(remaining);
            regs.data_buffer.extend_from_slice(&bytes[..n]);
            regs.data_count =
                (regs.write_transfer_len - regs.data_buffer.len()) as u32;
            if regs.data_buffer.len() >= regs.write_transfer_len {
                completed = Some(std::mem::take(&mut regs.data_buffer));
                regs.data_offset = 0;
                regs.write_transfer_active = false;
                regs.write_transfer_len = 0;
                regs.status |= STATUS_DATA_END | STATUS_DATA_BLOCK_END;
            }
        }

        if let Some(data) = completed {
            if self
                .bus
                .lock()
                .unwrap()
                .clone()
                .is_some_and(|bus| bus.write_data(&data).is_ok())
            {
                return;
            }
            let mut regs = self.regs.lock().unwrap();
            regs.status &= !(STATUS_DATA_END | STATUS_DATA_BLOCK_END);
        }
        self.update_irqs();
    }

    fn update_irqs(&self) {
        let (status, mask0, mask1) = {
            let regs = self.regs.lock().unwrap();
            (regs.status, regs.mask0, regs.mask1)
        };
        let outputs = self.outputs.lock().unwrap();
        if let Some(Some(line)) = outputs.get(PL181_IRQ0 as usize) {
            line.set(status & mask0 != 0);
        }
        if let Some(Some(line)) = outputs.get(PL181_IRQ1 as usize) {
            line.set(status & mask1 != 0);
        }
    }
}

impl Default for Pl181 {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Pl181Mmio(pub Arc<Pl181>);

impl MmioOps for Pl181Mmio {
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

fn read_unaligned(dev: &Pl181, offset: u64, size: u32) -> Option<u64> {
    if !needs_unaligned_split(offset, size) {
        return None;
    }

    let mut value = 0u64;
    let mut done = 0u32;
    while done < size {
        let cur = offset + u64::from(done);
        let chunk = aligned_chunk_size(cur, size - done);
        value |=
            (dev.read_reg(cur, chunk) & mask_for_size(chunk)) << (done * 8);
        done += chunk;
    }
    Some(value)
}

fn write_unaligned(dev: &Pl181, offset: u64, size: u32, value: u64) -> bool {
    if !needs_unaligned_split(offset, size) {
        return false;
    }

    let mut done = 0u32;
    while done < size {
        let cur = offset + u64::from(done);
        let chunk = aligned_chunk_size(cur, size - done);
        let chunk_value = (value >> (done * 8)) & mask_for_size(chunk);
        dev.write_reg(cur, chunk, chunk_value);
        done += chunk;
    }
    true
}

fn needs_unaligned_split(offset: u64, size: u32) -> bool {
    matches!(size, 2 | 4 | 8) && !offset.is_multiple_of(u64::from(size))
}

fn aligned_chunk_size(offset: u64, remaining: u32) -> u32 {
    for size in [8u32, 4, 2, 1] {
        if remaining >= size && offset.is_multiple_of(u64::from(size)) {
            return size;
        }
    }
    1
}

fn response_words(response: &[u8; 16]) -> [u32; 4] {
    [
        u32::from_be_bytes(response[0..4].try_into().unwrap()),
        u32::from_be_bytes(response[4..8].try_into().unwrap()),
        u32::from_be_bytes(response[8..12].try_into().unwrap()),
        u32::from_be_bytes(response[12..16].try_into().unwrap()),
    ]
}

fn is_read_data_command(cmd: u8) -> bool {
    matches!(cmd, CMD_READ_SINGLE_BLOCK | CMD_READ_MULTIPLE_BLOCK)
}

fn is_write_data_command(cmd: u8) -> bool {
    matches!(cmd, CMD_WRITE_BLOCK | CMD_WRITE_MULTIPLE_BLOCK)
}
