use std::sync::{Arc, Mutex, MutexGuard};

use machina_core::address::GPA;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

use crate::{BlockBackend, FlashMedia, StorageError};

const CFI01_TABLE_LEN: usize = 0x52;
const CFI02_TABLE_LEN: usize = 0x4d;
const STATUS_READY: u8 = 0x80;
const STATUS_PROGRAM_ERROR: u8 = 0x10;
const STATUS_ERASE_ERROR: u8 = 0x20;
const CFI02_WCYCLE_CFI: u8 = 7;
const CFI02_WCYCLE_AUTOSELECT_CFI: u8 = 8;

#[derive(Clone, Copy)]
pub struct PFlashCfi01Config {
    pub bank_width: u8,
    pub device_width: u8,
    pub max_device_width: u8,
    pub sector_len: u64,
    pub num_blocks: u32,
    pub big_endian: bool,
    pub read_only: bool,
    pub ident0: u16,
    pub ident1: u16,
    pub ident2: u16,
    pub ident3: u16,
}

impl Default for PFlashCfi01Config {
    fn default() -> Self {
        Self {
            bank_width: 1,
            device_width: 0,
            max_device_width: 0,
            sector_len: 4096,
            num_blocks: 1,
            big_endian: false,
            read_only: false,
            ident0: 0,
            ident1: 0,
            ident2: 0,
            ident3: 0,
        }
    }
}

struct PFlashCfi01Regs {
    wcycle: u8,
    cmd: u8,
    status: u8,
    counter: u64,
    buffer: Option<PFlashCfi01Buffer>,
    cfi_table: [u8; CFI01_TABLE_LEN],
}

struct PFlashCfi01Buffer {
    base: u64,
    bytes: Vec<u8>,
}

pub struct PFlashCfi01<B: BlockBackend> {
    state: Mutex<SysBusDeviceState>,
    flash: FlashMedia<B>,
    config: PFlashCfi01Config,
    writeblock_size: u64,
    regs: Mutex<PFlashCfi01Regs>,
}

impl<B: BlockBackend> PFlashCfi01<B> {
    pub fn new(
        flash: FlashMedia<B>,
        config: PFlashCfi01Config,
    ) -> Result<Self, StorageError> {
        Self::new_named("pflash-cfi01", flash, config)
    }

    pub fn new_named(
        local_id: &str,
        flash: FlashMedia<B>,
        config: PFlashCfi01Config,
    ) -> Result<Self, StorageError> {
        validate_common(config.sector_len, config.num_blocks)?;
        if config.bank_width == 0 || !config.bank_width.is_power_of_two() {
            return Err(StorageError::InvalidInput(
                "bank_width must be a non-zero power of two".to_string(),
            ));
        }
        if config.device_width != 0
            && (!config.device_width.is_power_of_two()
                || config.device_width > config.bank_width)
        {
            return Err(StorageError::InvalidInput(
                "device_width must be zero or a power of two no larger than bank_width"
                    .to_string(),
            ));
        }
        if config.max_device_width != 0
            && (!config.max_device_width.is_power_of_two()
                || config.max_device_width < config.device_width)
        {
            return Err(StorageError::InvalidInput(
                "max_device_width must be zero or a power of two at least device_width"
                    .to_string(),
            ));
        }
        let cfi_table = cfi01_table(&config);
        let writeblock_size = cfi01_writeblock_size(&config);
        Ok(Self {
            state: Mutex::new(SysBusDeviceState::new(local_id)),
            flash: flash.with_readonly(config.read_only),
            config,
            writeblock_size,
            regs: Mutex::new(PFlashCfi01Regs {
                wcycle: 0,
                cmd: 0,
                status: STATUS_READY,
                counter: 0,
                buffer: None,
                cfi_table,
            }),
        })
    }

    pub fn attach_to_bus(&self, bus: &mut SysBus) -> Result<(), SysBusError> {
        lock(&self.state).attach_to_bus(bus)
    }

    pub fn register_mmio(
        &self,
        region: MemoryRegion,
        base: GPA,
    ) -> Result<(), SysBusError> {
        lock(&self.state).register_mmio(region, base)
    }

    pub fn realize_onto(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        lock(&self.state).realize_onto(bus, address_space)
    }

    pub fn unrealize_from(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        lock(&self.state).unrealize_from(bus, address_space)
    }

    pub fn realized(&self) -> bool {
        lock(&self.state).device().is_realized()
    }

    pub fn with_mdevice<T>(&self, f: impl FnOnce(&dyn MDevice) -> T) -> T {
        let guard = lock(&self.state);
        f(&*guard)
    }

    pub fn object_info(&self) -> MObjectInfo {
        lock(&self.state).object_info()
    }

    pub fn reset_runtime(&self) {
        let mut regs = lock(&self.regs);
        regs.wcycle = 0;
        regs.cmd = 0;
        regs.status = STATUS_READY;
        regs.counter = 0;
        regs.buffer = None;
    }

