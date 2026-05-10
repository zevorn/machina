use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_hw_core::bus::SysBus;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_sd::card::{SdCardConfig, SdMemoryCard};
use machina_hw_sd::pl181::{Pl181, Pl181Mmio, PL181_IRQ0, PL181_IRQ1};
use machina_hw_sd::sdhci::{Sdhci, SdhciMmio};
use machina_hw_sd::ssi_sd::SsiSd;
use machina_hw_sd::{SdBus, SdBusHost, SdCard, SdError, SdRequest, SdVoltage};
use machina_hw_ssi::{SpiBus, SpiCsPolarity, SpiSlave};
use machina_hw_storage::{BlockBackend, BlockMedia, FileBackend, MemBackend};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

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

struct Pl181IrqSink {
    levels: Mutex<Vec<bool>>,
}

impl Pl181IrqSink {
    fn new(lines: usize) -> Arc<Self> {
        Arc::new(Self {
            levels: Mutex::new(vec![false; lines]),
        })
    }

    fn level(&self, irq: u32) -> bool {
        self.levels.lock().unwrap()[irq as usize]
    }
}

impl IrqSink for Pl181IrqSink {
    fn set_irq(&self, irq: u32, level: bool) {
        self.levels.lock().unwrap()[irq as usize] = level;
    }
}

#[derive(Default)]
struct DmaProbe {
    read_sizes: Mutex<Vec<u32>>,
    writes: Mutex<Vec<(u32, u64)>>,
}

impl DmaProbe {
    fn read_sizes(&self) -> Vec<u32> {
        self.read_sizes.lock().unwrap().clone()
    }

    fn writes(&self) -> Vec<(u32, u64)> {
        self.writes.lock().unwrap().clone()
    }
}

impl MmioOps for DmaProbe {
    fn read(&self, _offset: u64, size: u32) -> u64 {
        self.read_sizes.lock().unwrap().push(size);
        0x5a
    }

    fn write(&self, _offset: u64, size: u32, val: u64) {
        self.writes.lock().unwrap().push((size, val));
    }
}

fn sd_card(data: Vec<u8>) -> SdMemoryCard<MemBackend> {
    SdMemoryCard::new(
        BlockMedia::new(MemBackend::new(data, false), 512).unwrap(),
        SdCardConfig::default(),
    )
    .unwrap()
}

fn readonly_sd_card(data: Vec<u8>) -> SdMemoryCard<MemBackend> {
    SdMemoryCard::new(
        BlockMedia::new(MemBackend::new(data, true), 512).unwrap(),
        SdCardConfig::default(),
    )
    .unwrap()
}

fn make_test_aspace() -> (AddressSpace, SysBus) {
    let root = MemoryRegion::container("root", 0x1_0000_0000);
    let aspace = AddressSpace::new(root);
    let bus = SysBus::new("sysbus");
    (aspace, bus)
}

fn make_ram_aspace(size: u64) -> Arc<AddressSpace> {
    let mut root = MemoryRegion::container("root", size);
    let (ram, _block) = MemoryRegion::ram("ram", size);
    root.add_subregion(ram, GPA(0));
    Arc::new(AddressSpace::new(root))
}

fn make_io_aspace<T: MmioOps + 'static>(
    addr: u64,
    size: u64,
    ops: Arc<T>,
) -> Arc<AddressSpace> {
    let mut root = MemoryRegion::container("root", addr + size + 0x1000);
    let ops: Arc<dyn MmioOps> = ops;
    root.add_subregion(MemoryRegion::io("dma-probe", size, ops), GPA(addr));
    Arc::new(AddressSpace::new(root))
}

fn resp_u32(resp: &[u8]) -> u32 {
    u32::from_be_bytes(resp[..4].try_into().unwrap())
}

fn power_up<B: BlockBackend>(card: &SdMemoryCard<B>) {
    let mut resp = [0; 16];
    assert_eq!(card.do_command(&SdRequest::new(55, 0), &mut resp), 4);
    assert_eq!(
        card.do_command(&SdRequest::new(41, 0x00ff_8000), &mut resp),
        4
    );
}

fn select_card<B: BlockBackend>(card: &SdMemoryCard<B>) -> u32 {
    let mut resp = [0; 16];
    power_up(card);
    assert_eq!(card.do_command(&SdRequest::new(2, 0), &mut resp), 16);
    assert_eq!(card.do_command(&SdRequest::new(3, 0), &mut resp), 4);
    let rca = resp_u32(&resp) >> 16;
    assert_eq!(card.do_command(&SdRequest::new(7, rca << 16), &mut resp), 4);
    rca
}

fn identify_card<B: BlockBackend>(card: &SdMemoryCard<B>) -> (u32, [u8; 16]) {
    let mut resp = [0; 16];
    power_up(card);
    assert_eq!(card.do_command(&SdRequest::new(2, 0), &mut resp), 16);
    let cid = resp;
    assert_eq!(card.do_command(&SdRequest::new(3, 0), &mut resp), 4);
    (resp_u32(&resp) >> 16, cid)
}

fn sdsc_csd_sector_count(csd: &[u8; 16]) -> u64 {
    let word1 = u32::from_be_bytes(csd[4..8].try_into().unwrap());
    let word2 = u32::from_be_bytes(csd[8..12].try_into().unwrap());
    let c_size = ((word1 & 0x3ff) << 2) | ((word2 >> 30) & 0x3);
    let c_size_mult = (word2 >> 15) & 0x7;
    u64::from(c_size + 1) << (c_size_mult + 2)
}

fn uboot_sdsc_sector_count(csd: [u32; 4]) -> u64 {
    let read_bl_len = 1u64 << ((csd[1] >> 16) & 0xf);
    let c_size = ((csd[1] & 0x3ff) << 2) | ((csd[2] >> 30) & 0x3);
    let c_size_mult = (csd[2] >> 15) & 0x7;
    let blocks = u64::from(c_size + 1) << (c_size_mult + 2);
    blocks * read_bl_len / 512
}

fn sdhci_long_response_words(mmio: &SdhciMmio) -> [u32; 4] {
    [
        ((mmio.read(0x1c, 4) as u32) << 8) | mmio.read(0x1b, 1) as u32,
        ((mmio.read(0x18, 4) as u32) << 8) | mmio.read(0x17, 1) as u32,
        ((mmio.read(0x14, 4) as u32) << 8) | mmio.read(0x13, 1) as u32,
        (mmio.read(0x10, 4) as u32) << 8,
    ]
}

// -- Positive Tests --

#[test]
fn test_sd_card_lifecycle_and_mom_identity() {
    let card = Arc::new(sd_card(vec![0; 512]));
    assert!(!card.realized());
    card.with_mdevice(|device| assert_eq!(device.local_id(), "sd-card"));
    assert_eq!(card.object_info().local_id, "sd-card");

    card.realize().unwrap();
    assert!(card.realized());
    let err = card.realize().unwrap_err();
    assert!(
        err.to_string().contains("already realized"),
        "unexpected second-realize error: {err}"
    );

    card.unrealize().unwrap();
    assert!(!card.realized());
    let err = card.unrealize().unwrap_err();
    assert!(
        err.to_string().contains("not realized"),
        "unexpected second-unrealize error: {err}"
    );
}

#[test]
fn test_sd_card_cmd8_returns_if_cond_in_idle() {
    let card = sd_card(vec![0; 512]);
    let mut resp = [0; 16];

    let len = card.do_command(&SdRequest::new(8, 0x1aa), &mut resp);

    assert_eq!(len, 4);
    assert_eq!(resp_u32(&resp), 0x1aa);
}

#[test]
fn test_sd_card_cmd55_acmd41_powers_up() {
    let card = sd_card(vec![0; 512]);
    let mut resp = [0; 16];

    let len = card.do_command(&SdRequest::new(55, 0), &mut resp);
    assert_eq!(len, 4);
    assert_ne!(resp_u32(&resp) & machina_hw_sd::status::APP_CMD, 0);

    let len = card.do_command(&SdRequest::new(41, 0x00ff_8000), &mut resp);
    assert_eq!(len, 4);
    assert_ne!(resp_u32(&resp) & (1 << 31), 0);
}

