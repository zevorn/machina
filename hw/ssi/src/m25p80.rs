use machina_core::device_cell::DeviceRegs;
use machina_hw_core::mdev::MDeviceState;
use machina_hw_storage::{BlockBackend, FlashMedia};
use parking_lot::Mutex;

use crate::{SpiCsPolarity, SpiSlave};

const CMD_NOP: u8 = 0x00;
const CMD_PAGE_PROGRAM: u8 = 0x02;
const CMD_WRITE_STATUS: u8 = 0x01;
const CMD_READ: u8 = 0x03;
const CMD_WRITE_DISABLE: u8 = 0x04;
const CMD_READ_STATUS: u8 = 0x05;
const CMD_WRITE_ENABLE: u8 = 0x06;
const CMD_FAST_READ: u8 = 0x0b;
const CMD_FAST_READ4: u8 = 0x0c;
const CMD_PAGE_PROGRAM4: u8 = 0x12;
const CMD_READ4: u8 = 0x13;
const CMD_READ_CONFIG: u8 = 0x15;
const CMD_BANK_REGISTER_READ: u8 = 0x16;
const CMD_BANK_REGISTER_WRITE: u8 = 0x17;
const CMD_WRITE_STATUS2: u8 = 0x31;
const CMD_READ_CONTROL_OR_ENABLE_QPI: u8 = 0x35;
const CMD_DUAL_OUTPUT_READ: u8 = 0x3b;
const CMD_DUAL_OUTPUT_READ4: u8 = 0x3c;
const CMD_DUAL_IO_READ: u8 = 0xbb;
const CMD_DUAL_IO_READ4: u8 = 0xbc;
const CMD_QUAD_PAGE_PROGRAM: u8 = 0x32;
const CMD_QUAD_PAGE_PROGRAM4: u8 = 0x34;
const CMD_PAGE_PROGRAM4_ALIAS: u8 = 0x3e;
const CMD_READ_FLAG_STATUS: u8 = 0x70;
const CMD_WRITE_ENHANCED_VOLATILE_CONFIG: u8 = 0x61;
const CMD_READ_ENHANCED_VOLATILE_CONFIG: u8 = 0x65;
const CMD_QUAD_OUTPUT_READ: u8 = 0x6b;
const CMD_QUAD_OUTPUT_READ4: u8 = 0x6c;
const CMD_QUAD_IO_READ: u8 = 0xeb;
const CMD_QUAD_IO_READ4: u8 = 0xec;
const CMD_WRITE_VOLATILE_CONFIG: u8 = 0x81;
const CMD_READ_VOLATILE_CONFIG: u8 = 0x85;
const CMD_READ_ID_90: u8 = 0x90;
const CMD_JEDEC_ID: u8 = 0x9f;
const CMD_DUAL_PAGE_PROGRAM: u8 = 0xa2;
const CMD_READ_ID_AB: u8 = 0xab;
const CMD_AAI_WORD_PROGRAM: u8 = 0xad;
const CMD_WRITE_NONVOLATILE_CONFIG: u8 = 0xb1;
const CMD_BULK_ERASE_60: u8 = 0x60;
const CMD_BULK_ERASE_C7: u8 = 0xc7;
const CMD_DIE_ERASE: u8 = 0xc4;
const CMD_RESET_ENABLE: u8 = 0x66;
const CMD_RESET_MEMORY: u8 = 0x99;
const CMD_ENTER_4BYTE_ADDR: u8 = 0xb7;
const CMD_READ_NONVOLATILE_CONFIG: u8 = 0xb5;
const CMD_EXTEND_ADDR_WRITE: u8 = 0xc5;
const CMD_EXTEND_ADDR_READ: u8 = 0xc8;
const CMD_ERASE_4K: u8 = 0x20;
const CMD_ERASE4_4K: u8 = 0x21;
const CMD_ERASE_32K: u8 = 0x52;
const CMD_ERASE4_32K: u8 = 0x5c;
const CMD_ERASE_SECTOR: u8 = 0xd8;
const CMD_ERASE4_SECTOR: u8 = 0xdc;
const CMD_EXIT_4BYTE_ADDR: u8 = 0xe9;
const CMD_RESET_QIO: u8 = 0xf5;