    pub fn do_read(&self, offset: u64, size: u32) -> u64 {
        if size == 0 || size > 8 {
            return 0;
        }
        let mut regs = lock(&self.regs);
        match regs.cmd {
            0x00 => self.read_data(offset, size),
            0x10 | 0x20 | 0x28 | 0x40 | 0x50 | 0x60 | 0x70 | 0xe8 => {
                replicate_status(regs.status, size, self.config.device_width)
            }
            0x90 => self.read_cfi01_id(offset, size),
            0x98 => self.read_cfi01_query(&regs, offset, size),
            _ => {
                regs.wcycle = 0;
                regs.cmd = 0;
                self.read_data(offset, size)
            }
        }
    }

    pub fn do_write(&self, offset: u64, size: u32, value: u64) {
        if size == 0 || size > 8 {
            return;
        }
        let cmd = value as u8;
        let mut regs = lock(&self.regs);
        match regs.wcycle {
            0 => self.write_cfi01_cycle0(&mut regs, size, value, cmd),
            1 => self.write_cfi01_cycle1(&mut regs, offset, size, value, cmd),
            2 => self.write_cfi01_cycle2(&mut regs, offset, size, value),
            3 => self.write_cfi01_cycle3(&mut regs, cmd),
            _ => Self::cfi01_read_array(&mut regs),
        }
    }

    fn write_cfi01_cycle0(
        &self,
        regs: &mut PFlashCfi01Regs,
        size: u32,
        value: u64,
        cmd: u8,
    ) {
        match cmd {
            0x00 | 0xff | 0xf0 => Self::cfi01_read_array(regs),
            0x10 | 0x40 | 0x60 | 0x98 => {
                regs.wcycle = 1;
                regs.cmd = cmd;
            }
            0xe8 => {
                regs.status |= STATUS_READY;
                regs.wcycle = 1;
                regs.cmd = cmd;
                regs.counter = 0;
                regs.buffer = None;
            }
            0x20 => {
                regs.status |= STATUS_READY;
                regs.wcycle = 1;
                regs.cmd = cmd;
            }
            0x50 => {
                regs.status = STATUS_READY;
                Self::cfi01_read_array(regs);
            }
            0x70 | 0x90 => {
                regs.cmd = cmd;
            }
            _ => Self::cfi01_read_array(regs),
        }
        let _ = (size, value);
    }

    fn write_cfi01_cycle1(
        &self,
        regs: &mut PFlashCfi01Regs,
        offset: u64,
        size: u32,
        value: u64,
        cmd: u8,
    ) {
        match regs.cmd {
            0x10 | 0x40 => {
                if self.program_value(offset, size, value).is_err() {
                    regs.status |= STATUS_PROGRAM_ERROR;
                }
                regs.status |= STATUS_READY;
                regs.wcycle = 0;
            }
            0x20 | 0x28 => match cmd {
                0xd0 => {
                    self.erase_cfi01_sector(regs, offset);
                    regs.status |= STATUS_READY;
                    regs.wcycle = 0;
                }
                _ => Self::cfi01_read_array(regs),
            },
            0x60 => {
                if cmd == 0xd0 || cmd == 0x01 {
                    regs.status |= STATUS_READY;
                    regs.wcycle = 0;
                } else {
                    Self::cfi01_read_array(regs);
                }
            }
            0x98 => {
                if cmd == 0xff || cmd == 0xf0 {
                    Self::cfi01_read_array(regs);
                }
            }
            0xe8 => {
                regs.counter = mask_width(value, size);
                regs.wcycle = 2;
            }
            _ => Self::cfi01_read_array(regs),
        }
    }

    fn write_cfi01_cycle2(
        &self,
        regs: &mut PFlashCfi01Regs,
        offset: u64,
        size: u32,
        value: u64,
    ) {
        if regs.buffer.is_none() {
            match self.start_cfi01_buffer(offset) {
                Ok(buffer) => regs.buffer = Some(buffer),
                Err(_) => regs.status |= STATUS_PROGRAM_ERROR,
            }
        }
        if let Some(buffer) = regs.buffer.as_mut() {
            let bytes = value_to_bytes(value, size, self.config.big_endian);
            let start = offset.saturating_sub(buffer.base);
            let end = start.saturating_add(bytes.len() as u64);
            if offset < buffer.base || end > buffer.bytes.len() as u64 {
                regs.status |= STATUS_PROGRAM_ERROR;
            } else {
                let start = start as usize;
                for (dst, src) in buffer.bytes[start..start + bytes.len()]
                    .iter_mut()
                    .zip(bytes.iter())
                {
                    *dst &= *src;
                }
            }
        } else {
            regs.status |= STATUS_PROGRAM_ERROR;
        }
        regs.status |= STATUS_READY;
        if regs.counter == 0 {
            regs.wcycle = 3;
        } else {
            regs.counter -= 1;
        }
    }