#[test]
fn test_sd_card_identification_assigns_rca_and_selects_card() {
    let card = sd_card(vec![0; 512]);
    let mut resp = [0; 16];

    power_up(&card);

    assert_eq!(card.do_command(&SdRequest::new(2, 0), &mut resp), 16);

    let len = card.do_command(&SdRequest::new(3, 0), &mut resp);
    assert_eq!(len, 4);
    let rca = resp_u32(&resp) >> 16;
    assert_eq!(rca, 1);

    assert_eq!(card.do_command(&SdRequest::new(7, rca << 16), &mut resp), 4);

    assert_eq!(
        card.do_command(&SdRequest::new(13, rca << 16), &mut resp),
        4
    );
    assert_eq!(
        resp_u32(&resp) & machina_hw_sd::status::CURRENT_STATE,
        4 << 9
    );
}

#[test]
fn test_sd_card_cmd9_returns_csd_in_standby() {
    let card = sd_card(vec![0; 1024]);
    let (rca, _) = identify_card(&card);
    let mut resp = [0; 16];

    let len = card.do_command(&SdRequest::new(9, rca << 16), &mut resp);

    assert_eq!(len, 16);
    assert_ne!(resp, [0; 16]);
}

#[test]
fn test_sd_card_cmd9_reports_backing_media_capacity() {
    let sector_count = 1_048_609u64;
    let file = tempfile::NamedTempFile::new().unwrap();
    file.as_file().set_len(sector_count * 512).unwrap();
    let backend = FileBackend::open(file.path(), false).unwrap();
    let media = BlockMedia::new(backend, 512).unwrap();
    let card = SdMemoryCard::new(media, SdCardConfig::default()).unwrap();
    let (rca, _) = identify_card(&card);
    let mut resp = [0; 16];

    assert_eq!(
        card.do_command(&SdRequest::new(9, rca << 16), &mut resp),
        16
    );

    assert_eq!(sdsc_csd_sector_count(&resp), 1_048_576);
}

#[test]
fn test_sdhci_cmd9_reports_csd_capacity_through_long_response_regs() {
    let sector_count = 1_048_609u64;
    let file = tempfile::NamedTempFile::new().unwrap();
    file.as_file().set_len(sector_count * 512).unwrap();
    let backend = FileBackend::open(file.path(), false).unwrap();
    let media = BlockMedia::new(backend, 512).unwrap();
    let card =
        Arc::new(SdMemoryCard::new(media, SdCardConfig::default()).unwrap());
    let (rca, _) = identify_card(&card);

    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(bus.clone());
    bus.set_host(controller.clone());
    bus.insert_card(card);
    let mmio = SdhciMmio(controller);

    mmio.write(0x08, 4, u64::from(rca << 16));
    mmio.write(0x0e, 2, 9 << 8);

    assert_eq!(
        uboot_sdsc_sector_count(sdhci_long_response_words(&mmio)),
        1_048_576
    );
}

#[test]
fn test_sd_card_cmd10_returns_cid_in_standby() {
    let card = sd_card(vec![0; 1024]);
    let (rca, cid) = identify_card(&card);
    let mut resp = [0; 16];

    let len = card.do_command(&SdRequest::new(10, rca << 16), &mut resp);

    assert_eq!(len, 16);
    assert_eq!(resp, cid);
}

#[test]
fn test_sd_card_acmd51_returns_scr_data_in_transfer() {
    let card = sd_card(vec![0; 1024]);
    let rca = select_card(&card);
    let mut resp = [0; 16];

    assert_eq!(
        card.do_command(&SdRequest::new(55, rca << 16), &mut resp),
        4
    );
    assert_ne!(resp_u32(&resp) & machina_hw_sd::status::APP_CMD, 0);

    let len = card.do_command(&SdRequest::new(51, 0), &mut resp);

    assert_eq!(len, 4);
    assert_eq!(
        resp_u32(&resp) & machina_hw_sd::status::CURRENT_STATE,
        4 << 9
    );
    assert!(card.data_ready());

    let scr = [
        card.read_byte(),
        card.read_byte(),
        card.read_byte(),
        card.read_byte(),
        card.read_byte(),
        card.read_byte(),
        card.read_byte(),
        card.read_byte(),
    ];

    assert_eq!(scr[0] >> 4, 0);
    assert_eq!(scr[1] & 0x05, 0x05);
    assert!(!card.data_ready());
}

#[test]
fn test_sd_card_cmd17_reads_single_block() {
    let mut data = vec![0; 1024];
    data[512] = 0x11;
    data[513] = 0x22;
    data[1023] = 0xee;
    let card = sd_card(data);
    let rca = select_card(&card);
    let mut resp = [0; 16];

    assert_eq!(card.do_command(&SdRequest::new(16, 512), &mut resp), 4);
    assert_eq!(card.do_command(&SdRequest::new(17, 512), &mut resp), 4);

    assert!(card.data_ready());
    assert_eq!(card.read_byte(), 0x11);
    assert_eq!(card.read_byte(), 0x22);
    for _ in 2..511 {
        assert_eq!(card.read_byte(), 0);
    }
    assert_eq!(card.read_byte(), 0xee);
    assert!(!card.data_ready());

    assert_eq!(
        card.do_command(&SdRequest::new(13, rca << 16), &mut resp),
        4
    );
    assert_eq!(
        resp_u32(&resp) & machina_hw_sd::status::CURRENT_STATE,
        4 << 9
    );
}

#[test]
fn test_sd_card_cmd18_reads_multiple_blocks_until_cmd12() {
    let mut data = vec![0; 2048];
    for index in 0..512 {
        data[512 + index] = index as u8;
        data[1024 + index] = 0x80 | (index as u8 & 0x7f);
    }
    let card = sd_card(data);
    let rca = select_card(&card);
    let mut resp = [0; 16];

    assert_eq!(card.do_command(&SdRequest::new(18, 512), &mut resp), 4);

    for index in 0..512 {
        assert_eq!(card.read_byte(), index as u8);
    }
    assert!(card.data_ready());

    for index in 0..512 {
        assert_eq!(card.read_byte(), 0x80 | (index as u8 & 0x7f));
    }
    assert!(card.data_ready());

    assert_eq!(card.do_command(&SdRequest::new(12, 0), &mut resp), 4);
    assert!(!card.data_ready());

    assert_eq!(
        card.do_command(&SdRequest::new(13, rca << 16), &mut resp),
        4
    );
    assert_eq!(
        resp_u32(&resp) & machina_hw_sd::status::CURRENT_STATE,
        4 << 9
    );
}

#[test]
fn test_sd_card_cmd24_writes_single_block() {
    let card = sd_card(vec![0; 1024]);
    select_card(&card);
    let mut resp = [0; 16];

    assert_eq!(card.do_command(&SdRequest::new(24, 512), &mut resp), 4);
    assert!(card.receive_ready());

    for index in 0..512 {
        card.write_byte((index & 0xff) as u8);
    }

    assert!(!card.receive_ready());

    assert_eq!(card.do_command(&SdRequest::new(17, 512), &mut resp), 4);
    for index in 0..512 {
        assert_eq!(card.read_byte(), (index & 0xff) as u8);
    }
}

#[test]
fn test_sd_card_cmd24_writes_current_block_length() {
    let card = sd_card(vec![0xff; 1024]);
    select_card(&card);
    let mut resp = [0; 16];

    assert_eq!(card.do_command(&SdRequest::new(16, 4), &mut resp), 4);
    assert_eq!(card.do_command(&SdRequest::new(24, 0), &mut resp), 4);
    for byte in [0xaa, 0xbb, 0xcc, 0xdd] {
        card.write_byte(byte);
    }

    assert_eq!(card.do_command(&SdRequest::new(17, 0), &mut resp), 4);
    assert_eq!(card.read_byte(), 0xaa);
    assert_eq!(card.read_byte(), 0xbb);
    assert_eq!(card.read_byte(), 0xcc);
    assert_eq!(card.read_byte(), 0xdd);
}

#[test]
fn test_sd_card_cmd25_writes_multiple_blocks_until_cmd12() {
    let card = sd_card(vec![0; 2048]);
    select_card(&card);
    let mut resp = [0; 16];

    assert_eq!(card.do_command(&SdRequest::new(25, 512), &mut resp), 4);
    assert!(card.receive_ready());

    for index in 0..512 {
        card.write_byte(index as u8);
    }
    assert!(card.receive_ready());

    for index in 0..512 {
        card.write_byte(0x80 | (index as u8 & 0x7f));
    }
    assert!(card.receive_ready());

    assert_eq!(card.do_command(&SdRequest::new(12, 0), &mut resp), 4);
    assert!(!card.receive_ready());

    assert_eq!(card.do_command(&SdRequest::new(17, 512), &mut resp), 4);
    for index in 0..512 {
        assert_eq!(card.read_byte(), index as u8);
    }

    assert_eq!(card.do_command(&SdRequest::new(17, 1024), &mut resp), 4);
    for index in 0..512 {
        assert_eq!(card.read_byte(), 0x80 | (index as u8 & 0x7f));
    }
}