const STATUS_WRITE_ENABLE: u8 = 1 << 1;
const STATUS_AAI_ENABLE: u8 = 1 << 6;
const STATUS_QUAD_ENABLE: u8 = 1 << 6;
const STATUS_TOP_BOTTOM: u8 = 1 << 5;
const STATUS_BP3: u8 = 1 << 6;
const STATUS_WRITABLE_BITS: u8 = 0x9c;
const STATUS2_QUAD_ENABLE: u8 = 1 << 1;
const CONFIG_4BYTE_ADDR: u8 = 1 << 5;
const FLAG_STATUS_READY: u8 = 1 << 7;
const FLAG_STATUS_4BYTE_ADDR: u8 = 1;
const JEDEC_RESPONSE_LEN: usize = 6;
const ERASE_4K_SIZE: u64 = 4 * 1024;
const ERASE_32K_SIZE: u64 = 32 * 1024;
const ERASE_SECTOR_SIZE: u64 = 64 * 1024;
const MAX_3BYTE_SIZE: u64 = 16 * 1024 * 1024;
const JEDEC_NUMONYX: u8 = 0x20;
const JEDEC_SPANSION: u8 = 0x01;
const JEDEC_MACRONIX: u8 = 0xc2;
const JEDEC_SST: u8 = 0xbf;
const JEDEC_ISSI: u8 = 0x9d;
const JEDEC_WINBOND: u8 = 0xef;
const DEFAULT_NONVOLATILE_CONFIG: u16 = 0x8fff;
const NVCFG_XIP_MODE_DISABLED: u16 = 7 << 9;
const NVCFG_XIP_MODE_MASK: u16 = 7 << 9;
const NVCFG_DUAL_IO_MASK: u16 = 1 << 2;
const NVCFG_QUAD_IO_MASK: u16 = 1 << 3;
const NVCFG_4BYTE_ADDR_MASK: u16 = 1;
const NVCFG_LOWER_SEGMENT_MASK: u16 = 1 << 1;
const SPANSION_DEFAULT_CR2V: u8 = 0x08;
const VCFG_DUMMY: u8 = 1;
const VCFG_WRAP_SEQUENTIAL: u8 = 1 << 1;
const VCFG_XIP_MODE_DISABLED: u8 = 1 << 3;
const EVCFG_OUT_DRIVER_STRENGTH_DEF: u8 = 7;
const EVCFG_VPP_ACCELERATOR: u8 = 1 << 3;
const EVCFG_RESET_HOLD_ENABLED: u8 = 1 << 4;
const EVCFG_DUAL_IO_DISABLED: u8 = 1 << 6;
const EVCFG_QUAD_IO_DISABLED: u8 = 1 << 7;

#[derive(Clone, Copy)]
enum AddressAction {
    Read,
    FastRead { dummy: usize },
    PageProgram,
    AaiProgram,
    ReadId,
    Erase { len: u64 },
    DieErase,
}

#[derive(Clone, Copy)]
struct AddressCommand {
    action: AddressAction,
    bytes: usize,
}

enum TransferState {
    Idle,
    IgnoreUntilCs,
    ReadStatus,
    ReadConfig,
    ReadControl,
    ReadFlagStatus,
    ReadExtendedAddress,
    CollectStatus {
        bytes: [u8; 2],
        len: usize,
        needed: usize,
        variable: bool,
    },
    CollectStatus2,
    ReadJedec {
        index: usize,
    },
    ReadId {
        index: usize,
        bytes: [u8; 2],
    },
    ReadNonvolatileConfig {
        index: usize,
    },
    ReadVolatileConfig,
    CollectVolatileConfig,
    ReadEnhancedVolatileConfig,
    CollectEnhancedVolatileConfig,
    CollectNonvolatileConfig {
        bytes: [u8; 2],
        len: usize,
    },
    CollectExtendedAddress,
    CollectAddress {
        command: AddressCommand,
        bytes: [u8; 4],
        len: usize,
    },
    ReadDummy {
        addr: u32,
        remaining: usize,
    },
    ReadData {
        addr: u32,
    },
    PageProgram {
        addr: u32,
        aai: bool,
    },
}

struct M25p80Regs {
    selected: bool,
    write_enable: bool,
    status_register: u8,
    reset_enable: bool,
    four_byte_address_mode: bool,
    extended_address: u8,
    quad_enable: bool,
    aai_enable: bool,
    aai_address: u32,
    volatile_config: u8,
    enhanced_volatile_config: u8,
    nonvolatile_config: u16,
    spansion_cr2v: u8,
    state: TransferState,
}

impl M25p80Regs {
    fn new(manufacturer: u8, flash_size: u64) -> Self {
        let mut regs = Self {
            selected: false,
            write_enable: false,
            status_register: 0,
            reset_enable: false,
            four_byte_address_mode: false,
            extended_address: 0,
            quad_enable: false,
            aai_enable: false,
            aai_address: 0,
            volatile_config: 0,
            enhanced_volatile_config: 0,
            nonvolatile_config: DEFAULT_NONVOLATILE_CONFIG,
            spansion_cr2v: 0,
            state: TransferState::Idle,
        };
        regs.reset_config_from_nonvolatile(manufacturer, flash_size);
        regs
    }

    fn reset_parser(&mut self) {
        self.state = TransferState::Idle;
    }

    fn reset_runtime(&mut self, manufacturer: u8, flash_size: u64) {
        self.write_enable = false;
        self.status_register = 0;
        self.reset_enable = false;
        self.four_byte_address_mode = false;
        self.extended_address = 0;
        self.quad_enable = false;
        self.aai_enable = false;
        self.aai_address = 0;
        self.reset_config_from_nonvolatile(manufacturer, flash_size);
        self.reset_parser();
    }

    fn reset_config_from_nonvolatile(
        &mut self,
        manufacturer: u8,
        flash_size: u64,
    ) {
        self.volatile_config = 0;
        self.enhanced_volatile_config = 0;
        self.spansion_cr2v = 0;

        match manufacturer {
            JEDEC_NUMONYX => {
                self.reset_numonyx_config(flash_size);
            }
            JEDEC_SPANSION => {
                self.spansion_cr2v = SPANSION_DEFAULT_CR2V;
            }
            JEDEC_MACRONIX => {
                self.volatile_config = 0x07;
            }
            _ => {}
        }
    }