    fn write_cfi01_cycle3(&self, regs: &mut PFlashCfi01Regs, cmd: u8) {
        if cmd == 0xd0 && regs.status & STATUS_PROGRAM_ERROR == 0 {
            if let Some(buffer) = regs.buffer.take() {
                if self.flash.program(buffer.base, &buffer.bytes).is_err() {
                    regs.status |= STATUS_PROGRAM_ERROR;
                }
            }
            regs.status |= STATUS_READY;
            regs.wcycle = 0;
        } else {
            regs.buffer = None;
            Self::cfi01_read_array(regs);
        }
    }

    fn cfi01_read_array(regs: &mut PFlashCfi01Regs) {
        regs.wcycle = 0;
        regs.cmd = 0;
        regs.counter = 0;
    }

    fn read_data(&self, offset: u64, size: u32) -> u64 {
        let mut bytes = vec![0; size as usize];
        if self.flash.read(offset, &mut bytes).is_err() {
            return 0;
        }
        bytes_to_value(&bytes, self.config.big_endian)
    }

    fn read_cfi01_id(&self, offset: u64, size: u32) -> u64 {
        if self.config.device_width == 0 {
            let mut boff = offset & 0xff;
            if self.config.bank_width == 2 {
                boff >>= 1;
            } else if self.config.bank_width == 4 {
                boff >>= 2;
            }
            return mask_width(
                match boff {
                    0 => u64::from(
                        (self.config.ident0 << 8) | self.config.ident1,
                    ),
                    1 => u64::from(
                        (self.config.ident2 << 8) | self.config.ident3,
                    ),
                    _ => 0,
                },
                size,
            );
        }
        self.combine_cfi01_bank_read(offset, size, |query_offset| {
            let value = match self.adjust_width_offset(query_offset) & 0xff {
                0 => u64::from(self.config.ident0),
                1 => u64::from(self.config.ident1),
                _ => 0,
            };
            self.replicate_cfi01_bank_value(value)
        })
    }

    fn read_cfi01_query(
        &self,
        regs: &PFlashCfi01Regs,
        offset: u64,
        size: u32,
    ) -> u64 {
        if self.config.device_width == 0 {
            let mut boff = offset & 0xff;
            if self.config.bank_width == 2 {
                boff >>= 1;
            } else if self.config.bank_width == 4 {
                boff >>= 2;
            }
            return regs
                .cfi_table
                .get(boff as usize)
                .map_or(0, |byte| u64::from(*byte));
        }
        self.combine_cfi01_bank_read(offset, size, |query_offset| {
            self.cfi01_query_bank_value(regs, query_offset)
        })
    }

    fn adjust_width_offset(&self, offset: u64) -> u64 {
        let max_width = self
            .config
            .max_device_width
            .max(self.config.device_width)
            .max(1);
        let shift = trailing_log2(self.config.bank_width)
            + trailing_log2(max_width)
            - trailing_log2(self.config.device_width.max(1));
        offset >> shift
    }

    fn cfi01_query_bank_value(
        &self,
        regs: &PFlashCfi01Regs,
        offset: u64,
    ) -> u64 {
        let boff = self.adjust_width_offset(offset);
        let mut value =
            u64::from(regs.cfi_table.get(boff as usize).copied().unwrap_or(0));
        let max_width = self
            .config
            .max_device_width
            .max(self.config.device_width)
            .max(1);
        if self.config.device_width != max_width {
            if self.config.device_width != 1 || self.config.bank_width > 4 {
                return 0;
            }
            let byte = value;
            value = 0;
            for index in 0..max_width {
                value |= byte << (index * 8);
            }
        }
        self.replicate_cfi01_bank_value(value)
    }

    fn replicate_cfi01_bank_value(&self, value: u64) -> u64 {
        let device_width = self.config.device_width.max(1);
        let bank_width = self.config.bank_width;
        if device_width >= bank_width {
            return mask_width(value, u32::from(bank_width));
        }
        let unit = mask_width(value, u32::from(device_width));
        let mut out = 0;
        let mut shift = 0;
        while shift < bank_width {
            out |= unit << (usize::from(shift) * 8);
            shift += device_width;
        }
        out
    }

    fn combine_cfi01_bank_read<F>(
        &self,
        offset: u64,
        size: u32,
        mut read: F,
    ) -> u64
    where
        F: FnMut(u64) -> u64,
    {
        let bank_width = u32::from(self.config.bank_width);
        let mut out = 0;
        let mut index = 0;
        while index < size {
            let query_offset =
                offset + u64::from(index) * u64::from(bank_width);
            let chunk = bank_width.min(size - index);
            let shift = if self.config.big_endian {
                (size - index - chunk) * 8
            } else {
                index * 8
            };
            out |= mask_width(read(query_offset), chunk) << shift;
            index += bank_width;
        }
        out
    }