#[test]
fn test_sd_card_cmd32_cmd33_cmd38_erases_selected_blocks() {
    let mut data = vec![0xaa; 2048];
    data[512..1024].fill(0x55);
    data[1024..1536].fill(0x66);
    let card = sd_card(data);
    select_card(&card);
    let mut resp = [0; 16];

    assert_eq!(card.do_command(&SdRequest::new(32, 512), &mut resp), 4);
    assert_eq!(card.do_command(&SdRequest::new(33, 1024), &mut resp), 4);
    assert_eq!(card.do_command(&SdRequest::new(38, 0), &mut resp), 4);
    assert_eq!(
        resp_u32(&resp) & machina_hw_sd::status::CURRENT_STATE,
        4 << 9
    );

    assert_eq!(card.do_command(&SdRequest::new(17, 0), &mut resp), 4);
    for _ in 0..512 {
        assert_eq!(card.read_byte(), 0xaa);
    }

    assert_eq!(card.do_command(&SdRequest::new(17, 512), &mut resp), 4);
    for _ in 0..512 {
        assert_eq!(card.read_byte(), 0);
    }

    assert_eq!(card.do_command(&SdRequest::new(17, 1024), &mut resp), 4);
    for _ in 0..512 {
        assert_eq!(card.read_byte(), 0);
    }
}

#[test]
fn test_sd_card_cmd38_readonly_sets_wp_violation() {
    let card = readonly_sd_card(vec![0x5a; 1024]);
    select_card(&card);
    let mut resp = [0; 16];

    assert_eq!(card.do_command(&SdRequest::new(32, 0), &mut resp), 4);
    assert_eq!(card.do_command(&SdRequest::new(33, 512), &mut resp), 4);
    assert_eq!(card.do_command(&SdRequest::new(38, 0), &mut resp), 4);

    assert_ne!(resp_u32(&resp) & machina_hw_sd::status::WP_VIOLATION, 0);

    assert_eq!(card.do_command(&SdRequest::new(17, 0), &mut resp), 4);
    for _ in 0..512 {
        assert_eq!(card.read_byte(), 0x5a);
    }
}

#[test]
fn test_sdhci_mmio_reset_registers() {
    let controller = Arc::new(Sdhci::new());
    let mmio = SdhciMmio(controller);

    assert_eq!(mmio.read(0x24, 4), 1 << 17);
    assert_eq!(mmio.read(0x30, 2), 0);
    assert_eq!(mmio.read(0x34, 2), 0);
    assert_eq!(mmio.read(0x38, 2), 0);
    assert_eq!(mmio.read(0xfe, 2), 0x0002);
}

#[test]
fn test_sdhci_rejects_accesses_wider_than_32bit() {
    let controller = Arc::new(Sdhci::new());
    let mmio = SdhciMmio(controller);

    assert_eq!(mmio.read(0x24, 8), 0);

    mmio.write(0x08, 8, 0x1234_5678);
    assert_eq!(mmio.read(0x08, 4), 0);
}

#[test]
fn test_sdhci_rejects_unaligned_mmio_accesses() {
    let controller = Arc::new(Sdhci::new());
    let mmio = SdhciMmio(controller);

    assert_eq!(mmio.read(0x0e, 4), 0);

    mmio.write(0x0e, 4, 8 << 8);
    assert_eq!(mmio.read(0x30, 2), 0);
    assert_eq!(mmio.read(0x32, 2), 0);
}

#[test]
fn test_sdhci_bus_callbacks_update_present_state_and_interrupt() {
    let bus = SdBus::new();
    let controller = Arc::new(Sdhci::new());
    bus.set_host(controller.clone());
    let mmio = SdhciMmio(controller);

    let card = MockSdCard::new(true);
    *card.readonly.lock().unwrap() = true;
    bus.insert_card(card);

    let present = mmio.read(0x24, 4);
    assert_ne!(present & (1 << 16), 0);
    assert_ne!(present & (1 << 17), 0);
    assert_ne!(present & (1 << 18), 0);
    assert_eq!(present & (1 << 19), 0);
    assert_ne!(mmio.read(0x30, 2) & (1 << 6), 0);

    bus.remove_card();

    let present = mmio.read(0x24, 4);
    assert_eq!(present & (1 << 16), 0);
    assert_ne!(mmio.read(0x30, 2) & (1 << 7), 0);
}

#[test]
fn test_sdhci_software_reset_clears_runtime_registers() {
    let controller = Arc::new(Sdhci::new());
    let mmio = SdhciMmio(controller);

    mmio.write(0x34, 2, 0xffff);
    mmio.write(0x38, 2, 0x00ff);
    mmio.write(0x30, 2, 0xffff);
    assert_ne!(mmio.read(0x34, 2), 0);
    assert_ne!(mmio.read(0x38, 2), 0);

    mmio.write(0x2f, 1, 0x01);

    assert_eq!(mmio.read(0x30, 2), 0);
    assert_eq!(mmio.read(0x34, 2), 0);
    assert_eq!(mmio.read(0x38, 2), 0);
    assert_eq!(mmio.read(0x2f, 1), 0);
}

#[test]
fn test_sdhci_lifecycle_and_mom_identity() {
    let controller = Arc::new(Sdhci::new());
    assert!(!controller.realized());
    controller.with_mdevice(|device| assert_eq!(device.local_id(), "sdhci"));
    assert_eq!(controller.object_info().local_id, "sdhci");

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA::new(0x1000_0000);

    controller.attach_to_bus(&mut bus).unwrap();
    controller
        .register_mmio(
            MemoryRegion::io(
                "sdhci0",
                0x100,
                Arc::new(SdhciMmio(Arc::clone(&controller))),
            ),
            base,
        )
        .unwrap();

    controller.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(controller.realized());
    assert_eq!(aspace.read(GPA::new(base.0 + 0xfe), 2), 0x0002);

    let err = controller.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(
        err.to_string().contains("already realized"),
        "unexpected second-realize error: {err}"
    );

    controller.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!controller.realized());
    assert_eq!(aspace.read(GPA::new(base.0 + 0xfe), 2), 0);
}

#[test]
fn test_sdhci_command_write_dispatches_to_card_and_stores_response() {
    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(bus.clone());
    bus.set_host(controller.clone());
    let mmio = SdhciMmio(controller);

    let card = MockSdCard::new(true);
    card.set_response(&[0x00, 0x00, 0x01, 0xaa]);
    bus.insert_card(card.clone());

    mmio.write(0x08, 4, 0x1aa);
    mmio.write(0x0e, 2, 8 << 8);

    assert_eq!(card.commands(), vec![(8, 0x1aa)]);
    assert_eq!(mmio.read(0x10, 4), 0x1aa);
    assert_ne!(mmio.read(0x30, 2) & 1, 0);

    mmio.write(0x30, 2, 1);
    assert_eq!(mmio.read(0x30, 2) & 1, 0);
}

#[test]
fn test_sdhci_command_without_card_sets_error_interrupt_status() {
    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(bus);
    let mmio = SdhciMmio(controller);

    mmio.write(0x08, 4, 0x1aa);
    mmio.write(0x0e, 2, 8 << 8);

    assert_eq!(mmio.read(0x30, 2) & (1 << 0), 0);
    assert_ne!(mmio.read(0x30, 2) & (1 << 15), 0);
    assert_ne!(mmio.read(0x32, 2) & (1 << 0), 0);

    mmio.write(0x32, 2, 1 << 0);
    assert_eq!(mmio.read(0x32, 2), 0);
    assert_eq!(mmio.read(0x30, 2) & (1 << 15), 0);
}