    fn reset_numonyx_config(&mut self, flash_size: u64) {
        let cfg = self.nonvolatile_config;
        let mut volatile = VCFG_DUMMY | VCFG_WRAP_SEQUENTIAL;
        if cfg & NVCFG_XIP_MODE_MASK == NVCFG_XIP_MODE_DISABLED {
            volatile |= VCFG_XIP_MODE_DISABLED;
        }
        volatile |= (((cfg >> 12) & 0x0f) as u8) << 4;
        self.volatile_config = volatile;

        let mut enhanced = EVCFG_OUT_DRIVER_STRENGTH_DEF
            | EVCFG_VPP_ACCELERATOR
            | EVCFG_RESET_HOLD_ENABLED;
        if cfg & NVCFG_DUAL_IO_MASK != 0 {
            enhanced |= EVCFG_DUAL_IO_DISABLED;
        }
        if cfg & NVCFG_QUAD_IO_MASK != 0 {
            enhanced |= EVCFG_QUAD_IO_DISABLED;
        }
        self.enhanced_volatile_config = enhanced;

        if cfg & NVCFG_4BYTE_ADDR_MASK == 0 {
            self.four_byte_address_mode = true;
        }
        if cfg & NVCFG_LOWER_SEGMENT_MASK == 0 {
            self.extended_address = (flash_size / MAX_3BYTE_SIZE)
                .saturating_sub(1)
                .min(u64::from(u8::MAX))
                as u8;
        }
    }

    fn status(&self, manufacturer: u8) -> u8 {
        self.status_register
            | if self.write_enable {
                STATUS_WRITE_ENABLE
            } else {
                0
            }
            | if matches!(manufacturer, JEDEC_MACRONIX | JEDEC_ISSI)
                && self.quad_enable
            {
                STATUS_QUAD_ENABLE
            } else {
                0
            }
            | if manufacturer == JEDEC_SST && self.aai_enable {
                STATUS_AAI_ENABLE
            } else {
                0
            }
    }

    fn flag_status(&self) -> u8 {
        FLAG_STATUS_READY
            | if self.four_byte_address_mode {
                FLAG_STATUS_4BYTE_ADDR
            } else {
                0
            }
    }

    fn config(&self) -> u8 {
        self.volatile_config
            | if self.four_byte_address_mode {
                CONFIG_4BYTE_ADDR
            } else {
                0
            }
    }
}

/// SPI NOR flash model for the m25p80-compatible command set.
#[derive(machina_hw_core::MDevice)]
#[mom(state = state, lock = "parking_lot")]
pub struct M25p80<B: BlockBackend> {
    state: Mutex<MDeviceState>,
    cs_index: u8,
    jedec_id: [u8; 3],
    flash: FlashMedia<B>,
    regs: DeviceRegs<M25p80Regs>,
}

impl<B: BlockBackend> M25p80<B> {
    #[must_use]
    pub fn new(cs_index: u8, flash: FlashMedia<B>, jedec_id: [u8; 3]) -> Self {
        Self::new_named("m25p80", cs_index, flash, jedec_id)
    }

    #[must_use]
    pub fn new_named(
        local_id: &str,
        cs_index: u8,
        flash: FlashMedia<B>,
        jedec_id: [u8; 3],
    ) -> Self {
        let flash_size = flash.backend().size();
        Self {
            state: Mutex::new(MDeviceState::new(local_id)),
            cs_index,
            jedec_id,
            flash,
            regs: DeviceRegs::new(M25p80Regs::new(jedec_id[0], flash_size)),
        }
    }

    pub fn reset_runtime(&self) {
        self.regs
            .lock()
            .reset_runtime(self.jedec_id[0], self.flash.backend().size());
    }

    fn flash_addr(&self, addr: u32) -> Option<u64> {
        let size = self.flash.backend().size();
        if size == 0 {
            None
        } else {
            Some(u64::from(addr) % size)
        }
    }

    fn has_sr_tb(&self) -> bool {
        self.jedec_id[0] == JEDEC_NUMONYX && self.jedec_id[2] == 0x19
    }

    fn has_sr_bp3(&self) -> bool {
        self.jedec_id[0] == JEDEC_NUMONYX && self.jedec_id[2] == 0x19
    }

    fn die_count(&self) -> Option<u64> {
        (self.jedec_id[0] == JEDEC_NUMONYX && self.jedec_id[2] == 0x21)
            .then_some(4)
    }

    fn status_writable_bits(&self) -> u8 {
        let mut bits = STATUS_WRITABLE_BITS;
        if self.has_sr_tb() {
            bits |= STATUS_TOP_BOTTOM;
        }
        if self.has_sr_bp3() {
            bits |= STATUS_BP3;
        }
        bits
    }

    fn read_byte(&self, addr: u32) -> u8 {
        let mut buf = [0u8; 1];
        let Some(offset) = self.flash_addr(addr) else {
            return 0;
        };
        if self.flash.read(offset, &mut buf).is_ok() {
            buf[0]
        } else {
            0
        }
    }

    fn program_block_protected(&self, regs: &M25p80Regs, addr: u32) -> bool {
        let mut protect_bits = (regs.status_register >> 2) & 0x07;
        if self.has_sr_bp3() && regs.status_register & STATUS_BP3 != 0 {
            protect_bits |= 0x08;
        }
        if protect_bits == 0 {
            return false;
        }

        let size = self.flash.backend().size();
        if size == 0 {
            return false;
        }
        let Some(offset) = self.flash_addr(addr) else {
            return false;
        };
        let sector_count = size.div_ceil(ERASE_SECTOR_SIZE);
        let sector = offset / ERASE_SECTOR_SIZE;
        let protected_sectors = 1u64 << u32::from(protect_bits - 1);

        if self.has_sr_tb() && regs.status_register & STATUS_TOP_BOTTOM != 0 {
            sector < protected_sectors
        } else {
            sector_count <= sector + protected_sectors
        }
    }