    fn erase_cfi01_sector(&self, regs: &mut PFlashCfi01Regs, offset: u64) {
        let base = align_down(offset, self.config.sector_len);
        if self.flash.erase(base, self.config.sector_len).is_err() {
            regs.status |= STATUS_ERASE_ERROR;
        }
    }

    fn program_value(
        &self,
        offset: u64,
        size: u32,
        value: u64,
    ) -> Result<(), StorageError> {
        let bytes = value_to_bytes(value, size, self.config.big_endian);
        self.flash.program(offset, &bytes)
    }

    fn start_cfi01_buffer(
        &self,
        offset: u64,
    ) -> Result<PFlashCfi01Buffer, StorageError> {
        let base = align_down(offset, self.writeblock_size);
        let mut bytes = vec![0; self.writeblock_size as usize];
        self.flash.read(base, &mut bytes)?;
        Ok(PFlashCfi01Buffer { base, bytes })
    }
}

pub struct PFlashCfi01Mmio<B: BlockBackend>(pub Arc<PFlashCfi01<B>>);

impl<B: BlockBackend> MmioOps for PFlashCfi01Mmio<B> {
    fn read(&self, offset: u64, size: u32) -> u64 {
        if size == 8 {
            let lo = self.0.do_read(offset, 4);
            let hi = self.0.do_read(offset.wrapping_add(4), 4);
            return lo | (hi << 32);
        }
        self.0.do_read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        if size == 8 {
            self.0.do_write(offset, 4, val);
            self.0.do_write(offset.wrapping_add(4), 4, val >> 32);
            return;
        }
        self.0.do_write(offset, size, val);
    }
}

#[derive(Clone, Copy)]
pub struct PFlashCfi02Config {
    pub width: u8,
    pub sector_len: u64,
    pub num_blocks: u32,
    pub big_endian: bool,
    pub read_only: bool,
    pub ident0: u16,
    pub ident1: u16,
    pub ident2: u16,
    pub ident3: u16,
    pub unlock_addr0: u16,
    pub unlock_addr1: u16,
}

impl Default for PFlashCfi02Config {
    fn default() -> Self {
        Self {
            width: 1,
            sector_len: 4096,
            num_blocks: 1,
            big_endian: false,
            read_only: false,
            ident0: 0,
            ident1: 0,
            ident2: 0xffff,
            ident3: 0xffff,
            unlock_addr0: 0x555,
            unlock_addr1: 0x2aa,
        }
    }
}

struct PFlashCfi02Regs {
    wcycle: u8,
    cmd: u8,
    status: u8,
    bypass: bool,
    erase_suspended: bool,
    erasing_ranges: Vec<(u64, u64)>,
    cfi_table: [u8; CFI02_TABLE_LEN],
}

pub struct PFlashCfi02<B: BlockBackend> {
    state: Mutex<SysBusDeviceState>,
    flash: FlashMedia<B>,
    config: PFlashCfi02Config,
    chip_len: u64,
    regs: Mutex<PFlashCfi02Regs>,
}

impl<B: BlockBackend> PFlashCfi02<B> {
    pub fn new(
        flash: FlashMedia<B>,
        config: PFlashCfi02Config,
    ) -> Result<Self, StorageError> {
        Self::new_named("pflash-cfi02", flash, config)
    }

    pub fn new_named(
        local_id: &str,
        flash: FlashMedia<B>,
        mut config: PFlashCfi02Config,
    ) -> Result<Self, StorageError> {
        validate_common(config.sector_len, config.num_blocks)?;
        if config.width == 0
            || config.width > 4
            || !config.width.is_power_of_two()
        {
            return Err(StorageError::InvalidInput(
                "width must be a non-zero power of two no larger than 4"
                    .to_string(),
            ));
        }
        config.unlock_addr0 &= 0x07ff;
        config.unlock_addr1 &= 0x07ff;
        let chip_len = config
            .sector_len
            .checked_mul(u64::from(config.num_blocks))
            .ok_or(StorageError::Overflow)?;
        let cfi_table = cfi02_table(&config, chip_len);
        Ok(Self {
            state: Mutex::new(SysBusDeviceState::new(local_id)),
            flash: flash.with_readonly(config.read_only),
            config,
            chip_len,
            regs: Mutex::new(PFlashCfi02Regs {
                wcycle: 0,
                cmd: 0,
                status: 0,
                bypass: false,
                erase_suspended: false,
                erasing_ranges: Vec::new(),
                cfi_table,
            }),
        })
    }

    pub fn attach_to_bus(&self, bus: &mut SysBus) -> Result<(), SysBusError> {
        lock(&self.state).attach_to_bus(bus)
    }

    pub fn register_mmio(
        &self,
        region: MemoryRegion,
        base: GPA,
    ) -> Result<(), SysBusError> {
        lock(&self.state).register_mmio(region, base)
    }