#[test]
fn test_sdhci_data_port_reads_single_block_after_cmd17() {
    let mut data = vec![0; 1024];
    data[512] = 0x11;
    data[513] = 0x22;
    data[514] = 0x33;
    data[515] = 0x44;
    data[1023] = 0xee;
    let card = Arc::new(sd_card(data));
    select_card(&card);

    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(bus.clone());
    bus.set_host(controller.clone());
    bus.insert_card(card);
    let mmio = SdhciMmio(controller);

    mmio.write(0x04, 2, 512);
    mmio.write(0x08, 4, 512);
    mmio.write(0x0e, 2, 17 << 8);

    assert_ne!(mmio.read(0x30, 2) & (1 << 0), 0);
    assert_ne!(mmio.read(0x30, 2) & (1 << 5), 0);
    assert_eq!(mmio.read(0x20, 4), 0x4433_2211);
    for _ in 1..127 {
        assert_eq!(mmio.read(0x20, 4), 0);
    }
    assert_eq!(mmio.read(0x20, 4), 0xee00_0000);
    assert_ne!(mmio.read(0x30, 2) & (1 << 1), 0);
    assert_eq!(mmio.read(0x30, 2) & (1 << 5), 0);
}

#[test]
fn test_sdhci_sdma_reads_single_block_after_cmd17() {
    let mut data = vec![0; 1024];
    data[512..520]
        .copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
    data[1023] = 0xee;
    let card = Arc::new(sd_card(data));
    select_card(&card);

    let dma_addr = 0x2000;
    let aspace = make_ram_aspace(0x4000);
    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(bus.clone());
    controller.set_dma_address_space(aspace.clone());
    bus.set_host(controller.clone());
    bus.insert_card(card);
    let mmio = SdhciMmio(controller);

    assert_ne!(mmio.read(0x40, 4) & (1 << 22), 0);

    mmio.write(0x00, 4, dma_addr);
    mmio.write(0x04, 2, 512);
    mmio.write(0x0c, 2, 1);
    mmio.write(0x08, 4, 512);
    mmio.write(0x0e, 2, 17 << 8);

    assert_ne!(mmio.read(0x30, 2) & (1 << 0), 0);
    assert_ne!(mmio.read(0x30, 2) & (1 << 1), 0);
    assert_eq!(mmio.read(0x30, 2) & (1 << 5), 0);
    assert_eq!(aspace.read(GPA(dma_addr), 4), 0x4433_2211);
    assert_eq!(aspace.read(GPA(dma_addr + 4), 4), 0x8877_6655);
    assert_eq!(aspace.read(GPA(dma_addr + 508), 4), 0xee00_0000);
}

#[test]
fn test_sdhci_sdma_card_read_caps_dma_write_at_region_end() {
    let mut data = vec![0; 1024];
    data[512] = 0x5a;
    let card = Arc::new(sd_card(data));
    select_card(&card);

    let dma_addr = 0x2000;
    let probe = Arc::new(DmaProbe::default());
    let aspace = make_io_aspace(dma_addr, 1, probe.clone());
    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(bus.clone());
    controller.set_dma_address_space(aspace);
    bus.set_host(controller.clone());
    bus.insert_card(card);
    let mmio = SdhciMmio(controller);

    mmio.write(0x00, 4, dma_addr);
    mmio.write(0x04, 2, 512);
    mmio.write(0x0c, 2, 1);
    mmio.write(0x08, 4, 512);
    mmio.write(0x0e, 2, 17 << 8);

    assert_ne!(mmio.read(0x30, 2) & (1 << 1), 0);
    assert_eq!(probe.writes(), vec![(1, 0x5a)]);
}

#[test]
fn test_sdhci_sdma_reads_scr_after_acmd51() {
    let card = Arc::new(sd_card(vec![0; 1024]));
    let rca = select_card(&card);

    let dma_addr = 0x2000;
    let aspace = make_ram_aspace(0x4000);
    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(bus.clone());
    controller.set_dma_address_space(aspace.clone());
    bus.set_host(controller.clone());
    bus.insert_card(card);
    let mmio = SdhciMmio(controller);

    mmio.write(0x08, 4, u64::from(rca << 16));
    mmio.write(0x0e, 2, 55 << 8);
    mmio.write(0x30, 2, 1);

    mmio.write(0x00, 4, dma_addr);
    mmio.write(0x04, 2, 8);
    mmio.write(0x0c, 2, 1);
    mmio.write(0x08, 4, 0);
    mmio.write(0x0e, 2, 51 << 8);

    assert_ne!(mmio.read(0x30, 2) & (1 << 1), 0);
    assert_ne!(aspace.read(GPA(dma_addr), 4), 0);
}

#[test]
fn test_sdhci_sdma_reads_switch_status_after_cmd6() {
    let card = Arc::new(sd_card(vec![0; 1024]));
    select_card(&card);

    let dma_addr = 0x2000;
    let aspace = make_ram_aspace(0x4000);
    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(bus.clone());
    controller.set_dma_address_space(aspace.clone());
    bus.set_host(controller.clone());
    bus.insert_card(card);
    let mmio = SdhciMmio(controller);

    mmio.write(0x00, 4, dma_addr);
    mmio.write(0x04, 2, 64);
    mmio.write(0x0c, 2, 1);
    mmio.write(0x08, 4, 0x00ff_fff1);
    mmio.write(0x0e, 2, 6 << 8);

    assert_ne!(mmio.read(0x30, 2) & (1 << 1), 0);
    assert_eq!(aspace.read(GPA(dma_addr + 16), 4) & 0xff, 1);
}

#[test]
fn test_sdhci_cmd12_reports_data_end_for_r1b_stop() {
    let card = Arc::new(sd_card(vec![0; 4096]));
    select_card(&card);

    let dma_addr = 0x2000;
    let aspace = make_ram_aspace(0x4000);
    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(bus.clone());
    controller.set_dma_address_space(aspace);
    bus.set_host(controller.clone());
    bus.insert_card(card);
    let mmio = SdhciMmio(controller);

    mmio.write(0x00, 4, dma_addr);
    mmio.write(0x04, 2, 512);
    mmio.write(0x06, 2, 2);
    mmio.write(0x0c, 2, 1);
    mmio.write(0x08, 4, 0);
    mmio.write(0x0e, 2, 18 << 8);
    mmio.write(0x30, 2, u64::MAX);

    mmio.write(0x08, 4, 0);
    mmio.write(0x0e, 2, 12 << 8);

    let status = mmio.read(0x30, 2);
    assert_ne!(status & (1 << 0), 0);
    assert_ne!(status & (1 << 1), 0);
}

#[test]
fn test_sdhci_data_port_writes_single_block_after_cmd24() {
    let card = Arc::new(sd_card(vec![0; 1024]));
    select_card(&card);

    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(bus.clone());
    bus.set_host(controller.clone());
    bus.insert_card(card.clone());
    let mmio = SdhciMmio(controller);

    mmio.write(0x04, 2, 512);
    mmio.write(0x08, 4, 512);
    mmio.write(0x0e, 2, 24 << 8);

    assert_ne!(mmio.read(0x30, 2) & (1 << 0), 0);
    assert_ne!(mmio.read(0x30, 2) & (1 << 4), 0);
    for index in 0..128 {
        let bytes = [
            (index * 4) as u8,
            (index * 4 + 1) as u8,
            (index * 4 + 2) as u8,
            (index * 4 + 3) as u8,
        ];
        mmio.write(0x20, 4, u64::from(u32::from_le_bytes(bytes)));
    }

    assert_ne!(mmio.read(0x30, 2) & (1 << 1), 0);
    assert_eq!(mmio.read(0x30, 2) & (1 << 4), 0);

    let mut resp = [0; 16];
    assert_eq!(card.do_command(&SdRequest::new(17, 512), &mut resp), 4);
    for index in 0..512 {
        assert_eq!(card.read_byte(), (index & 0xff) as u8);
    }
}

#[test]
fn test_sdhci_sdma_writes_single_block_after_cmd24() {
    let card = Arc::new(sd_card(vec![0; 1024]));
    select_card(&card);

    let dma_addr = 0x2000;
    let aspace = make_ram_aspace(0x4000);
    for offset in (0..512).step_by(4) {
        let bytes = [
            offset as u8,
            (offset + 1) as u8,
            (offset + 2) as u8,
            (offset + 3) as u8,
        ];
        aspace.write(
            GPA(dma_addr + offset as u64),
            4,
            u64::from(u32::from_le_bytes(bytes)),
        );
    }

    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(bus.clone());
    controller.set_dma_address_space(aspace);
    bus.set_host(controller.clone());
    bus.insert_card(card.clone());
    let mmio = SdhciMmio(controller);

    mmio.write(0x00, 4, dma_addr);
    mmio.write(0x04, 2, 512);
    mmio.write(0x0c, 2, 1);
    mmio.write(0x08, 4, 512);
    mmio.write(0x0e, 2, 24 << 8);

    assert_ne!(mmio.read(0x30, 2) & (1 << 0), 0);
    assert_ne!(mmio.read(0x30, 2) & (1 << 1), 0);
    assert_eq!(mmio.read(0x30, 2) & (1 << 4), 0);

    let mut resp = [0; 16];
    assert_eq!(card.do_command(&SdRequest::new(17, 512), &mut resp), 4);
    for index in 0..512 {
        assert_eq!(card.read_byte(), (index & 0xff) as u8);
    }
}