    fn program_byte(&self, regs: &M25p80Regs, addr: u32, value: u8) {
        if self.program_block_protected(regs, addr) {
            return;
        }
        if let Some(offset) = self.flash_addr(addr) {
            let _ = self.flash.program(offset, &[value]);
        }
    }

    fn erase_region(&self, addr: u32, len: u64) {
        let Some(offset) = self.flash_addr(addr) else {
            return;
        };
        let block = u64::from(self.flash.erase_block_size());
        let base = (offset / block) * block;
        let _ = self.flash.erase(base, len);
    }

    fn erase_all(&self) {
        let _ = self.flash.erase_all();
    }

    fn erase_die(&self, addr: u32) {
        let Some(die_count) = self.die_count() else {
            return;
        };
        let size = self.flash.backend().size();
        if size == 0 {
            return;
        }
        let Some(offset) = self.flash_addr(addr) else {
            return;
        };
        let len = size / die_count;
        if len == 0 {
            return;
        }
        let base = (offset / len) * len;
        let _ = self.flash.erase(base, len);
    }

    fn collect_command(action: AddressAction, bytes: usize) -> TransferState {
        TransferState::CollectAddress {
            command: AddressCommand { action, bytes },
            bytes: [0; 4],
            len: 0,
        }
    }

    fn address_bytes(regs: &M25p80Regs) -> usize {
        if regs.four_byte_address_mode {
            4
        } else {
            3
        }
    }

    fn numonyx_dummy_bytes(regs: &M25p80Regs, qio: bool) -> usize {
        let dummies = (regs.volatile_config >> 4) & 0x0f;
        if dummies == 0 || dummies == 0x0f {
            if qio
                || regs.enhanced_volatile_config & EVCFG_QUAD_IO_DISABLED == 0
            {
                10
            } else {
                8
            }
        } else {
            usize::from(dummies)
        }
    }

    fn numonyx_qio_mode(&self, regs: &M25p80Regs) -> bool {
        self.jedec_id[0] == JEDEC_NUMONYX
            && regs.enhanced_volatile_config & EVCFG_QUAD_IO_DISABLED == 0
    }

    fn numonyx_dio_mode(&self, regs: &M25p80Regs) -> bool {
        self.jedec_id[0] == JEDEC_NUMONYX
            && !self.numonyx_qio_mode(regs)
            && regs.enhanced_volatile_config & EVCFG_DUAL_IO_DISABLED == 0
    }

    fn numonyx_std_mode(&self, regs: &M25p80Regs) -> bool {
        self.jedec_id[0] != JEDEC_NUMONYX
            || (!self.numonyx_qio_mode(regs) && !self.numonyx_dio_mode(regs))
    }

    fn fast_read_dummy_bytes(&self, regs: &M25p80Regs) -> usize {
        match self.jedec_id[0] {
            JEDEC_NUMONYX => Self::numonyx_dummy_bytes(regs, false),
            JEDEC_SPANSION => usize::from(regs.spansion_cr2v & 0x0f),
            JEDEC_WINBOND => 8,
            JEDEC_SST | JEDEC_ISSI => 1,
            JEDEC_MACRONIX => {
                if (regs.volatile_config >> 6) & 0x03 == 1 {
                    6
                } else {
                    8
                }
            }
            _ => 0,
        }
    }

    fn io_read_dummy_bytes(&self, regs: &M25p80Regs, qio: bool) -> usize {
        match self.jedec_id[0] {
            JEDEC_NUMONYX => Self::numonyx_dummy_bytes(regs, qio),
            JEDEC_SPANSION => usize::from((regs.spansion_cr2v & 0x0f) + 1),
            JEDEC_WINBOND if qio => 5,
            JEDEC_WINBOND => 1,
            JEDEC_ISSI if qio => 3,
            JEDEC_ISSI => 1,
            JEDEC_MACRONIX => match (regs.volatile_config >> 6) & 0x03 {
                1 => {
                    if qio {
                        4
                    } else {
                        6
                    }
                }
                2 => 8,
                _ => {
                    if qio {
                        6
                    } else {
                        4
                    }
                }
            },
            _ => 0,
        }
    }

    fn read_after_dummy(addr: u32, dummy: usize) -> TransferState {
        if dummy == 0 {
            TransferState::ReadData { addr }
        } else {
            TransferState::ReadDummy {
                addr,
                remaining: dummy,
            }
        }
    }

    fn collect_status_state(&self) -> TransferState {
        match self.jedec_id[0] {
            JEDEC_MACRONIX | JEDEC_WINBOND => TransferState::CollectStatus {
                bytes: [0; 2],
                len: 0,
                needed: 2,
                variable: true,
            },
            JEDEC_SPANSION => TransferState::CollectStatus {
                bytes: [0; 2],
                len: 0,
                needed: 2,
                variable: false,
            },
            _ => TransferState::CollectStatus {
                bytes: [0; 2],
                len: 0,
                needed: 1,
                variable: false,
            },
        }
    }