    pub fn realize_onto(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        lock(&self.state).realize_onto(bus, address_space)
    }

    pub fn unrealize_from(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        lock(&self.state).unrealize_from(bus, address_space)
    }

    pub fn realized(&self) -> bool {
        lock(&self.state).device().is_realized()
    }

    pub fn with_mdevice<T>(&self, f: impl FnOnce(&dyn MDevice) -> T) -> T {
        let guard = lock(&self.state);
        f(&*guard)
    }

    pub fn object_info(&self) -> MObjectInfo {
        lock(&self.state).object_info()
    }

    pub fn reset_runtime(&self) {
        let mut regs = lock(&self.regs);
        Self::cfi02_reset(&mut regs);
        regs.erase_suspended = false;
        regs.erasing_ranges.clear();
        regs.status = 0;
    }

    pub fn expire_timer(&self) {
        let mut regs = lock(&self.regs);
        if regs.cmd == 0x10 || regs.cmd == 0x30 {
            regs.status ^= 0x80;
            regs.erase_suspended = false;
            regs.erasing_ranges.clear();
            Self::cfi02_reset(&mut regs);
        }
    }

    pub fn do_read(&self, offset: u64, size: u32) -> u64 {
        if size == 0 || size > 4 {
            return 0;
        }
        let offset = self.wrap_offset(offset);
        let mut regs = lock(&self.regs);
        if regs.erase_suspended && self.cfi02_offset_is_erasing(&regs, offset) {
            regs.status ^= 0x04;
            return u64::from(regs.status);
        }
        match regs.cmd {
            0x00 | 0x80 => self.read_data(offset, size),
            0x90 => self.read_cfi02_id(offset, size),
            0x98 => self.read_cfi02_query(&regs, offset, size),
            0x10 => {
                regs.status ^= 0x40;
                u64::from(regs.status)
            }
            0x30 => {
                regs.status ^= 0x44;
                u64::from(regs.status)
            }
            0xa0 => {
                regs.status ^= 0x40;
                u64::from(regs.status)
            }
            _ => {
                Self::cfi02_reset(&mut regs);
                self.read_data(offset, size)
            }
        }
    }

    pub fn do_write(&self, offset: u64, size: u32, value: u64) {
        if size == 0 || size > 4 {
            return;
        }
        let cmd = value as u8;
        let offset = self.wrap_offset(offset);
        let boff = self.command_offset(offset);
        let mut regs = lock(&self.regs);
        if regs.cmd != 0xa0
            && cmd == 0xf0
            && regs.cmd != 0x10
            && regs.cmd != 0x30
        {
            if regs.wcycle == CFI02_WCYCLE_AUTOSELECT_CFI {
                regs.wcycle = 3;
                regs.cmd = 0x90;
            } else {
                regs.bypass = false;
                Self::cfi02_reset(&mut regs);
            }
            return;
        }

        match regs.wcycle {
            0 => self.write_cfi02_cycle0(&mut regs, boff, cmd),
            1 => self.write_cfi02_cycle1(&mut regs, boff, cmd),
            2 => self.write_cfi02_cycle2(&mut regs, boff, cmd),
            3 => self
                .write_cfi02_cycle3(&mut regs, offset, boff, size, value, cmd),
            4 => self.write_cfi02_cycle4(&mut regs, boff, cmd),
            5 => self.write_cfi02_cycle5(&mut regs, offset, boff, cmd),
            6 => self.write_cfi02_cycle6(&mut regs, offset, cmd),
            CFI02_WCYCLE_CFI | CFI02_WCYCLE_AUTOSELECT_CFI => {
                regs.bypass = false;
                Self::cfi02_reset(&mut regs);
            }
            _ => {
                regs.bypass = false;
                Self::cfi02_reset(&mut regs);
            }
        }
    }

    fn write_cfi02_cycle0(
        &self,
        regs: &mut PFlashCfi02Regs,
        boff: u64,
        cmd: u8,
    ) {
        if boff == 0x55 && cmd == 0x98 {
            regs.wcycle = CFI02_WCYCLE_CFI;
            regs.cmd = 0x98;
            return;
        }
        if cmd == 0x30 {
            if regs.erase_suspended {
                regs.erase_suspended = false;
                regs.status = (regs.status & 0x7f) | 0x08;
                regs.wcycle = 6;
                regs.cmd = 0x30;
            } else {
                regs.bypass = false;
                Self::cfi02_reset(regs);
            }
            return;
        }
        if cmd == 0xb0 {
            return;
        }
        if boff == u64::from(self.config.unlock_addr0) && cmd == 0xaa {
            regs.wcycle = 1;
        } else {
            regs.bypass = false;
            Self::cfi02_reset(regs);
        }
    }