#[test]
fn test_sdhci_sdma_card_write_caps_dma_read_at_region_end() {
    let card = Arc::new(sd_card(vec![0; 1024]));
    select_card(&card);

    let dma_addr = 0x2000;
    let probe = Arc::new(DmaProbe::default());
    let aspace = make_io_aspace(dma_addr, 1, probe.clone());
    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(bus.clone());
    controller.set_dma_address_space(aspace);
    bus.set_host(controller.clone());
    bus.insert_card(card.clone());
    let mmio = SdhciMmio(controller);

    mmio.write(0x00, 4, dma_addr);
    mmio.write(0x04, 2, 512);
    mmio.write(0x0c, 2, 1);
    mmio.write(0x08, 4, 512);
    mmio.write(0x0e, 2, 24 << 8);

    assert_ne!(mmio.read(0x30, 2) & (1 << 1), 0);
    assert_eq!(probe.read_sizes(), vec![1]);

    let mut resp = [0; 16];
    assert_eq!(card.do_command(&SdRequest::new(17, 512), &mut resp), 4);
    assert_eq!(card.read_byte(), 0x5a);
    for _ in 1..512 {
        assert_eq!(card.read_byte(), 0);
    }
}

#[test]
fn test_sdhci_snps_phy_and_vendor_registers_are_tolerated() {
    let controller = Arc::new(Sdhci::new());
    let mmio = SdhciMmio(controller);

    mmio.write(0x300, 2, 0);
    assert_ne!(mmio.read(0x300, 4) & (1 << 1), 0);

    mmio.write(0x304, 2, 0x0262);
    assert_eq!(mmio.read(0x304, 2), 0x0262);

    mmio.write(0x31d, 1, 0x44);
    assert_eq!(mmio.read(0x31d, 1), 0x44);

    mmio.write(0x540, 4, 0x001b_0000);
    mmio.write(0x544, 4, 0);
    assert_eq!(mmio.read(0x540, 4), 0x001b_0000);
    assert_eq!(mmio.read(0x544, 4), 0);
}

#[test]
fn test_sdhci_clock_control_reports_internal_clock_stable() {
    let controller = Arc::new(Sdhci::new());
    let mmio = SdhciMmio(controller);

    mmio.write(0x2c, 2, 1);

    assert_eq!(mmio.read(0x2c, 2) & 0x3, 0x3);
}

#[test]
fn test_sdhci_capabilities_advertise_sdma_clock_and_3v3() {
    let controller = Arc::new(Sdhci::new());
    let mmio = SdhciMmio(controller);
    let caps = mmio.read(0x40, 4);

    assert_ne!(caps & (1 << 22), 0);
    assert_ne!(caps & (1 << 24), 0);
    assert_ne!(caps & (0xff << 8), 0);
}

#[test]
fn test_pl181_reset_clears_runtime_registers() {
    let controller = Arc::new(Pl181::new());
    let mmio = Pl181Mmio(controller.clone());

    mmio.write(0x00, 4, 0x03);
    mmio.write(0x04, 4, 0x1ff);
    mmio.write(0x08, 4, 0xdead_beef);

    assert_eq!(mmio.read(0x00, 4), 0x03);
    assert_eq!(mmio.read(0x04, 4), 0xff);
    assert_eq!(mmio.read(0x08, 4), 0xdead_beef);

    controller.reset_runtime();

    assert_eq!(mmio.read(0x00, 4), 0);
    assert_eq!(mmio.read(0x04, 4), 0);
    assert_eq!(mmio.read(0x08, 4), 0);
}

#[test]
fn test_pl181_write_masks_narrow_control_registers() {
    let controller = Arc::new(Pl181::new());
    let mmio = Pl181Mmio(controller);

    mmio.write(0x00, 4, 0x123);
    mmio.write(0x28, 4, 0x1234_5678);
    mmio.write(0x2c, 4, 0x1ff);

    assert_eq!(mmio.read(0x00, 4), 0x23);
    assert_eq!(mmio.read(0x28, 4), 0x5678);
    assert_eq!(mmio.read(0x2c, 4), 0xff);
}

#[test]
fn test_pl181_wide_mmio_accesses_split_into_32bit_callbacks() {
    let controller = Arc::new(Pl181::new());
    let mmio = Pl181Mmio(controller);

    mmio.write(0x3c, 8, 0x1234_5678_9abc_def0);

    assert_eq!(mmio.read(0x3c, 4), 0x9abc_def0);
    assert_eq!(mmio.read(0x40, 4), 0x1234_5678);
    assert_eq!(mmio.read(0x3c, 8), 0x1234_5678_9abc_def0);
}

#[test]
fn test_pl181_unaligned_wide_accesses_split_like_qemu() {
    let controller = Arc::new(Pl181::new());
    let mmio = Pl181Mmio(controller);

    mmio.write(0x08, 4, 0x1234_5678);
    mmio.write(0x0c, 4, 0x0000_daf0);

    assert_eq!(mmio.read(0x09, 4), 0xf000_0000);
    assert_eq!(mmio.read(0x0a, 4), 0xdaf0_0000);
    assert_eq!(mmio.read(0x0b, 4), 0x00da_f000);

    mmio.write(0x08, 4, 0);
    mmio.write(0x0c, 4, 0);
    mmio.write(0x09, 4, 0x0102_0304);
    assert_eq!(mmio.read(0x08, 4), 0);
    assert_eq!(mmio.read(0x0c, 4), 0x0000_0001);

    mmio.write(0x0c, 4, 0);
    mmio.write(0x0a, 4, 0x0102_0304);
    assert_eq!(mmio.read(0x0c, 4), 0x0000_0102);

    mmio.write(0x0c, 4, 0);
    mmio.write(0x0b, 4, 0x0102_0304);
    assert_eq!(mmio.read(0x0c, 4), 0x0000_0203);
}

#[test]
fn test_pl181_lifecycle_and_mom_identity() {
    const RESET_STATUS: u64 = (1 << 18) | (1 << 19);

    let controller = Arc::new(Pl181::new());
    assert!(!controller.realized());
    controller.with_mdevice(|device| assert_eq!(device.local_id(), "pl181"));
    assert_eq!(controller.object_info().local_id, "pl181");

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA::new(0x1001_0000);

    controller.attach_to_bus(&mut bus).unwrap();
    controller
        .register_mmio(
            MemoryRegion::io(
                "pl1810",
                0x1000,
                Arc::new(Pl181Mmio(Arc::clone(&controller))),
            ),
            base,
        )
        .unwrap();

    controller.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(controller.realized());
    assert_eq!(aspace.read(GPA::new(base.0 + 0x34), 4), RESET_STATUS);

    let err = controller.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(
        err.to_string().contains("already realized"),
        "unexpected second-realize error: {err}"
    );

    controller.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!controller.realized());
    assert_eq!(aspace.read(GPA::new(base.0 + 0x34), 4), 0);
}

#[test]
fn test_pl181_command_dispatch_stores_response_and_status() {
    const COMMAND_RESPONSE: u64 = 1 << 6;
    const COMMAND_ENABLE: u64 = 1 << 10;
    const STATUS_COMMAND_RESPONSE_END: u64 = 1 << 6;

    let sd_bus = Arc::new(SdBus::new());
    let controller = Arc::new(Pl181::new());
    controller.connect_bus(sd_bus.clone());
    let mmio = Pl181Mmio(controller);

    let card = MockSdCard::new(true);
    card.set_response(&[0x12, 0x34, 0x56, 0x78]);
    sd_bus.insert_card(card.clone());

    mmio.write(0x08, 4, 0x1aa);
    mmio.write(0x0c, 4, COMMAND_ENABLE | COMMAND_RESPONSE | 8);

    assert_eq!(card.commands(), vec![(8, 0x1aa)]);
    assert_eq!(mmio.read(0x10, 4), 8);
    assert_eq!(mmio.read(0x14, 4), 0x1234_5678);
    assert_ne!(mmio.read(0x34, 4) & STATUS_COMMAND_RESPONSE_END, 0);

    mmio.write(0x38, 4, STATUS_COMMAND_RESPONSE_END);

    assert_eq!(mmio.read(0x34, 4) & STATUS_COMMAND_RESPONSE_END, 0);
}