    fn start_command(&self, regs: &mut M25p80Regs, value: u8) -> u8 {
        let reset_armed = regs.reset_enable;
        if value != CMD_RESET_MEMORY {
            regs.reset_enable = false;
        }

        match value {
            CMD_NOP => {
                regs.state = TransferState::Idle;
                0
            }
            CMD_WRITE_ENABLE => {
                regs.write_enable = true;
                regs.state = TransferState::IgnoreUntilCs;
                0
            }
            CMD_WRITE_STATUS => {
                regs.state = if regs.write_enable {
                    self.collect_status_state()
                } else {
                    TransferState::IgnoreUntilCs
                };
                0
            }
            CMD_WRITE_DISABLE => {
                regs.write_enable = false;
                if self.jedec_id[0] == JEDEC_SST {
                    regs.aai_enable = false;
                }
                regs.state = TransferState::IgnoreUntilCs;
                0
            }
            CMD_WRITE_STATUS2 => {
                regs.state =
                    if regs.write_enable && self.jedec_id[0] == JEDEC_WINBOND {
                        TransferState::CollectStatus2
                    } else {
                        TransferState::IgnoreUntilCs
                    };
                0
            }
            CMD_READ_CONTROL_OR_ENABLE_QPI => {
                match self.jedec_id[0] {
                    JEDEC_SPANSION | JEDEC_WINBOND => {
                        regs.state = TransferState::ReadControl;
                    }
                    JEDEC_MACRONIX => {
                        regs.quad_enable = true;
                        regs.state = TransferState::IgnoreUntilCs;
                    }
                    _ => regs.state = TransferState::IgnoreUntilCs,
                }
                0
            }
            CMD_RESET_QIO => {
                regs.quad_enable = false;
                regs.state = TransferState::IgnoreUntilCs;
                0
            }
            CMD_RESET_ENABLE => {
                regs.reset_enable = true;
                regs.state = TransferState::IgnoreUntilCs;
                0
            }
            CMD_RESET_MEMORY => {
                if reset_armed {
                    regs.reset_runtime(
                        self.jedec_id[0],
                        self.flash.backend().size(),
                    );
                }
                regs.state = TransferState::IgnoreUntilCs;
                0
            }
            CMD_ENTER_4BYTE_ADDR => {
                regs.four_byte_address_mode = true;
                regs.state = TransferState::IgnoreUntilCs;
                0
            }
            CMD_EXIT_4BYTE_ADDR => {
                regs.four_byte_address_mode = false;
                regs.state = TransferState::IgnoreUntilCs;
                0
            }
            CMD_READ_STATUS => {
                regs.state = TransferState::ReadStatus;
                0
            }
            CMD_READ_CONFIG => {
                regs.state = TransferState::ReadConfig;
                0
            }
            CMD_READ_FLAG_STATUS => {
                regs.state = TransferState::ReadFlagStatus;
                0
            }
            CMD_BANK_REGISTER_READ | CMD_EXTEND_ADDR_READ => {
                regs.state = TransferState::ReadExtendedAddress;
                0
            }
            CMD_BANK_REGISTER_WRITE | CMD_EXTEND_ADDR_WRITE => {
                regs.state = if regs.write_enable {
                    TransferState::CollectExtendedAddress
                } else {
                    TransferState::IgnoreUntilCs
                };
                0
            }
            CMD_JEDEC_ID => {
                regs.state = if self.numonyx_std_mode(regs) {
                    TransferState::ReadJedec { index: 0 }
                } else {
                    TransferState::Idle
                };
                0
            }
            CMD_READ_ID_90 | CMD_READ_ID_AB => {
                regs.state = Self::collect_command(
                    AddressAction::ReadId,
                    Self::address_bytes(regs),
                );
                0
            }
            CMD_READ_NONVOLATILE_CONFIG => {
                regs.state = TransferState::ReadNonvolatileConfig { index: 0 };
                0
            }
            CMD_READ_VOLATILE_CONFIG => {
                regs.state = TransferState::ReadVolatileConfig;
                0
            }
            CMD_READ_ENHANCED_VOLATILE_CONFIG => {
                regs.state = TransferState::ReadEnhancedVolatileConfig;
                0
            }
            CMD_WRITE_VOLATILE_CONFIG => {
                regs.state = if regs.write_enable {
                    TransferState::CollectVolatileConfig
                } else {
                    TransferState::IgnoreUntilCs
                };
                0
            }
            CMD_WRITE_ENHANCED_VOLATILE_CONFIG => {
                regs.state = if regs.write_enable {
                    TransferState::CollectEnhancedVolatileConfig
                } else {
                    TransferState::IgnoreUntilCs
                };
                0
            }
            CMD_WRITE_NONVOLATILE_CONFIG => {
                regs.state =
                    if regs.write_enable && self.jedec_id[0] == JEDEC_NUMONYX {
                        TransferState::CollectNonvolatileConfig {
                            bytes: [0; 2],
                            len: 0,
                        }
                    } else {
                        TransferState::IgnoreUntilCs
                    };
                0
            }
            CMD_READ => {
                regs.state = if self.numonyx_std_mode(regs) {
                    Self::collect_command(
                        AddressAction::Read,
                        Self::address_bytes(regs),
                    )
                } else {
                    TransferState::Idle
                };
                0
            }
            CMD_FAST_READ => {
                regs.state = Self::collect_command(
                    AddressAction::FastRead {
                        dummy: self.fast_read_dummy_bytes(regs),
                    },
                    Self::address_bytes(regs),
                );
                0
            }
            CMD_DUAL_OUTPUT_READ => {
                regs.state = if self.numonyx_qio_mode(regs) {
                    TransferState::Idle
                } else {
                    Self::collect_command(
                        AddressAction::FastRead {
                            dummy: self.fast_read_dummy_bytes(regs),
                        },
                        Self::address_bytes(regs),
                    )
                };
                0
            }
            CMD_QUAD_OUTPUT_READ => {
                regs.state = if self.numonyx_dio_mode(regs) {
                    TransferState::Idle
                } else {
                    Self::collect_command(
                        AddressAction::FastRead {
                            dummy: self.fast_read_dummy_bytes(regs),
                        },
                        Self::address_bytes(regs),
                    )
                };
                0
            }
            CMD_READ4 => {
                regs.state = if self.numonyx_std_mode(regs) {
                    Self::collect_command(AddressAction::Read, 4)
                } else {
                    TransferState::Idle
                };
                0
            }
            CMD_FAST_READ4 => {
                regs.state = Self::collect_command(
                    AddressAction::FastRead {
                        dummy: self.fast_read_dummy_bytes(regs),
                    },
                    4,
                );
                0
            }
            CMD_DUAL_OUTPUT_READ4 => {
                regs.state = if self.numonyx_qio_mode(regs) {
                    TransferState::Idle
                } else {
                    Self::collect_command(
                        AddressAction::FastRead {
                            dummy: self.fast_read_dummy_bytes(regs),
                        },
                        4,
                    )
                };
                0
            }
            CMD_QUAD_OUTPUT_READ4 => {
                regs.state = if self.numonyx_dio_mode(regs) {
                    TransferState::Idle
                } else {
                    Self::collect_command(
                        AddressAction::FastRead {
                            dummy: self.fast_read_dummy_bytes(regs),
                        },
                        4,
                    )
                };
                0
            }
            CMD_DUAL_IO_READ | CMD_QUAD_IO_READ => {
                let blocked = if value == CMD_QUAD_IO_READ {
                    self.numonyx_dio_mode(regs)
                } else {
                    self.numonyx_qio_mode(regs)
                };
                regs.state = if blocked {
                    TransferState::Idle
                } else {
                    Self::collect_command(
                        AddressAction::FastRead {
                            dummy: self.io_read_dummy_bytes(
                                regs,
                                value == CMD_QUAD_IO_READ,
                            ),
                        },
                        Self::address_bytes(regs),
                    )
                };
                0
            }
            CMD_DUAL_IO_READ4 | CMD_QUAD_IO_READ4 => {
                let blocked = if value == CMD_QUAD_IO_READ4 {
                    self.numonyx_dio_mode(regs)
                } else {
                    self.numonyx_qio_mode(regs)
                };
                regs.state = if blocked {
                    TransferState::Idle
                } else {
                    Self::collect_command(
                        AddressAction::FastRead {
                            dummy: self.io_read_dummy_bytes(
                                regs,
                                value == CMD_QUAD_IO_READ4,
                            ),
                        },
                        4,
                    )
                };
                0
            }
            CMD_PAGE_PROGRAM => {
                regs.state = Self::collect_command(
                    AddressAction::PageProgram,
                    Self::address_bytes(regs),
                );
                0
            }
            CMD_DUAL_PAGE_PROGRAM => {
                regs.state = if self.numonyx_qio_mode(regs) {
                    TransferState::Idle
                } else {
                    Self::collect_command(
                        AddressAction::PageProgram,
                        Self::address_bytes(regs),
                    )
                };
                0
            }
            CMD_QUAD_PAGE_PROGRAM => {
                regs.state = if self.numonyx_dio_mode(regs) {
                    TransferState::Idle
                } else {
                    Self::collect_command(
                        AddressAction::PageProgram,
                        Self::address_bytes(regs),
                    )
                };
                0
            }
            CMD_PAGE_PROGRAM4 => {
                regs.state =
                    Self::collect_command(AddressAction::PageProgram, 4);
                0
            }
            CMD_QUAD_PAGE_PROGRAM4 | CMD_PAGE_PROGRAM4_ALIAS => {
                regs.state = if self.numonyx_dio_mode(regs) {
                    TransferState::Idle
                } else {
                    Self::collect_command(AddressAction::PageProgram, 4)
                };
                0
            }
            CMD_AAI_WORD_PROGRAM => {
                regs.state =
                    if self.jedec_id[0] == JEDEC_SST && regs.write_enable {
                        if regs.aai_enable {
                            TransferState::PageProgram {
                                addr: regs.aai_address,
                                aai: true,
                            }
                        } else {
                            regs.aai_enable = true;
                            Self::collect_command(
                                AddressAction::AaiProgram,
                                Self::address_bytes(regs),
                            )
                        }
                    } else {
                        TransferState::IgnoreUntilCs
                    };
                0
            }
            CMD_ERASE_4K => {
                regs.state = Self::collect_command(
                    AddressAction::Erase { len: ERASE_4K_SIZE },
                    Self::address_bytes(regs),
                );
                0
            }
            CMD_ERASE4_4K => {
                regs.state = Self::collect_command(
                    AddressAction::Erase { len: ERASE_4K_SIZE },
                    4,
                );
                0
            }
            CMD_ERASE_32K => {
                regs.state = Self::collect_command(
                    AddressAction::Erase {
                        len: ERASE_32K_SIZE,
                    },
                    Self::address_bytes(regs),
                );
                0
            }
            CMD_ERASE4_32K => {
                regs.state = Self::collect_command(
                    AddressAction::Erase {
                        len: ERASE_32K_SIZE,
                    },
                    4,
                );
                0
            }
            CMD_ERASE_SECTOR => {
                regs.state = Self::collect_command(
                    AddressAction::Erase {
                        len: ERASE_SECTOR_SIZE,
                    },
                    Self::address_bytes(regs),
                );
                0
            }
            CMD_ERASE4_SECTOR => {
                regs.state = Self::collect_command(
                    AddressAction::Erase {
                        len: ERASE_SECTOR_SIZE,
                    },
                    4,
                );
                0
            }
            CMD_BULK_ERASE_60 | CMD_BULK_ERASE_C7 => {
                if regs.write_enable {
                    self.erase_all();
                }
                regs.state = TransferState::IgnoreUntilCs;
                0
            }
            CMD_DIE_ERASE => {
                regs.state = Self::collect_command(
                    AddressAction::DieErase,
                    Self::address_bytes(regs),
                );
                0
            }
            _ => {
                regs.state = TransferState::IgnoreUntilCs;
                0
            }
        }
    }