    fn write_cfi02_cycle1(
        &self,
        regs: &mut PFlashCfi02Regs,
        boff: u64,
        cmd: u8,
    ) {
        if boff == u64::from(self.config.unlock_addr1) && cmd == 0x55 {
            regs.wcycle = 2;
        } else {
            regs.bypass = false;
            Self::cfi02_reset(regs);
        }
    }

    fn write_cfi02_cycle2(
        &self,
        regs: &mut PFlashCfi02Regs,
        boff: u64,
        cmd: u8,
    ) {
        if !regs.bypass && boff != u64::from(self.config.unlock_addr0) {
            regs.bypass = false;
            Self::cfi02_reset(regs);
            return;
        }
        match cmd {
            0x20 => {
                regs.bypass = true;
                regs.wcycle = 2;
                regs.cmd = 0;
            }
            0x80 | 0x90 | 0xa0 => {
                regs.cmd = cmd;
                regs.wcycle = 3;
            }
            _ => {
                regs.bypass = false;
                Self::cfi02_reset(regs);
            }
        }
    }

    fn write_cfi02_cycle3(
        &self,
        regs: &mut PFlashCfi02Regs,
        offset: u64,
        boff: u64,
        size: u32,
        value: u64,
        cmd: u8,
    ) {
        match regs.cmd {
            0x80 => {
                if boff == u64::from(self.config.unlock_addr0) && cmd == 0xaa {
                    regs.wcycle = 4;
                } else {
                    regs.bypass = false;
                    Self::cfi02_reset(regs);
                }
            }
            0xa0 => {
                if regs.erase_suspended
                    && self.cfi02_offset_is_erasing(regs, offset)
                {
                    if regs.bypass {
                        regs.wcycle = 2;
                        regs.cmd = 0;
                    } else {
                        regs.bypass = false;
                        Self::cfi02_reset(regs);
                    }
                    return;
                }
                if !self.config.read_only {
                    let bytes =
                        value_to_bytes(value, size, self.config.big_endian);
                    let _ = self.flash.program(offset, &bytes);
                }
                regs.status = (regs.status & 0x7f) | ((!cmd) & 0x80);
                if regs.bypass {
                    regs.wcycle = 2;
                    regs.cmd = 0;
                } else {
                    Self::cfi02_reset(regs);
                }
            }
            0x90 => {
                if boff == 0x55 && cmd == 0x98 {
                    regs.wcycle = CFI02_WCYCLE_AUTOSELECT_CFI;
                    regs.cmd = 0x98;
                } else {
                    regs.bypass = false;
                    Self::cfi02_reset(regs);
                }
            }
            _ => {
                regs.bypass = false;
                Self::cfi02_reset(regs);
            }
        }
    }

    fn write_cfi02_cycle4(
        &self,
        regs: &mut PFlashCfi02Regs,
        boff: u64,
        cmd: u8,
    ) {
        if regs.cmd == 0x80
            && boff == u64::from(self.config.unlock_addr1)
            && cmd == 0x55
        {
            regs.wcycle = 5;
        } else {
            regs.bypass = false;
            Self::cfi02_reset(regs);
        }
    }

    fn write_cfi02_cycle5(
        &self,
        regs: &mut PFlashCfi02Regs,
        offset: u64,
        boff: u64,
        cmd: u8,
    ) {
        if regs.erase_suspended {
            regs.bypass = false;
            Self::cfi02_reset(regs);
            return;
        }
        match cmd {
            0x10 if boff == u64::from(self.config.unlock_addr0) => {
                let _ = self.flash.erase_all();
                regs.status &= 0x7f;
                regs.cmd = cmd;
                regs.wcycle = 6;
            }
            0x30 => {
                self.begin_cfi02_sector_erase(regs, offset);
            }
            _ => {
                regs.bypass = false;
                Self::cfi02_reset(regs);
            }
        }
    }

    fn write_cfi02_cycle6(
        &self,
        regs: &mut PFlashCfi02Regs,
        offset: u64,
        cmd: u8,
    ) {
        match regs.cmd {
            0x10 => {}
            0x30 => match cmd {
                0xb0 => {
                    regs.erase_suspended = true;
                    regs.status &= !0x08;
                    Self::cfi02_reset(regs);
                }
                0x30 if regs.status & 0x08 == 0 => {
                    self.begin_cfi02_sector_erase(regs, offset)
                }
                _ if regs.status & 0x08 == 0 => {
                    regs.bypass = false;
                    Self::cfi02_reset(regs);
                }
                _ => {}
            },
            _ => {
                regs.bypass = false;
                Self::cfi02_reset(regs);
            }
        }
    }

    fn cfi02_reset(regs: &mut PFlashCfi02Regs) {
        regs.cmd = 0;
        regs.wcycle = 0;
        regs.bypass = false;
    }