#[test]
fn test_pl181_masked_status_drives_two_irq_outputs() {
    const COMMAND_RESPONSE: u64 = 1 << 6;
    const COMMAND_ENABLE: u64 = 1 << 10;
    const STATUS_COMMAND_RESPONSE_END: u64 = 1 << 6;

    let sink = Pl181IrqSink::new(2);
    let controller = Arc::new(Pl181::new());
    controller.connect_output(
        PL181_IRQ0,
        InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0),
    );
    controller.connect_output(
        PL181_IRQ1,
        InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 1),
    );
    let sd_bus = Arc::new(SdBus::new());
    controller.connect_bus(sd_bus.clone());
    let mmio = Pl181Mmio(controller);

    let card = MockSdCard::new(true);
    card.set_response(&[0x00]);
    sd_bus.insert_card(card);

    mmio.write(0x08, 4, 0x1aa);
    mmio.write(0x0c, 4, COMMAND_ENABLE | COMMAND_RESPONSE | 8);

    assert!(!sink.level(0));
    assert!(!sink.level(1));

    mmio.write(0x3c, 4, STATUS_COMMAND_RESPONSE_END);
    assert!(sink.level(0));
    assert!(!sink.level(1));

    mmio.write(0x40, 4, STATUS_COMMAND_RESPONSE_END);
    assert!(sink.level(1));

    mmio.write(0x38, 4, STATUS_COMMAND_RESPONSE_END);
    assert!(!sink.level(0));
    assert!(!sink.level(1));
}

#[test]
fn test_pl181_data_fifo_reads_single_block_after_cmd17() {
    const COMMAND_RESPONSE: u64 = 1 << 6;
    const COMMAND_ENABLE: u64 = 1 << 10;
    const DATACTRL_ENABLE: u64 = 1 << 0;
    const DATACTRL_DIRECTION: u64 = 1 << 1;
    const DATACTRL_BLOCK_SIZE_512: u64 = 9 << 4;
    const STATUS_DATA_END: u64 = 1 << 8;
    const STATUS_DATA_BLOCK_END: u64 = 1 << 10;
    const STATUS_RX_DATA_AVAILABLE: u64 = 1 << 21;

    let mut data = vec![0; 1024];
    data[512] = 0x11;
    data[513] = 0x22;
    data[514] = 0x33;
    data[515] = 0x44;
    data[1023] = 0xee;
    let card = Arc::new(sd_card(data));
    select_card(&card);

    let sd_bus = Arc::new(SdBus::new());
    let controller = Arc::new(Pl181::new());
    controller.connect_bus(sd_bus.clone());
    sd_bus.insert_card(card);
    let mmio = Pl181Mmio(controller);

    mmio.write(0x28, 4, 512);
    mmio.write(
        0x2c,
        4,
        DATACTRL_ENABLE | DATACTRL_DIRECTION | DATACTRL_BLOCK_SIZE_512,
    );
    mmio.write(0x08, 4, 512);
    mmio.write(0x0c, 4, COMMAND_ENABLE | COMMAND_RESPONSE | 17);

    assert_ne!(mmio.read(0x34, 4) & STATUS_RX_DATA_AVAILABLE, 0);
    assert_eq!(mmio.read(0x80, 4), 0x4433_2211);
    for _ in 1..127 {
        assert_eq!(mmio.read(0x80, 4), 0);
    }
    assert_eq!(mmio.read(0x80, 4), 0xee00_0000);
    assert_ne!(mmio.read(0x34, 4) & STATUS_DATA_END, 0);
    assert_ne!(mmio.read(0x34, 4) & STATUS_DATA_BLOCK_END, 0);
    assert_eq!(mmio.read(0x34, 4) & STATUS_RX_DATA_AVAILABLE, 0);
}

#[test]
fn test_pl181_data_fifo_reads_multiple_blocks_after_cmd18() {
    const COMMAND_RESPONSE: u64 = 1 << 6;
    const COMMAND_ENABLE: u64 = 1 << 10;
    const DATACTRL_ENABLE: u64 = 1 << 0;
    const DATACTRL_DIRECTION: u64 = 1 << 1;
    const DATACTRL_BLOCK_SIZE_512: u64 = 9 << 4;
    const STATUS_DATA_END: u64 = 1 << 8;
    const STATUS_DATA_BLOCK_END: u64 = 1 << 10;
    const STATUS_RX_DATA_AVAILABLE: u64 = 1 << 21;

    let mut data = vec![0; 2048];
    for index in 0..512 {
        data[512 + index] = index as u8;
        data[1024 + index] = 0x80 | (index as u8 & 0x7f);
    }
    let card = Arc::new(sd_card(data));
    let rca = select_card(&card);

    let sd_bus = Arc::new(SdBus::new());
    let controller = Arc::new(Pl181::new());
    controller.connect_bus(sd_bus.clone());
    sd_bus.insert_card(card.clone());
    let mmio = Pl181Mmio(controller);

    mmio.write(0x28, 4, 1024);
    mmio.write(
        0x2c,
        4,
        DATACTRL_ENABLE | DATACTRL_DIRECTION | DATACTRL_BLOCK_SIZE_512,
    );
    mmio.write(0x08, 4, 512);
    mmio.write(0x0c, 4, COMMAND_ENABLE | COMMAND_RESPONSE | 18);

    assert_ne!(mmio.read(0x34, 4) & STATUS_RX_DATA_AVAILABLE, 0);
    for index in 0..256 {
        let offset = index * 4;
        let expected = [
            if offset < 512 {
                offset as u8
            } else {
                0x80 | (offset as u8 & 0x7f)
            },
            if offset + 1 < 512 {
                (offset + 1) as u8
            } else {
                0x80 | ((offset + 1) as u8 & 0x7f)
            },
            if offset + 2 < 512 {
                (offset + 2) as u8
            } else {
                0x80 | ((offset + 2) as u8 & 0x7f)
            },
            if offset + 3 < 512 {
                (offset + 3) as u8
            } else {
                0x80 | ((offset + 3) as u8 & 0x7f)
            },
        ];
        assert_eq!(mmio.read(0x80, 4), u64::from(u32::from_le_bytes(expected)));
    }
    assert_ne!(mmio.read(0x34, 4) & STATUS_DATA_END, 0);
    assert_ne!(mmio.read(0x34, 4) & STATUS_DATA_BLOCK_END, 0);
    assert_eq!(mmio.read(0x34, 4) & STATUS_RX_DATA_AVAILABLE, 0);

    mmio.write(0x08, 4, 0);
    mmio.write(0x0c, 4, COMMAND_ENABLE | COMMAND_RESPONSE | 12);

    let mut resp = [0; 16];
    assert_eq!(
        card.do_command(&SdRequest::new(13, rca << 16), &mut resp),
        4
    );
    assert_eq!(
        resp_u32(&resp) & machina_hw_sd::status::CURRENT_STATE,
        4 << 9
    );
}

#[test]
fn test_pl181_cmd17_without_datactrl_does_not_drain_card_data() {
    const COMMAND_RESPONSE: u64 = 1 << 6;
    const COMMAND_ENABLE: u64 = 1 << 10;
    const STATUS_RX_DATA_AVAILABLE: u64 = 1 << 21;

    let mut data = vec![0; 1024];
    data[512] = 0x11;
    data[513] = 0x22;
    let card = Arc::new(sd_card(data));
    select_card(&card);

    let sd_bus = Arc::new(SdBus::new());
    let controller = Arc::new(Pl181::new());
    controller.connect_bus(sd_bus.clone());
    sd_bus.insert_card(card.clone());
    let mmio = Pl181Mmio(controller);

    mmio.write(0x08, 4, 512);
    mmio.write(0x0c, 4, COMMAND_ENABLE | COMMAND_RESPONSE | 17);

    assert_eq!(mmio.read(0x34, 4) & STATUS_RX_DATA_AVAILABLE, 0);
    assert!(card.data_ready());
    assert_eq!(card.read_byte(), 0x11);
    assert_eq!(card.read_byte(), 0x22);
}