    fn collect_address(
        &self,
        regs: &mut M25p80Regs,
        command: AddressCommand,
        mut bytes: [u8; 4],
        mut len: usize,
        value: u8,
    ) -> u8 {
        bytes[len] = value;
        len += 1;
        if len < command.bytes {
            regs.state = TransferState::CollectAddress {
                command,
                bytes,
                len,
            };
            return 0;
        }

        let mut addr = if command.bytes == 3 {
            u32::from(regs.extended_address)
        } else {
            0
        };
        for byte in bytes.iter().take(command.bytes) {
            addr = (addr << 8) | u32::from(*byte);
        }
        regs.state = match command.action {
            AddressAction::Read => TransferState::ReadData { addr },
            AddressAction::FastRead { dummy } => {
                Self::read_after_dummy(addr, dummy)
            }
            AddressAction::PageProgram => {
                TransferState::PageProgram { addr, aai: false }
            }
            AddressAction::AaiProgram => {
                let addr = addr & !1;
                regs.aai_address = addr;
                TransferState::PageProgram { addr, aai: true }
            }
            AddressAction::ReadId => {
                self.read_id_bytes(addr)
                    .map_or(TransferState::Idle, |bytes| {
                        TransferState::ReadId { index: 0, bytes }
                    })
            }
            AddressAction::Erase { len } => {
                if regs.write_enable {
                    self.erase_region(addr, len);
                }
                TransferState::IgnoreUntilCs
            }
            AddressAction::DieErase => {
                if regs.write_enable {
                    self.erase_die(addr);
                }
                TransferState::IgnoreUntilCs
            }
        };
        0
    }