    fn begin_cfi02_sector_erase(
        &self,
        regs: &mut PFlashCfi02Regs,
        offset: u64,
    ) {
        let base = align_down(offset, self.config.sector_len);
        let _ = self.flash.erase(base, self.config.sector_len);
        regs.status &= 0x77;
        regs.cmd = 0x30;
        regs.wcycle = 6;
        regs.erase_suspended = false;
        let end = base.saturating_add(self.config.sector_len);
        if !regs
            .erasing_ranges
            .iter()
            .any(|&(start, stop)| start == base && stop == end)
        {
            regs.erasing_ranges.push((base, end));
        }
    }

    fn cfi02_offset_is_erasing(
        &self,
        regs: &PFlashCfi02Regs,
        offset: u64,
    ) -> bool {
        regs.erasing_ranges
            .iter()
            .any(|&(start, end)| offset >= start && offset < end)
    }

    fn read_data(&self, offset: u64, size: u32) -> u64 {
        let mut bytes = vec![0; size as usize];
        if self.flash.read(offset, &mut bytes).is_err() {
            return 0;
        }
        bytes_to_value(&bytes, self.config.big_endian)
    }

    fn read_cfi02_id(&self, offset: u64, size: u32) -> u64 {
        if self.config.width == 1 && size > 1 {
            let mut bytes = Vec::with_capacity(size as usize);
            for index in 0..size {
                bytes.push(
                    self.read_cfi02_id_x8_byte(offset + u64::from(index)),
                );
            }
            return bytes_to_value(&bytes, self.config.big_endian);
        }
        let mut boff = offset & 0xff;
        if self.config.width == 2 {
            boff >>= 1;
        } else if self.config.width == 4 {
            boff >>= 2;
        }
        let value = match boff {
            0x00 => u64::from(self.config.ident0),
            0x01 => u64::from(self.config.ident1),
            0x02 => 0,
            0x0e if self.config.ident2 as u8 != 0xff => {
                u64::from(self.config.ident2)
            }
            0x0f if self.config.ident3 as u8 != 0xff => {
                u64::from(self.config.ident3)
            }
            _ => self.read_data(offset, size),
        };
        mask_width(value, size)
    }

    fn read_cfi02_id_x8_byte(&self, offset: u64) -> u8 {
        match offset & 0xff {
            0x00 => self.config.ident0 as u8,
            0x01 => self.config.ident1 as u8,
            0x02 => 0,
            0x0e if self.config.ident2 as u8 != 0xff => {
                self.config.ident2 as u8
            }
            0x0f if self.config.ident3 as u8 != 0xff => {
                self.config.ident3 as u8
            }
            _ => {
                let mut byte = [0];
                if self.flash.read(offset, &mut byte).is_err() {
                    0
                } else {
                    byte[0]
                }
            }
        }
    }

    fn read_cfi02_query(
        &self,
        regs: &PFlashCfi02Regs,
        offset: u64,
        size: u32,
    ) -> u64 {
        if self.config.width == 1 && size > 1 {
            let mut bytes = Vec::with_capacity(size as usize);
            for index in 0..size {
                let boff = ((offset + u64::from(index)) & 0xff) as usize;
                bytes.push(regs.cfi_table.get(boff).copied().unwrap_or(0));
            }
            return bytes_to_value(&bytes, self.config.big_endian);
        }
        let mut boff = offset & 0xff;
        if self.config.width == 2 {
            boff >>= 1;
        } else if self.config.width == 4 {
            boff >>= 2;
        }
        let value = regs.cfi_table.get(boff as usize).copied().unwrap_or(0);
        mask_width(u64::from(value), size)
    }

    fn wrap_offset(&self, offset: u64) -> u64 {
        if self.chip_len == 0 {
            0
        } else {
            offset % self.chip_len
        }
    }

    fn command_offset(&self, offset: u64) -> u64 {
        let mut boff = offset;
        if self.config.width == 2 {
            boff >>= 1;
        } else if self.config.width == 4 {
            boff >>= 2;
        }
        boff & 0x7ff
    }
}

pub struct PFlashCfi02Mmio<B: BlockBackend>(pub Arc<PFlashCfi02<B>>);

impl<B: BlockBackend> MmioOps for PFlashCfi02Mmio<B> {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.do_read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.do_write(offset, size, val);
    }
}

fn validate_common(
    sector_len: u64,
    num_blocks: u32,
) -> Result<(), StorageError> {
    if sector_len == 0 {
        return Err(StorageError::InvalidInput(
            "sector_len must be non-zero".to_string(),
        ));
    }
    if num_blocks == 0 {
        return Err(StorageError::InvalidInput(
            "num_blocks must be non-zero".to_string(),
        ));
    }
    Ok(())
}