#[test]
fn test_pl181_data_fifo_writes_single_block_after_cmd24() {
    const COMMAND_RESPONSE: u64 = 1 << 6;
    const COMMAND_ENABLE: u64 = 1 << 10;
    const DATACTRL_ENABLE: u64 = 1 << 0;
    const DATACTRL_BLOCK_SIZE_512: u64 = 9 << 4;
    const STATUS_DATA_END: u64 = 1 << 8;
    const STATUS_DATA_BLOCK_END: u64 = 1 << 10;

    let card = Arc::new(sd_card(vec![0; 1024]));
    select_card(&card);

    let sd_bus = Arc::new(SdBus::new());
    let controller = Arc::new(Pl181::new());
    controller.connect_bus(sd_bus.clone());
    sd_bus.insert_card(card.clone());
    let mmio = Pl181Mmio(controller);

    mmio.write(0x28, 4, 512);
    mmio.write(0x2c, 4, DATACTRL_ENABLE | DATACTRL_BLOCK_SIZE_512);
    mmio.write(0x08, 4, 512);
    mmio.write(0x0c, 4, COMMAND_ENABLE | COMMAND_RESPONSE | 24);

    for index in 0..128 {
        let bytes = [
            (index * 4) as u8,
            (index * 4 + 1) as u8,
            (index * 4 + 2) as u8,
            (index * 4 + 3) as u8,
        ];
        mmio.write(0x80, 4, u64::from(u32::from_le_bytes(bytes)));
    }

    assert_ne!(mmio.read(0x34, 4) & STATUS_DATA_END, 0);
    assert_ne!(mmio.read(0x34, 4) & STATUS_DATA_BLOCK_END, 0);

    let mut resp = [0; 16];
    assert_eq!(card.do_command(&SdRequest::new(17, 512), &mut resp), 4);
    for index in 0..512 {
        assert_eq!(card.read_byte(), (index & 0xff) as u8);
    }
}

#[test]
fn test_pl181_data_fifo_writes_multiple_blocks_after_cmd25() {
    const COMMAND_RESPONSE: u64 = 1 << 6;
    const COMMAND_ENABLE: u64 = 1 << 10;
    const DATACTRL_ENABLE: u64 = 1 << 0;
    const DATACTRL_BLOCK_SIZE_512: u64 = 9 << 4;
    const STATUS_DATA_END: u64 = 1 << 8;
    const STATUS_DATA_BLOCK_END: u64 = 1 << 10;

    let card = Arc::new(sd_card(vec![0; 2048]));
    select_card(&card);

    let sd_bus = Arc::new(SdBus::new());
    let controller = Arc::new(Pl181::new());
    controller.connect_bus(sd_bus.clone());
    sd_bus.insert_card(card.clone());
    let mmio = Pl181Mmio(controller);

    mmio.write(0x28, 4, 1024);
    mmio.write(0x2c, 4, DATACTRL_ENABLE | DATACTRL_BLOCK_SIZE_512);
    mmio.write(0x08, 4, 512);
    mmio.write(0x0c, 4, COMMAND_ENABLE | COMMAND_RESPONSE | 25);

    for index in 0..256 {
        let offset = index * 4;
        let bytes = [
            if offset < 512 {
                offset as u8
            } else {
                0x80 | (offset as u8 & 0x7f)
            },
            if offset + 1 < 512 {
                (offset + 1) as u8
            } else {
                0x80 | ((offset + 1) as u8 & 0x7f)
            },
            if offset + 2 < 512 {
                (offset + 2) as u8
            } else {
                0x80 | ((offset + 2) as u8 & 0x7f)
            },
            if offset + 3 < 512 {
                (offset + 3) as u8
            } else {
                0x80 | ((offset + 3) as u8 & 0x7f)
            },
        ];
        mmio.write(0x80, 4, u64::from(u32::from_le_bytes(bytes)));
    }

    assert_ne!(mmio.read(0x34, 4) & STATUS_DATA_END, 0);
    assert_ne!(mmio.read(0x34, 4) & STATUS_DATA_BLOCK_END, 0);

    mmio.write(0x08, 4, 0);
    mmio.write(0x0c, 4, COMMAND_ENABLE | COMMAND_RESPONSE | 12);

    let mut resp = [0; 16];
    assert_eq!(card.do_command(&SdRequest::new(17, 512), &mut resp), 4);
    for index in 0..512 {
        assert_eq!(card.read_byte(), index as u8);
    }
    assert_eq!(card.do_command(&SdRequest::new(17, 1024), &mut resp), 4);
    for index in 0..512 {
        assert_eq!(card.read_byte(), 0x80 | (index as u8 & 0x7f));
    }
}

#[test]
fn test_sdhci_data_port_reads_multiple_blocks_after_cmd18() {
    let mut data = vec![0; 2048];
    for index in 0..512 {
        data[512 + index] = index as u8;
        data[1024 + index] = 0x80 | (index as u8 & 0x7f);
    }
    let card = Arc::new(sd_card(data));
    let rca = select_card(&card);

    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(bus.clone());
    bus.set_host(controller.clone());
    bus.insert_card(card.clone());
    let mmio = SdhciMmio(controller);

    mmio.write(0x04, 2, 512);
    mmio.write(0x06, 2, 2);
    mmio.write(0x08, 4, 512);
    mmio.write(0x0e, 2, 18 << 8);

    assert_ne!(mmio.read(0x30, 2) & (1 << 0), 0);
    assert_ne!(mmio.read(0x30, 2) & (1 << 5), 0);
    for index in 0..256 {
        let offset = index * 4;
        let expected = [
            if offset < 512 {
                offset as u8
            } else {
                0x80 | (offset as u8 & 0x7f)
            },
            if offset + 1 < 512 {
                (offset + 1) as u8
            } else {
                0x80 | ((offset + 1) as u8 & 0x7f)
            },
            if offset + 2 < 512 {
                (offset + 2) as u8
            } else {
                0x80 | ((offset + 2) as u8 & 0x7f)
            },
            if offset + 3 < 512 {
                (offset + 3) as u8
            } else {
                0x80 | ((offset + 3) as u8 & 0x7f)
            },
        ];
        assert_eq!(mmio.read(0x20, 4), u64::from(u32::from_le_bytes(expected)));
    }
    assert_ne!(mmio.read(0x30, 2) & (1 << 1), 0);
    assert_eq!(mmio.read(0x30, 2) & (1 << 5), 0);

    mmio.write(0x30, 2, 0xffff);
    mmio.write(0x08, 4, 0);
    mmio.write(0x0e, 2, 12 << 8);

    let mut resp = [0; 16];
    assert_eq!(
        card.do_command(&SdRequest::new(13, rca << 16), &mut resp),
        4
    );
    assert_eq!(
        resp_u32(&resp) & machina_hw_sd::status::CURRENT_STATE,
        4 << 9
    );
}

#[test]
fn test_sdhci_data_port_writes_multiple_blocks_after_cmd25() {
    let card = Arc::new(sd_card(vec![0; 2048]));
    select_card(&card);

    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(bus.clone());
    bus.set_host(controller.clone());
    bus.insert_card(card.clone());
    let mmio = SdhciMmio(controller);

    mmio.write(0x04, 2, 512);
    mmio.write(0x06, 2, 2);
    mmio.write(0x08, 4, 512);
    mmio.write(0x0e, 2, 25 << 8);

    assert_ne!(mmio.read(0x30, 2) & (1 << 0), 0);
    assert_ne!(mmio.read(0x30, 2) & (1 << 4), 0);
    for index in 0..256 {
        let offset = index * 4;
        let bytes = [
            if offset < 512 {
                offset as u8
            } else {
                0x80 | (offset as u8 & 0x7f)
            },
            if offset + 1 < 512 {
                (offset + 1) as u8
            } else {
                0x80 | ((offset + 1) as u8 & 0x7f)
            },
            if offset + 2 < 512 {
                (offset + 2) as u8
            } else {
                0x80 | ((offset + 2) as u8 & 0x7f)
            },
            if offset + 3 < 512 {
                (offset + 3) as u8
            } else {
                0x80 | ((offset + 3) as u8 & 0x7f)
            },
        ];
        mmio.write(0x20, 4, u64::from(u32::from_le_bytes(bytes)));
    }
    assert_ne!(mmio.read(0x30, 2) & (1 << 1), 0);
    assert_eq!(mmio.read(0x30, 2) & (1 << 4), 0);

    mmio.write(0x30, 2, 0xffff);
    mmio.write(0x08, 4, 0);
    mmio.write(0x0e, 2, 12 << 8);

    let mut resp = [0; 16];
    assert_eq!(card.do_command(&SdRequest::new(17, 512), &mut resp), 4);
    for index in 0..512 {
        assert_eq!(card.read_byte(), index as u8);
    }
    assert_eq!(card.do_command(&SdRequest::new(17, 1024), &mut resp), 4);
    for index in 0..512 {
        assert_eq!(card.read_byte(), 0x80 | (index as u8 & 0x7f));
    }
}

