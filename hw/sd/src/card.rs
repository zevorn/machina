//! SD memory card model.
//!
//! This module implements the card side of the native SD protocol used by
//! host controllers and SPI bridges.  The controller-facing transport stays in
//! [`crate::SdBus`]; this type owns card state and block media.

use std::sync::Mutex;

use machina_core::device_cell::DeviceRegs;
use machina_hw_core::mdev::MDeviceState;
use machina_hw_storage::{BlockBackend, BlockMedia, StorageError};

use crate::{status, SdCard, SdRequest};

const OCR_VOLTAGE_WINDOW: u32 = 0x00ff_ff00;
const OCR_CARD_CAPACITY: u32 = 1 << 30;
const OCR_POWER_UP: u32 = 1 << 31;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SdCardConfig {
    pub inserted: bool,
    pub rca: u16,
}

impl Default for SdCardConfig {
    fn default() -> Self {
        Self {
            inserted: true,
            rca: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CardState {
    Idle,
    Ready,
    Identification,
    Standby,
    Transfer,
    SendingData,
    ReceivingData,
}

struct SdCardRegs {
    state: CardState,
    expecting_acmd: bool,
    ocr: u32,
    vhs: u32,
    block_len: usize,
    status_flags: u32,
    data_start_sector: u64,
    data_next_sector: u64,
    data_multi_block: bool,
    erase_start_sector: Option<u64>,
    erase_end_sector: Option<u64>,
    data: Vec<u8>,
    data_offset: usize,
}

#[derive(machina_hw_core::MDevice)]
#[mom(state = state, lock = "std")]
pub struct SdMemoryCard<B: BlockBackend> {
    state: Mutex<MDeviceState>,
    media: BlockMedia<B>,
    config: SdCardConfig,
    regs: DeviceRegs<SdCardRegs>,
}

impl<B: BlockBackend> SdMemoryCard<B> {
    pub fn new(
        media: BlockMedia<B>,
        config: SdCardConfig,
    ) -> Result<Self, StorageError> {
        Self::new_named("sd-card", media, config)
    }

    pub fn new_named(
        local_id: &str,
        media: BlockMedia<B>,
        config: SdCardConfig,
    ) -> Result<Self, StorageError> {
        let ocr = initial_ocr(media.sector_count());
        Ok(Self {
            state: Mutex::new(MDeviceState::new(local_id)),
            media,
            config,
            regs: DeviceRegs::new(SdCardRegs {
                state: CardState::Idle,
                expecting_acmd: false,
                ocr,
                vhs: 0,
                block_len: 512,
                status_flags: 0,
                data_start_sector: 0,
                data_next_sector: 0,
                data_multi_block: false,
                erase_start_sector: None,
                erase_end_sector: None,
                data: Vec::new(),
                data_offset: 0,
            }),
        })
    }

    pub fn reset_runtime(&self) {
        let mut regs = self.regs.lock();
        self.reset_regs(&mut regs);
    }

    fn reset_regs(&self, regs: &mut SdCardRegs) {
        regs.state = CardState::Idle;
        regs.expecting_acmd = false;
        regs.ocr = initial_ocr(self.media.sector_count());
        regs.vhs = 0;
        regs.block_len = 512;
        regs.status_flags = 0;
        regs.data_start_sector = 0;
        regs.data_next_sector = 0;
        regs.data_multi_block = false;
        regs.erase_start_sector = None;
        regs.erase_end_sector = None;
        regs.data.clear();
        regs.data_offset = 0;
    }

    fn read_sector_data(
        &self,
        sector: u64,
        block_len: usize,
    ) -> Result<Vec<u8>, StorageError> {
        let mut data = vec![0; self.media.block_size() as usize];
        self.media.read_block(sector, &mut data)?;
        data.truncate(block_len);
        Ok(data)
    }

    fn erase_sector_range(
        &self,
        start: u64,
        end: u64,
    ) -> Result<(), StorageError> {
        if start > end || end >= self.media.sector_count() {
            return Err(StorageError::OutOfRange);
        }
        let erased = vec![0; self.media.block_size() as usize];
        for sector in start..=end {
            self.media.write_block(sector, &erased)?;
        }
        Ok(())
    }

    fn write_received_block(&self, sector: u64, data: &[u8]) {
        let block_size = self.media.block_size() as usize;
        if data.len() == block_size {
            let _ = self.media.write_block(sector, data);
            return;
        }

        let mut block = vec![0; block_size];
        if self.media.read_block(sector, &mut block).is_err() {
            return;
        }
        let len = data.len().min(block.len());
        block[..len].copy_from_slice(&data[..len]);
        let _ = self.media.write_block(sector, &block);
    }
}

impl<B: BlockBackend> SdCard for SdMemoryCard<B> {
    fn do_command(&self, req: &SdRequest, resp: &mut [u8]) -> usize {
        let mut regs = self.regs.lock();
        if regs.expecting_acmd {
            regs.expecting_acmd = false;
            return match req.cmd {
                41 if regs.state == CardState::Idle => {
                    if regs.ocr & req.arg & OCR_VOLTAGE_WINDOW != 0 {
                        regs.ocr |= OCR_POWER_UP;
                        regs.state = CardState::Ready;
                    }
                    write_be32(resp, regs.ocr)
                }
                51 if regs.state == CardState::Transfer => {
                    regs.data = DEFAULT_SCR.to_vec();
                    regs.data_offset = 0;
                    regs.state = CardState::SendingData;
                    write_r1(resp, CardState::Transfer, false)
                }
                _ => 0,
            };
        }

        match req.cmd {
            0 => {
                self.reset_regs(&mut regs);
                0
            }
            2 if regs.state == CardState::Ready => {
                regs.state = CardState::Identification;
                write_bytes(resp, &DEFAULT_CID)
            }
            3 if matches!(
                regs.state,
                CardState::Identification | CardState::Standby
            ) =>
            {
                regs.state = CardState::Standby;
                write_r6(resp, self.config.rca)
            }
            9 if regs.state == CardState::Standby
                && req.arg >> 16 == u32::from(self.config.rca) =>
            {
                write_bytes(resp, &DEFAULT_CSD)
            }
            10 if regs.state == CardState::Standby
                && req.arg >> 16 == u32::from(self.config.rca) =>
            {
                write_bytes(resp, &DEFAULT_CID)
            }
            7 if regs.state == CardState::Standby
                && req.arg >> 16 == u32::from(self.config.rca) =>
            {
                regs.state = CardState::Transfer;
                write_r1(resp, CardState::Standby, false)
            }
            8 if regs.state == CardState::Idle => {
                if !valid_vhs(req.arg) {
                    return 0;
                }
                regs.vhs = req.arg;
                write_be32(resp, req.arg)
            }
            55 if req.arg == 0
                || req.arg >> 16 == u32::from(self.config.rca) =>
            {
                let state = regs.state;
                regs.expecting_acmd = true;
                write_r1(resp, state, true)
            }
            13 if matches!(
                regs.state,
                CardState::Standby | CardState::Transfer
            ) && req.arg >> 16 == u32::from(self.config.rca) =>
            {
                write_r1(resp, regs.state, false)
            }
            32 if regs.state == CardState::Transfer => {
                let Some(sector) = byte_address_to_sector(req.arg) else {
                    regs.status_flags = status::ADDRESS_ERROR;
                    return write_r1_flags(
                        resp,
                        CardState::Transfer,
                        false,
                        regs.status_flags,
                    );
                };
                if sector >= self.media.sector_count() {
                    regs.status_flags = status::ADDRESS_ERROR;
                    return write_r1_flags(
                        resp,
                        CardState::Transfer,
                        false,
                        regs.status_flags,
                    );
                }
                regs.status_flags = 0;
                regs.erase_start_sector = Some(sector);
                regs.erase_end_sector = None;
                write_r1(resp, CardState::Transfer, false)
            }
            33 if regs.state == CardState::Transfer => {
                let Some(sector) = byte_address_to_sector(req.arg) else {
                    regs.status_flags = status::ADDRESS_ERROR;
                    return write_r1_flags(
                        resp,
                        CardState::Transfer,
                        false,
                        regs.status_flags,
                    );
                };
                if sector >= self.media.sector_count() {
                    regs.status_flags = status::ADDRESS_ERROR;
                    return write_r1_flags(
                        resp,
                        CardState::Transfer,
                        false,
                        regs.status_flags,
                    );
                }
                regs.status_flags = 0;
                regs.erase_end_sector = Some(sector);
                write_r1(resp, CardState::Transfer, false)
            }
            38 if regs.state == CardState::Transfer => {
                let (Some(start), Some(end)) =
                    (regs.erase_start_sector, regs.erase_end_sector)
                else {
                    regs.status_flags = status::ERASE_SEQ_ERROR;
                    return write_r1_flags(
                        resp,
                        CardState::Transfer,
                        false,
                        regs.status_flags,
                    );
                };
                match self.erase_sector_range(start, end) {
                    Ok(()) => {
                        regs.status_flags = 0;
                        regs.erase_start_sector = None;
                        regs.erase_end_sector = None;
                    }
                    Err(StorageError::ReadOnly) => {
                        regs.status_flags = status::WP_VIOLATION;
                    }
                    Err(_) => {
                        regs.status_flags = status::ERASE_PARAM;
                    }
                }
                write_r1_flags(
                    resp,
                    CardState::Transfer,
                    false,
                    regs.status_flags,
                )
            }
            12 if matches!(
                regs.state,
                CardState::SendingData | CardState::ReceivingData
            ) =>
            {
                regs.data.clear();
                regs.data_offset = 0;
                regs.data_multi_block = false;
                regs.state = CardState::Transfer;
                write_r1(resp, CardState::Transfer, false)
            }
            16 if regs.state == CardState::Transfer => {
                if req.arg != 0 && req.arg <= 512 {
                    regs.block_len = req.arg as usize;
                    regs.status_flags = 0;
                } else {
                    regs.status_flags = status::BLOCK_LEN_ERROR;
                }
                write_r1_flags(
                    resp,
                    CardState::Transfer,
                    false,
                    regs.status_flags,
                )
            }
            17 if regs.state == CardState::Transfer => {
                let Some(sector) = byte_address_to_sector(req.arg) else {
                    regs.status_flags = status::ADDRESS_ERROR;
                    return write_r1_flags(
                        resp,
                        CardState::Transfer,
                        false,
                        regs.status_flags,
                    );
                };
                let Ok(data) = self.read_sector_data(sector, regs.block_len)
                else {
                    regs.status_flags = status::ADDRESS_ERROR;
                    return write_r1_flags(
                        resp,
                        CardState::Transfer,
                        false,
                        regs.status_flags,
                    );
                };
                regs.status_flags = 0;
                regs.data = data;
                regs.data_offset = 0;
                regs.data_next_sector = sector + 1;
                regs.data_multi_block = false;
                regs.state = CardState::SendingData;
                write_r1(resp, CardState::Transfer, false)
            }
            18 if regs.state == CardState::Transfer => {
                let Some(sector) = byte_address_to_sector(req.arg) else {
                    regs.status_flags = status::ADDRESS_ERROR;
                    return write_r1_flags(
                        resp,
                        CardState::Transfer,
                        false,
                        regs.status_flags,
                    );
                };
                let Ok(data) = self.read_sector_data(sector, regs.block_len)
                else {
                    regs.status_flags = status::ADDRESS_ERROR;
                    return write_r1_flags(
                        resp,
                        CardState::Transfer,
                        false,
                        regs.status_flags,
                    );
                };
                regs.status_flags = 0;
                regs.data = data;
                regs.data_offset = 0;
                regs.data_next_sector = sector + 1;
                regs.data_multi_block = true;
                regs.state = CardState::SendingData;
                write_r1(resp, CardState::Transfer, false)
            }
            24 if regs.state == CardState::Transfer => {
                let Some(sector) = byte_address_to_sector(req.arg) else {
                    regs.status_flags = status::ADDRESS_ERROR;
                    return write_r1_flags(
                        resp,
                        CardState::Transfer,
                        false,
                        regs.status_flags,
                    );
                };
                if self.media.backend().readonly() {
                    regs.status_flags = status::WP_VIOLATION;
                    return write_r1_flags(
                        resp,
                        CardState::Transfer,
                        false,
                        regs.status_flags,
                    );
                }
                regs.status_flags = 0;
                regs.data_start_sector = sector;
                regs.data_next_sector = sector + 1;
                regs.data_multi_block = false;
                regs.data = vec![0; regs.block_len];
                regs.data_offset = 0;
                regs.state = CardState::ReceivingData;
                write_r1(resp, CardState::Transfer, false)
            }
            25 if regs.state == CardState::Transfer => {
                let Some(sector) = byte_address_to_sector(req.arg) else {
                    regs.status_flags = status::ADDRESS_ERROR;
                    return write_r1_flags(
                        resp,
                        CardState::Transfer,
                        false,
                        regs.status_flags,
                    );
                };
                if self.media.backend().readonly() {
                    regs.status_flags = status::WP_VIOLATION;
                    return write_r1_flags(
                        resp,
                        CardState::Transfer,
                        false,
                        regs.status_flags,
                    );
                }
                regs.status_flags = 0;
                regs.data_start_sector = sector;
                regs.data_next_sector = sector + 1;
                regs.data_multi_block = true;
                regs.data = vec![0; regs.block_len];
                regs.data_offset = 0;
                regs.state = CardState::ReceivingData;
                write_r1(resp, CardState::Transfer, false)
            }
            _ => 0,
        }
    }

    fn write_byte(&self, value: u8) {
        let mut regs = self.regs.lock();
        if regs.state != CardState::ReceivingData
            || regs.data_offset >= regs.data.len()
        {
            return;
        }
        let offset = regs.data_offset;
        regs.data[offset] = value;
        regs.data_offset += 1;
        if regs.data_offset >= regs.data.len() {
            self.write_received_block(regs.data_start_sector, &regs.data);
            if regs.data_multi_block {
                regs.data_start_sector = regs.data_next_sector;
                regs.data_next_sector += 1;
                regs.data.fill(0);
                regs.data_offset = 0;
            } else {
                regs.state = CardState::Transfer;
            }
        }
    }

    fn read_byte(&self) -> u8 {
        let mut regs = self.regs.lock();
        if regs.data_offset >= regs.data.len() {
            return 0xff;
        }
        let value = regs.data[regs.data_offset];
        regs.data_offset += 1;
        if regs.data_offset >= regs.data.len() {
            if regs.data_multi_block {
                let sector = regs.data_next_sector;
                match self.read_sector_data(sector, regs.block_len) {
                    Ok(data) => {
                        regs.data = data;
                        regs.data_offset = 0;
                        regs.data_next_sector = sector + 1;
                    }
                    Err(_) => {
                        regs.status_flags = status::ADDRESS_ERROR;
                        regs.data.clear();
                        regs.data_offset = 0;
                        regs.data_multi_block = false;
                        regs.state = CardState::Transfer;
                    }
                }
            } else {
                regs.state = CardState::Transfer;
            }
        }
        value
    }

    fn receive_ready(&self) -> bool {
        let regs = self.regs.lock();
        regs.state == CardState::ReceivingData
            && regs.data_offset < regs.data.len()
    }

    fn data_ready(&self) -> bool {
        let regs = self.regs.lock();
        regs.state == CardState::SendingData
            && regs.data_offset < regs.data.len()
    }

    fn get_inserted(&self) -> bool {
        self.config.inserted
    }

    fn get_readonly(&self) -> bool {
        self.media.backend().readonly()
    }

    fn set_voltage(&self, _millivolts: u16) {}

    fn get_dat_lines(&self) -> u8 {
        0b1111
    }

    fn get_cmd_line(&self) -> bool {
        true
    }
}

fn valid_vhs(arg: u32) -> bool {
    (arg >> 8).count_ones() == 1
}

fn initial_ocr(sector_count: u64) -> u32 {
    let mut ocr = OCR_VOLTAGE_WINDOW;
    if sector_count > (2 * 1024 * 1024 * 1024u64 / 512) {
        ocr |= OCR_CARD_CAPACITY;
    }
    ocr
}

fn write_r1(resp: &mut [u8], state: CardState, app_cmd: bool) -> usize {
    write_r1_flags(resp, state, app_cmd, 0)
}

fn write_r1_flags(
    resp: &mut [u8],
    state: CardState,
    app_cmd: bool,
    flags: u32,
) -> usize {
    let mut status =
        flags | status::READY_FOR_DATA | (card_state_value(state) << 9);
    if app_cmd {
        status |= status::APP_CMD;
    }
    write_be32(resp, status)
}

fn card_state_value(state: CardState) -> u32 {
    match state {
        CardState::Idle => 0,
        CardState::Ready => 1,
        CardState::Identification => 2,
        CardState::Standby => 3,
        CardState::Transfer => 4,
        CardState::SendingData => 5,
        CardState::ReceivingData => 6,
    }
}

const DEFAULT_CID: [u8; 16] = [
    0x00, 0x4d, 0x41, 0x43, 0x48, 0x49, 0x4e, 0x41, 0x01, 0x00, 0x00, 0x00,
    0x00, 0x01, 0x00, 0xff,
];

const DEFAULT_CSD: [u8; 16] = [
    0x40, 0x0e, 0x00, 0x32, 0x5b, 0x59, 0x00, 0x00, 0x00, 0x01, 0x7f, 0x80,
    0x0a, 0x40, 0x00, 0xff,
];

const DEFAULT_SCR: [u8; 8] = [0x02, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

fn write_r6(resp: &mut [u8], rca: u16) -> usize {
    let status = status::READY_FOR_DATA as u16;
    write_be32(resp, (u32::from(rca) << 16) | u32::from(status))
}

fn write_bytes(resp: &mut [u8], bytes: &[u8]) -> usize {
    if resp.len() < bytes.len() {
        return 0;
    }
    resp[..bytes.len()].copy_from_slice(bytes);
    bytes.len()
}

fn byte_address_to_sector(address: u32) -> Option<u64> {
    if !address.is_multiple_of(512) {
        return None;
    }
    Some(u64::from(address / 512))
}

fn write_be32(resp: &mut [u8], value: u32) -> usize {
    if resp.len() < 4 {
        return 0;
    }
    resp[..4].copy_from_slice(&value.to_be_bytes());
    4
}