fn cfi01_table(config: &PFlashCfi01Config) -> [u8; CFI01_TABLE_LEN] {
    let mut table = [0; CFI01_TABLE_LEN];
    let num_devices = if config.device_width == 0 {
        1
    } else {
        u64::from(config.bank_width / config.device_width)
    };
    let blocks_per_device = u64::from(config.num_blocks);
    let sector_len_per_device = config.sector_len / num_devices;
    let device_len = sector_len_per_device * blocks_per_device;

    table[0x10] = b'Q';
    table[0x11] = b'R';
    table[0x12] = b'Y';
    table[0x13] = 0x01;
    table[0x15] = 0x31;
    table[0x1b] = 0x45;
    table[0x1c] = 0x55;
    table[0x1f] = 0x07;
    table[0x20] = 0x07;
    table[0x21] = 0x0a;
    table[0x23] = 0x04;
    table[0x24] = 0x04;
    table[0x25] = 0x04;
    table[0x27] = trailing_log2_u64(device_len);
    table[0x28] = 0x02;
    table[0x2a] = if config.bank_width == 1 { 0x08 } else { 0x0b };
    table[0x2c] = 0x01;
    let blocks_minus_one = blocks_per_device.saturating_sub(1);
    table[0x2d] = blocks_minus_one as u8;
    table[0x2e] = (blocks_minus_one >> 8) as u8;
    table[0x2f] = (sector_len_per_device >> 8) as u8;
    table[0x30] = (sector_len_per_device >> 16) as u8;
    table[0x31] = b'P';
    table[0x32] = b'R';
    table[0x33] = b'I';
    table[0x34] = b'1';
    table[0x35] = b'0';
    table
}

fn cfi01_writeblock_size(config: &PFlashCfi01Config) -> u64 {
    let num_devices = if config.device_width == 0 {
        1
    } else {
        u64::from(config.bank_width / config.device_width)
    };
    let base = if config.bank_width == 1 {
        1 << 8
    } else {
        1 << 11
    };
    base * num_devices
}

fn cfi02_table(
    config: &PFlashCfi02Config,
    chip_len: u64,
) -> [u8; CFI02_TABLE_LEN] {
    let mut table = [0; CFI02_TABLE_LEN];
    table[0x10] = b'Q';
    table[0x11] = b'R';
    table[0x12] = b'Y';
    table[0x13] = 0x02;
    table[0x15] = 0x40;
    table[0x1b] = 0x27;
    table[0x1c] = 0x36;
    table[0x1f] = 0x07;
    table[0x21] = 0x09;
    table[0x22] = 0x0c;
    table[0x23] = 0x01;
    table[0x25] = 0x0a;
    table[0x26] = 0x0d;
    table[0x27] = trailing_log2_u64(chip_len);
    table[0x28] = 0x02;
    table[0x2c] = 0x01;
    let blocks_minus_one = u64::from(config.num_blocks).saturating_sub(1);
    table[0x2d] = blocks_minus_one as u8;
    table[0x2e] = (blocks_minus_one >> 8) as u8;
    table[0x2f] = (config.sector_len >> 8) as u8;
    table[0x30] = (config.sector_len >> 16) as u8;
    table[0x40] = b'P';
    table[0x41] = b'R';
    table[0x42] = b'I';
    table[0x43] = b'1';
    table[0x44] = b'0';
    table[0x46] = 0x02;
    table
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn bytes_to_value(bytes: &[u8], big_endian: bool) -> u64 {
    let mut out = 0;
    if big_endian {
        for byte in bytes {
            out = (out << 8) | u64::from(*byte);
        }
    } else {
        for (shift, byte) in bytes.iter().enumerate() {
            out |= u64::from(*byte) << (shift * 8);
        }
    }
    out
}

fn value_to_bytes(value: u64, size: u32, big_endian: bool) -> Vec<u8> {
    let len = size as usize;
    let mut bytes = vec![0; len];
    for (index, byte) in bytes.iter_mut().enumerate() {
        let shift = if big_endian {
            (len - 1 - index) * 8
        } else {
            index * 8
        };
        *byte = (value >> shift) as u8;
    }
    bytes
}

fn mask_width(value: u64, size: u32) -> u64 {
    let bits = size.saturating_mul(8);
    if bits >= u64::BITS {
        value
    } else {
        value & ((1u64 << bits) - 1)
    }
}

fn replicate_status(status: u8, size: u32, device_width: u8) -> u64 {
    if device_width == 0 {
        let mut value = u64::from(status);
        if size > 2 {
            value |= u64::from(status) << 16;
        }
        return mask_width(value, size);
    }
    let stride = usize::from(device_width.max(1));
    let mut bytes = vec![0; size as usize];
    for chunk in bytes.chunks_mut(stride) {
        chunk[0] = status;
    }
    bytes_to_value(&bytes, false)
}

fn align_down(offset: u64, len: u64) -> u64 {
    (offset / len) * len
}

fn trailing_log2(value: u8) -> u32 {
    value.trailing_zeros()
}

fn trailing_log2_u64(value: u64) -> u8 {
    value.trailing_zeros() as u8
}