    fn read_id_bytes(&self, addr: u32) -> Option<[u8; 2]> {
        if self.jedec_id[0] != JEDEC_SST {
            return None;
        }
        match self.flash_addr(addr)? {
            0 => Some([self.jedec_id[0], self.jedec_id[2]]),
            1 => Some([self.jedec_id[2], self.jedec_id[0]]),
            _ => None,
        }
    }

    fn finish_status_write(
        &self,
        regs: &mut M25p80Regs,
        bytes: [u8; 2],
        len: usize,
    ) {
        regs.status_register = bytes[0] & self.status_writable_bits();
        match self.jedec_id[0] {
            JEDEC_SPANSION => {
                regs.quad_enable = bytes[1] & STATUS2_QUAD_ENABLE != 0;
            }
            JEDEC_MACRONIX => {
                regs.quad_enable = bytes[0] & STATUS_QUAD_ENABLE != 0;
                if len > 1 {
                    regs.volatile_config = bytes[1];
                    regs.four_byte_address_mode =
                        bytes[1] & CONFIG_4BYTE_ADDR != 0;
                }
            }
            JEDEC_WINBOND if len > 1 => {
                regs.quad_enable = bytes[1] & STATUS2_QUAD_ENABLE != 0;
            }
            JEDEC_ISSI => {
                regs.quad_enable = bytes[0] & STATUS_QUAD_ENABLE != 0;
            }
            _ => {}
        }
        regs.write_enable = false;
        regs.state = TransferState::IgnoreUntilCs;
    }

    fn collect_status(
        &self,
        regs: &mut M25p80Regs,
        mut bytes: [u8; 2],
        mut len: usize,
        needed: usize,
        variable: bool,
        value: u8,
    ) -> u8 {
        if len < bytes.len() {
            bytes[len] = value;
            len += 1;
        }
        if len < needed {
            regs.state = TransferState::CollectStatus {
                bytes,
                len,
                needed,
                variable,
            };
            return 0;
        }

        self.finish_status_write(regs, bytes, len);
        0
    }