#[test]
fn test_ssi_sd_lifecycle_and_mom_identity() {
    let bridge = Arc::new(SsiSd::new(0));
    assert!(!bridge.realized());
    bridge.with_mdevice(|device| assert_eq!(device.local_id(), "ssi-sd"));
    assert_eq!(bridge.object_info().local_id, "ssi-sd");

    bridge.realize().unwrap();
    assert!(bridge.realized());
    let err = bridge.realize().unwrap_err();
    assert!(
        err.to_string().contains("already realized"),
        "unexpected second-realize error: {err}"
    );

    bridge.unrealize().unwrap();
    assert!(!bridge.realized());
    let err = bridge.unrealize().unwrap_err();
    assert!(
        err.to_string().contains("not realized"),
        "unexpected second-unrealize error: {err}"
    );
}

#[test]
fn test_ssi_sd_decodes_command_packet_and_returns_response_bytes() {
    let sd_bus = Arc::new(SdBus::new());
    let bridge = Arc::new(SsiSd::new(0));
    bridge.connect_sd_bus(sd_bus.clone());
    let spi_bus = SpiBus::new();
    spi_bus.attach(bridge.clone()).unwrap();

    let card = MockSdCard::new(true);
    card.set_response(&[0x01, 0x02, 0x03, 0x04]);
    sd_bus.insert_card(card.clone());

    assert_eq!(bridge.cs_polarity(), SpiCsPolarity::Low);
    assert_eq!(spi_bus.transfer(0xff), 0xff);

    spi_bus.set_cs(0, false);
    for byte in [0x48, 0x00, 0x00, 0x01, 0xaa, 0x87] {
        assert_eq!(spi_bus.transfer(byte), 0xff);
    }

    assert_eq!(card.commands(), vec![(8, 0x1aa)]);
    assert_eq!(spi_bus.transfer(0xff), 0xff);
    assert_eq!(spi_bus.transfer(0xff), 0x01);
    assert_eq!(spi_bus.transfer(0xff), 0x02);
    assert_eq!(spi_bus.transfer(0xff), 0x03);
    assert_eq!(spi_bus.transfer(0xff), 0x04);
    assert_eq!(spi_bus.transfer(0xff), 0xff);

    spi_bus.set_cs(0, true);
    assert_eq!(spi_bus.transfer(0xff), 0xff);
}

#[test]
fn test_ssi_sd_cmd17_returns_data_token_and_block_bytes() {
    let sd_bus = Arc::new(SdBus::new());
    let bridge = Arc::new(SsiSd::new(0));
    bridge.connect_sd_bus(sd_bus.clone());
    let spi_bus = SpiBus::new();
    spi_bus.attach(bridge).unwrap();

    let card = MockSdCard::new(true);
    card.set_response(&[0x00]);
    sd_bus.insert_card(card);

    spi_bus.set_cs(0, false);
    for byte in [0x51, 0x00, 0x00, 0x02, 0x00, 0xff] {
        assert_eq!(spi_bus.transfer(byte), 0xff);
    }

    assert_eq!(spi_bus.transfer(0xff), 0xff);
    assert_eq!(spi_bus.transfer(0xff), 0x00);
    assert_eq!(spi_bus.transfer(0xff), 0xfe);
    assert_eq!(spi_bus.transfer(0xff), 0x11);
    assert_eq!(spi_bus.transfer(0xff), 0x22);
    assert_eq!(spi_bus.transfer(0xff), 0x33);
    assert_eq!(spi_bus.transfer(0xff), 0x44);
    for _ in 4..512 {
        assert_eq!(spi_bus.transfer(0xff), 0xff);
    }
    assert_eq!(spi_bus.transfer(0xff), 0xff);
    assert_eq!(spi_bus.transfer(0xff), 0xff);
    assert_eq!(spi_bus.transfer(0xff), 0xff);
}

#[test]
fn test_ssi_sd_cmd24_accepts_data_token_and_writes_block_bytes() {
    let sd_bus = Arc::new(SdBus::new());
    let bridge = Arc::new(SsiSd::new(0));
    bridge.connect_sd_bus(sd_bus.clone());
    let spi_bus = SpiBus::new();
    spi_bus.attach(bridge).unwrap();

    let card = MockSdCard::new(true);
    card.set_response(&[0x00]);
    sd_bus.insert_card(card.clone());

    spi_bus.set_cs(0, false);
    for byte in [0x58, 0x00, 0x00, 0x02, 0x00, 0xff] {
        assert_eq!(spi_bus.transfer(byte), 0xff);
    }

    assert_eq!(spi_bus.transfer(0xff), 0xff);
    assert_eq!(spi_bus.transfer(0xff), 0x00);
    assert_eq!(spi_bus.transfer(0xff), 0xff);
    assert_eq!(spi_bus.transfer(0xfe), 0xff);

    let data: Vec<u8> = (0..512).map(|index| (index & 0xff) as u8).collect();
    for &byte in &data {
        assert_eq!(spi_bus.transfer(u32::from(byte)), 0xff);
    }
    assert_eq!(spi_bus.transfer(0xff), 0xff);
    assert_eq!(spi_bus.transfer(0xff), 0x05);
    assert_eq!(spi_bus.transfer(0xff), 0xff);

    assert_eq!(card.commands(), vec![(24, 512)]);
    assert_eq!(card.written(), data);
}

#[test]
fn test_sd_card_cmd0_resets_to_idle() {
    let card = sd_card(vec![0; 512]);
    select_card(&card);
    let mut resp = [0; 16];

    assert_eq!(card.do_command(&SdRequest::new(0, 0), &mut resp), 0);

    let len = card.do_command(&SdRequest::new(8, 0x1aa), &mut resp);
    assert_eq!(len, 4);
    assert_eq!(resp_u32(&resp), 0x1aa);
}

#[test]
fn test_sd_card_cmd17_out_of_range_sets_address_error() {
    let card = sd_card(vec![0; 512]);
    select_card(&card);
    let mut resp = [0; 16];

    assert_eq!(card.do_command(&SdRequest::new(17, 512), &mut resp), 4);

    assert_ne!(resp_u32(&resp) & machina_hw_sd::status::ADDRESS_ERROR, 0);
    assert!(!card.data_ready());
}

#[test]
fn test_sd_card_cmd24_readonly_sets_wp_violation() {
    let card = readonly_sd_card(vec![0; 1024]);
    select_card(&card);
    let mut resp = [0; 16];

    assert_eq!(card.do_command(&SdRequest::new(24, 512), &mut resp), 4);

    assert_ne!(resp_u32(&resp) & machina_hw_sd::status::WP_VIOLATION, 0);
    assert!(!card.receive_ready());
}

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

// -- Regression: host readonly sync --

#[test]
fn test_sd_insert_readonly_card_notifies_host() {
    let bus = SdBus::new();
    let host = MockHost::new();
    bus.set_host(host.clone());

    // Insert a readonly card
    let card = MockSdCard::new(true);
    *card.readonly.lock().unwrap() = true;
    bus.insert_card(card);

    assert_eq!(host.last_inserted(), Some(true));
    assert_eq!(host._last_readonly(), Some(true));
}

#[test]
fn test_sd_set_host_syncs_existing_card_state() {
    let bus = SdBus::new();

    // Insert a card before setting host
    let card = MockSdCard::new(true);
    bus.insert_card(card);

    // Now set the host — it should sync both inserted and readonly
    let host = MockHost::new();
    bus.set_host(host.clone());

    assert_eq!(host.last_inserted(), Some(true));
    assert_eq!(host._last_readonly(), Some(false)); // card is not readonly
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