    fn collect_nonvolatile_config(
        regs: &mut M25p80Regs,
        mut bytes: [u8; 2],
        mut len: usize,
        value: u8,
    ) -> u8 {
        bytes[len] = value;
        len += 1;
        if len < bytes.len() {
            regs.state = TransferState::CollectNonvolatileConfig { bytes, len };
            return 0;
        }

        regs.nonvolatile_config = u16::from_le_bytes(bytes);
        regs.state = TransferState::IgnoreUntilCs;
        0
    }
}

impl<B: BlockBackend> SpiSlave for M25p80<B> {
    fn transfer(&self, val: u32) -> u32 {
        let value = val as u8;
        let mut regs = self.regs.lock();
        if !regs.selected {
            return 0xff;
        }

        match std::mem::replace(&mut regs.state, TransferState::Idle) {
            TransferState::Idle => {
                u32::from(self.start_command(&mut regs, value))
            }
            TransferState::IgnoreUntilCs => {
                regs.state = TransferState::IgnoreUntilCs;
                0
            }
            TransferState::ReadStatus => {
                regs.state = TransferState::ReadStatus;
                u32::from(regs.status(self.jedec_id[0]))
            }
            TransferState::ReadConfig => {
                regs.state = TransferState::Idle;
                u32::from(regs.config())
            }
            TransferState::ReadControl => {
                regs.state = TransferState::Idle;
                u32::from(if regs.quad_enable {
                    STATUS2_QUAD_ENABLE
                } else {
                    0
                })
            }
            TransferState::CollectStatus {
                bytes,
                len,
                needed,
                variable,
            } => u32::from(self.collect_status(
                &mut regs, bytes, len, needed, variable, value,
            )),
            TransferState::CollectStatus2 => {
                regs.quad_enable = value & STATUS2_QUAD_ENABLE != 0;
                regs.state = TransferState::IgnoreUntilCs;
                0
            }
            TransferState::ReadFlagStatus => {
                regs.state = TransferState::ReadFlagStatus;
                u32::from(regs.flag_status())
            }
            TransferState::ReadExtendedAddress => {
                regs.state = TransferState::Idle;
                u32::from(regs.extended_address)
            }
            TransferState::ReadJedec { index } => {
                let out = self.jedec_id.get(index).copied().unwrap_or(0);
                if index + 1 < JEDEC_RESPONSE_LEN {
                    regs.state = TransferState::ReadJedec { index: index + 1 };
                }
                u32::from(out)
            }
            TransferState::ReadId { index, bytes } => {
                let out = bytes[index % bytes.len()];
                regs.state = TransferState::ReadId {
                    index: index + 1,
                    bytes,
                };
                u32::from(out)
            }
            TransferState::ReadNonvolatileConfig { index } => {
                let bytes = regs.nonvolatile_config.to_le_bytes();
                let out = bytes.get(index).copied().unwrap_or(0);
                if index + 1 < bytes.len() {
                    regs.state = TransferState::ReadNonvolatileConfig {
                        index: index + 1,
                    };
                }
                u32::from(out)
            }
            TransferState::ReadVolatileConfig => {
                regs.state = TransferState::Idle;
                u32::from(regs.volatile_config)
            }
            TransferState::CollectVolatileConfig => {
                regs.volatile_config = value;
                regs.state = TransferState::IgnoreUntilCs;
                0
            }
            TransferState::ReadEnhancedVolatileConfig => {
                regs.state = TransferState::Idle;
                u32::from(regs.enhanced_volatile_config)
            }
            TransferState::CollectEnhancedVolatileConfig => {
                regs.enhanced_volatile_config = value;
                regs.state = TransferState::IgnoreUntilCs;
                0
            }
            TransferState::CollectNonvolatileConfig { bytes, len } => {
                u32::from(M25p80::<B>::collect_nonvolatile_config(
                    &mut regs, bytes, len, value,
                ))
            }
            TransferState::CollectAddress {
                command,
                bytes,
                len,
            } => u32::from(
                self.collect_address(&mut regs, command, bytes, len, value),
            ),
            TransferState::CollectExtendedAddress => {
                regs.extended_address = value;
                regs.state = TransferState::IgnoreUntilCs;
                0
            }
            TransferState::ReadDummy { addr, remaining } => {
                if remaining > 1 {
                    regs.state = TransferState::ReadDummy {
                        addr,
                        remaining: remaining - 1,
                    };
                } else {
                    regs.state = TransferState::ReadData { addr };
                }
                0
            }
            TransferState::ReadData { addr } => {
                let out = self.read_byte(addr);
                regs.state = TransferState::ReadData {
                    addr: addr.wrapping_add(1),
                };
                u32::from(out)
            }
            TransferState::PageProgram { addr, aai } => {
                if regs.write_enable {
                    self.program_byte(&regs, addr, value);
                }
                let next_addr = addr.wrapping_add(1);
                if aai {
                    regs.aai_address = next_addr;
                    if self.flash_addr(next_addr) == Some(0) {
                        regs.write_enable = false;
                        regs.aai_enable = false;
                    }
                }
                regs.state = TransferState::PageProgram {
                    addr: next_addr,
                    aai,
                };
                0
            }
        }
    }

    fn set_cs(&self, cs: bool) {
        let mut regs = self.regs.lock();
        let selected = !cs;
        if regs.selected && !selected {
            if let TransferState::CollectStatus {
                bytes,
                len,
                variable: true,
                ..
            } = regs.state
            {
                if len > 0 {
                    self.finish_status_write(&mut regs, bytes, len);
                }
            }
            regs.reset_parser();
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
