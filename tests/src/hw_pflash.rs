use std::sync::Arc;

use machina_core::address::GPA;
use machina_hw_core::bus::SysBus;
use machina_hw_storage::pflash::{
    PFlashCfi01, PFlashCfi01Config, PFlashCfi01Mmio, PFlashCfi02,
    PFlashCfi02Config, PFlashCfi02Mmio,
};
use machina_hw_storage::{FlashMedia, MemBackend};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

fn flash(data: Vec<u8>, sector_len: u32) -> FlashMedia<MemBackend> {
    FlashMedia::new(MemBackend::new(data, false), sector_len).unwrap()
}

fn cfi01(data: Vec<u8>) -> PFlashCfi01<MemBackend> {
    PFlashCfi01::new(
        flash(data, 4096),
        PFlashCfi01Config {
            sector_len: 4096,
            num_blocks: 2,
            ident0: 0x00,
            ident1: 0x89,
            ident2: 0x00,
            ident3: 0x18,
            ..PFlashCfi01Config::default()
        },
    )
    .unwrap()
}

fn cfi02(data: Vec<u8>) -> PFlashCfi02<MemBackend> {
    PFlashCfi02::new(
        flash(data, 4096),
        PFlashCfi02Config {
            sector_len: 4096,
            num_blocks: 2,
            ident0: 0x01,
            ident1: 0x7e,
            ident2: 0x22,
            ident3: 0x21,
            unlock_addr0: 0x555,
            unlock_addr1: 0x2aa,
            ..PFlashCfi02Config::default()
        },
    )
    .unwrap()
}

fn cfi01_x8(data: Vec<u8>) -> PFlashCfi01<MemBackend> {
    PFlashCfi01::new(
        flash(data, 4096),
        PFlashCfi01Config {
            bank_width: 1,
            device_width: 1,
            max_device_width: 1,
            sector_len: 4096,
            num_blocks: 2,
            ident0: 0x89,
            ident1: 0x18,
            ident2: 0,
            ident3: 0,
            ..PFlashCfi01Config::default()
        },
    )
    .unwrap()
}

fn make_test_aspace() -> (AddressSpace, SysBus) {
    let root = MemoryRegion::container("root", 0x1_0000_0000);
    let aspace = AddressSpace::new(root);
    let bus = SysBus::new("sysbus");
    (aspace, bus)
}

#[test]
fn test_pflash_cfi01_read_array_cfi_and_id_modes() {
    let dev = cfi01(vec![0xaa; 8192]);

    assert_eq!(dev.do_read(0x10, 1), 0xaa);

    dev.do_write(0, 1, 0x98);
    assert_eq!(dev.do_read(0x10, 1), u64::from(b'Q'));
    assert_eq!(dev.do_read(0x11, 1), u64::from(b'R'));
    assert_eq!(dev.do_read(0x12, 1), u64::from(b'Y'));

    dev.do_write(0, 1, 0xff);
    assert_eq!(dev.do_read(0x10, 1), 0xaa);

    dev.do_write(0, 1, 0x90);
    assert_eq!(dev.do_read(0, 1), 0x89);
    assert_eq!(dev.do_read(1, 1), 0x18);
}

#[test]
fn test_pflash_cfi01_cfi_query_resets_on_f0() {
    let dev = cfi01(vec![0xaa; 8192]);

    dev.do_write(0, 1, 0x98);
    assert_eq!(dev.do_read(0x10, 1), u64::from(b'Q'));

    dev.do_write(0, 1, 0xf0);

    assert_eq!(dev.do_read(0x10, 1), 0xaa);
}

#[test]
fn test_pflash_cfi01_mmio_combines_wide_cfi_and_id_reads() {
    let dev = Arc::new(cfi01_x8(vec![0xff; 8192]));
    let mmio = PFlashCfi01Mmio(Arc::clone(&dev));

    mmio.write(0, 1, 0x98);
    assert_eq!(mmio.read(0x10, 4), 0x0159_5251);

    mmio.write(0, 1, 0xff);
    mmio.write(0, 1, 0x90);
    assert_eq!(mmio.read(0, 4), 0x0000_1889);
}

#[test]
fn test_pflash_cfi01_wide_mmio_write_splits_into_32bit_callbacks() {
    let dev = Arc::new(cfi01_x8(vec![0xff; 8192]));
    let mmio = PFlashCfi01Mmio(Arc::clone(&dev));

    mmio.write(0, 8, 0x0000_000f_0000_0040);
    mmio.write(0, 1, 0xff);

    assert_eq!(mmio.read(4, 4), 0x0000_000f);
}

#[test]
fn test_pflash_cfi01_lifecycle_and_mom_identity() {
    let dev = Arc::new(cfi01_x8(vec![0xff; 8192]));
    assert!(!dev.realized());
    dev.with_mdevice(|device| assert_eq!(device.local_id(), "pflash-cfi01"));
    assert_eq!(dev.object_info().local_id, "pflash-cfi01");

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA::new(0x2000_0000);

    dev.attach_to_bus(&mut bus).unwrap();
    dev.register_mmio(
        MemoryRegion::io(
            "pflash-cfi01-0",
            8192,
            Arc::new(PFlashCfi01Mmio(Arc::clone(&dev))),
        ),
        base,
    )
    .unwrap();

    dev.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(dev.realized());
    assert_eq!(aspace.read(base, 1), 0xff);

    let err = dev.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(
        err.to_string().contains("already realized"),
        "unexpected second-realize error: {err}"
    );

    dev.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!dev.realized());
    assert_eq!(aspace.read(base, 1), 0);
}

#[test]
fn test_pflash_cfi01_mmio_big_endian_combines_wide_cfi_and_id_reads() {
    let dev = Arc::new(
        PFlashCfi01::new(
            flash(vec![0xff; 8192], 4096),
            PFlashCfi01Config {
                bank_width: 1,
                device_width: 1,
                max_device_width: 1,
                sector_len: 4096,
                num_blocks: 2,
                big_endian: true,
                ident0: 0x89,
                ident1: 0x18,
                ident2: 0,
                ident3: 0,
                ..PFlashCfi01Config::default()
            },
        )
        .unwrap(),
    );
    let mmio = PFlashCfi01Mmio(Arc::clone(&dev));

    mmio.write(0, 1, 0x98);
    assert_eq!(mmio.read(0x10, 4), 0x5152_5901);

    mmio.write(0, 1, 0xff);
    mmio.write(0, 1, 0x90);
    assert_eq!(mmio.read(0, 4), 0x8918_0000);
}

#[test]
fn test_pflash_cfi01_legacy_status_wide_reads_match_bank_layout() {
    let dev = cfi01(vec![0xff; 8192]);

    dev.do_write(0, 1, 0x70);

    assert_eq!(dev.do_read(0, 2), 0x80);
    assert_eq!(dev.do_read(0, 4), 0x0080_0080);
}

#[test]
fn test_pflash_cfi01_wide_mmio_read_splits_into_32bit_callbacks() {
    let dev = Arc::new(cfi01(vec![0xff; 8192]));
    let mmio = PFlashCfi01Mmio(Arc::clone(&dev));

    mmio.write(0, 1, 0x70);

    assert_eq!(mmio.read(0, 8), 0x0080_0080_0080_0080);
}

#[test]
fn test_pflash_cfi01_rejects_non_power_of_two_device_width() {
    let result = PFlashCfi01::new(
        flash(vec![0xff; 8192], 4096),
        PFlashCfi01Config {
            bank_width: 4,
            device_width: 3,
            max_device_width: 4,
            sector_len: 4096,
            num_blocks: 2,
            ..PFlashCfi01Config::default()
        },
    );

    assert!(result.is_err());
}

#[test]
fn test_pflash_cfi01_rejects_non_power_of_two_max_device_width() {
    let result = PFlashCfi01::new(
        flash(vec![0xff; 8192], 4096),
        PFlashCfi01Config {
            bank_width: 4,
            device_width: 1,
            max_device_width: 3,
            sector_len: 4096,
            num_blocks: 2,
            ..PFlashCfi01Config::default()
        },
    );

    assert!(result.is_err());
}

#[test]
fn test_pflash_cfi01_program_and_sector_erase() {
    let dev = cfi01(vec![0xff; 8192]);

    dev.do_write(0, 1, 0x40);
    dev.do_write(0x20, 1, 0x0f);
    assert_eq!(dev.do_read(0, 1), 0x80);

    dev.do_write(0, 1, 0xff);
    assert_eq!(dev.do_read(0x20, 1), 0x0f);

    dev.do_write(0x20, 1, 0x20);
    assert_eq!(dev.do_read(0, 1), 0x80);
    dev.do_write(0x20, 1, 0xd0);
    dev.do_write(0, 1, 0xff);

    assert_eq!(dev.do_read(0x20, 1), 0xff);
}

#[test]
fn test_pflash_cfi01_sector_erase_waits_for_confirm() {
    let dev = cfi01(vec![0xff; 8192]);

    dev.do_write(0, 1, 0x40);
    dev.do_write(0x20, 1, 0x0f);
    dev.do_write(0, 1, 0xff);
    assert_eq!(dev.do_read(0x20, 1), 0x0f);

    dev.do_write(0x20, 1, 0x20);
    dev.do_write(0, 1, 0xff);
    assert_eq!(dev.do_read(0x20, 1), 0x0f);

    dev.do_write(0x20, 1, 0x20);
    dev.do_write(0x20, 1, 0xd0);
    dev.do_write(0, 1, 0xff);
    assert_eq!(dev.do_read(0x20, 1), 0xff);
}

#[test]
fn test_pflash_cfi01_clear_status_preserves_ready_bit() {
    let dev = cfi01(vec![0xff; 8192]);

    dev.do_write(0, 1, 0x40);
    dev.do_write(0x3000, 1, 0x0f);
    assert_eq!(dev.do_read(0, 1), 0x90);

    dev.do_write(0, 1, 0x50);
    dev.do_write(0, 1, 0x70);

    assert_eq!(dev.do_read(0, 1), 0x80);
}

#[test]
fn test_pflash_cfi01_buffered_write_commits_only_after_confirm() {
    let dev = cfi01(vec![0xff; 8192]);

    dev.do_write(0x100, 1, 0xe8);
    assert_eq!(dev.do_read(0, 1), 0x80);
    dev.do_write(0x100, 1, 1);
    dev.do_write(0x100, 1, 0xaa);
    dev.do_write(0x101, 1, 0x55);
    dev.do_write(0x100, 1, 0xd0);

    dev.do_write(0, 1, 0xff);
    assert_eq!(dev.do_read(0x100, 1), 0xaa);
    assert_eq!(dev.do_read(0x101, 1), 0x55);
}

#[test]
fn test_pflash_cfi01_buffered_write_count_zero_writes_one_unit() {
    let dev = cfi01(vec![0xff; 8192]);

    dev.do_write(0x100, 1, 0xe8);
    dev.do_write(0x100, 1, 0);
    dev.do_write(0x100, 1, 0xaa);
    dev.do_write(0x100, 1, 0xd0);

    dev.do_write(0, 1, 0xff);
    assert_eq!(dev.do_read(0x100, 1), 0xaa);
}

#[test]
fn test_pflash_cfi01_buffered_write_aborts_on_bad_confirm() {
    let dev = cfi01(vec![0xff; 8192]);

    dev.do_write(0x100, 1, 0xe8);
    dev.do_write(0x100, 1, 0);
    dev.do_write(0x100, 1, 0xaa);
    dev.do_write(0x100, 1, 0x00);

    dev.do_write(0, 1, 0xff);
    assert_eq!(dev.do_read(0x100, 1), 0xff);
}

#[test]
fn test_pflash_cfi02_cfi_and_autoselect_modes() {
    let dev = cfi02(vec![0xaa; 8192]);

    assert_eq!(dev.do_read(0x20, 1), 0xaa);

    dev.do_write(0x55, 1, 0x98);
    assert_eq!(dev.do_read(0x10, 1), u64::from(b'Q'));
    assert_eq!(dev.do_read(0x11, 1), u64::from(b'R'));
    assert_eq!(dev.do_read(0x12, 1), u64::from(b'Y'));

    dev.do_write(0, 1, 0xf0);
    assert_eq!(dev.do_read(0x20, 1), 0xaa);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x90);
    assert_eq!(dev.do_read(0, 1), 0x01);
    assert_eq!(dev.do_read(1, 1), 0x7e);
    assert_eq!(dev.do_read(0x0e, 1), 0x22);
    assert_eq!(dev.do_read(0x0f, 1), 0x21);
}

#[test]
fn test_pflash_cfi02_default_extended_ids_fall_back_to_array_data() {
    let mut data = vec![0xaa; 8192];
    data[0x0e] = 0x5a;
    data[0x0f] = 0xa5;
    let dev = PFlashCfi02::new(
        flash(data, 4096),
        PFlashCfi02Config {
            sector_len: 4096,
            num_blocks: 2,
            ident0: 0x01,
            ident1: 0x7e,
            ..PFlashCfi02Config::default()
        },
    )
    .unwrap();

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x90);

    assert_eq!(dev.do_read(0x0e, 1), 0x5a);
    assert_eq!(dev.do_read(0x0f, 1), 0xa5);
}

#[test]
fn test_pflash_cfi02_non_power_of_two_length_keeps_in_range_offsets() {
    let mut data = vec![0xaa; 6000];
    data[0x90] = 0x5a;
    let dev = PFlashCfi02::new(
        flash(data, 3000),
        PFlashCfi02Config {
            sector_len: 3000,
            num_blocks: 2,
            ..PFlashCfi02Config::default()
        },
    )
    .unwrap();

    assert_eq!(dev.do_read(0x90, 1), 0x5a);
}

#[test]
fn test_pflash_cfi02_masks_unlock_addresses_to_11_bits() {
    let dev = PFlashCfi02::new(
        flash(vec![0xff; 8192], 4096),
        PFlashCfi02Config {
            sector_len: 4096,
            num_blocks: 2,
            ident0: 0x01,
            ident1: 0x7e,
            unlock_addr0: 0x0d55,
            unlock_addr1: 0x0aaa,
            ..PFlashCfi02Config::default()
        },
    )
    .unwrap();

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0xa0);
    dev.do_write(0x20, 1, 0x0f);

    assert_eq!(dev.do_read(0x20, 1), 0x0f);
}

#[test]
fn test_pflash_cfi02_mmio_autoselect_cfi_returns_to_id_mode() {
    let dev = Arc::new(cfi02(vec![0xaa; 8192]));
    let mmio = PFlashCfi02Mmio(Arc::clone(&dev));

    mmio.write(0x555, 1, 0xaa);
    mmio.write(0x2aa, 1, 0x55);
    mmio.write(0x555, 1, 0x90);
    assert_eq!(mmio.read(0, 1), 0x01);

    mmio.write(0x55, 1, 0x98);
    assert_eq!(mmio.read(0x10, 1), u64::from(b'Q'));
    assert_eq!(mmio.read(0x11, 1), u64::from(b'R'));
    assert_eq!(mmio.read(0x12, 1), u64::from(b'Y'));

    mmio.write(0, 1, 0xf0);
    assert_eq!(mmio.read(0, 1), 0x01);
    assert_eq!(mmio.read(1, 1), 0x7e);
}

#[test]
fn test_pflash_cfi02_mmio_combines_wide_cfi_reads() {
    let dev = Arc::new(cfi02(vec![0xff; 8192]));
    let mmio = PFlashCfi02Mmio(Arc::clone(&dev));

    mmio.write(0x55, 1, 0x98);

    assert_eq!(mmio.read(0x10, 4), 0x0259_5251);
}

#[test]
fn test_pflash_cfi02_mmio_combines_wide_id_reads() {
    let dev = Arc::new(cfi02(vec![0xaa; 8192]));
    let mmio = PFlashCfi02Mmio(Arc::clone(&dev));

    mmio.write(0x555, 1, 0xaa);
    mmio.write(0x2aa, 1, 0x55);
    mmio.write(0x555, 1, 0x90);

    assert_eq!(mmio.read(0, 4), 0xaa00_7e01);
}

#[test]
fn test_pflash_cfi02_lifecycle_and_mom_identity() {
    let dev = Arc::new(cfi02(vec![0xff; 8192]));
    assert!(!dev.realized());
    dev.with_mdevice(|device| assert_eq!(device.local_id(), "pflash-cfi02"));
    assert_eq!(dev.object_info().local_id, "pflash-cfi02");

    let (mut aspace, mut bus) = make_test_aspace();
    let base = GPA::new(0x2001_0000);

    dev.attach_to_bus(&mut bus).unwrap();
    dev.register_mmio(
        MemoryRegion::io(
            "pflash-cfi02-0",
            8192,
            Arc::new(PFlashCfi02Mmio(Arc::clone(&dev))),
        ),
        base,
    )
    .unwrap();

    dev.realize_onto(&mut bus, &mut aspace).unwrap();
    assert!(dev.realized());
    assert_eq!(aspace.read(base, 1), 0xff);

    let err = dev.realize_onto(&mut bus, &mut aspace).unwrap_err();
    assert!(
        err.to_string().contains("already realized"),
        "unexpected second-realize error: {err}"
    );

    dev.unrealize_from(&mut bus, &mut aspace).unwrap();
    assert!(!dev.realized());
    assert_eq!(aspace.read(base, 1), 0);
}

#[test]
fn test_pflash_cfi02_rejects_accesses_wider_than_u32() {
    let dev = cfi02(vec![0xff; 8192]);

    assert_eq!(dev.do_read(0, 8), 0);

    dev.do_write(0x555, 8, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0xa0);
    dev.do_write(0x20, 1, 0x0f);

    assert_eq!(dev.do_read(0x20, 1), 0xff);
}

#[test]
fn test_pflash_cfi02_rejects_width_wider_than_u32() {
    let result = PFlashCfi02::new(
        flash(vec![0xff; 8192], 4096),
        PFlashCfi02Config {
            width: 8,
            sector_len: 4096,
            num_blocks: 2,
            ..PFlashCfi02Config::default()
        },
    );

    assert!(result.is_err());
}

#[test]
fn test_pflash_cfi02_unlock_bypass_programs_multiple_bytes_and_resets() {
    let dev = cfi02(vec![0xff; 8192]);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x20);

    dev.do_write(0, 1, 0xa0);
    dev.do_write(0x20, 1, 0xaa);
    dev.do_write(0, 1, 0xa0);
    dev.do_write(0x21, 1, 0x55);

    assert_eq!(dev.do_read(0x20, 1), 0xaa);
    assert_eq!(dev.do_read(0x21, 1), 0x55);

    dev.do_write(0, 1, 0x90);
    dev.do_write(0, 1, 0x00);
    dev.do_write(0, 1, 0xa0);
    dev.do_write(0x22, 1, 0x0f);

    assert_eq!(dev.do_read(0x22, 1), 0xff);
}

#[test]
fn test_pflash_cfi02_runtime_reset_clears_unlock_bypass() {
    let dev = cfi02(vec![0xff; 8192]);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x20);

    dev.reset_runtime();

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x20, 1, 0xa0);
    dev.do_write(0x20, 1, 0x0f);

    assert_eq!(dev.do_read(0x20, 1), 0xff);
}

#[test]
fn test_pflash_cfi02_runtime_reset_clears_status() {
    let dev = cfi02(vec![0xff; 8192]);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0xa0);
    dev.do_write(0x20, 1, 0x0f);

    dev.reset_runtime();

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0xa0);

    assert_eq!(dev.do_read(0, 1), 0x40);
}

#[test]
fn test_pflash_cfi02_program_and_erase_sequences() {
    let dev = cfi02(vec![0xff; 8192]);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0xa0);
    dev.do_write(0x20, 1, 0x0f);
    assert_eq!(dev.do_read(0x20, 1), 0x0f);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x80);
    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x20, 1, 0x30);

    assert_eq!(dev.do_read(0x20, 1), 0x44);
    dev.reset_runtime();
    assert_eq!(dev.do_read(0x20, 1), 0xff);
}

#[test]
fn test_pflash_cfi02_sector_erase_reports_status_until_reset() {
    let dev = cfi02(vec![0x00; 8192]);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x80);
    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x20, 1, 0x30);

    assert_eq!(dev.do_read(0x20, 1), 0x44);
    assert_eq!(dev.do_read(0x20, 1), 0x00);

    dev.reset_runtime();
    assert_eq!(dev.do_read(0x20, 1), 0xff);
    assert_eq!(dev.do_read(0x1020, 1), 0x00);
}

#[test]
fn test_pflash_cfi02_sector_erase_reset_allows_program() {
    let dev = cfi02(vec![0x00; 8192]);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x80);
    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x20, 1, 0x30);
    dev.do_write(0, 1, 0xf0);

    assert_eq!(dev.do_read(0x20, 1), 0xff);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0xa0);
    dev.do_write(0x20, 1, 0x0f);

    assert_eq!(dev.do_read(0x20, 1), 0x0f);
}

#[test]
fn test_pflash_cfi02_sector_erase_timer_completion_returns_to_array() {
    let dev = cfi02(vec![0x00; 8192]);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x80);
    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x20, 1, 0x30);
    assert_eq!(dev.do_read(0x20, 1), 0x44);

    dev.expire_timer();

    assert_eq!(dev.do_read(0x20, 1), 0xff);
    assert_eq!(dev.do_read(0x1020, 1), 0x00);
}

#[test]
fn test_pflash_cfi02_sector_erase_suspend_reads_array_and_resumes() {
    let mut data = vec![0x00; 8192];
    data[0x1020] = 0x5a;
    let dev = cfi02(data);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x80);
    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x20, 1, 0x30);

    dev.do_write(0, 1, 0xb0);
    assert_eq!(dev.do_read(0x1020, 1), 0x5a);
    assert_eq!(dev.do_read(0x20, 1), 0x04);

    dev.do_write(0, 1, 0x30);
    assert_eq!(dev.do_read(0x20, 1), 0x48);

    dev.reset_runtime();
    assert_eq!(dev.do_read(0x20, 1), 0xff);
    assert_eq!(dev.do_read(0x1020, 1), 0x5a);
}

#[test]
fn test_pflash_cfi02_suspend_tracks_multiple_erasing_sectors() {
    let dev = cfi02(vec![0x00; 8192]);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x80);
    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x20, 1, 0x30);
    dev.do_write(0x1020, 1, 0x30);

    dev.do_write(0, 1, 0xb0);

    assert_eq!(dev.do_read(0x20, 1), 0x04);
    assert_eq!(dev.do_read(0x1020, 1), 0x00);
}

#[test]
fn test_pflash_cfi02_suspend_rejects_new_sector_erase() {
    let mut data = vec![0x00; 8192];
    data[0x1020] = 0x5a;
    let dev = cfi02(data);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x80);
    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x20, 1, 0x30);
    dev.do_write(0, 1, 0xb0);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x80);
    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x1020, 1, 0x30);

    assert_eq!(dev.do_read(0x1020, 1), 0x5a);
}

#[test]
fn test_pflash_cfi02_chip_erase_reports_status_until_reset() {
    let dev = cfi02(vec![0x00; 8192]);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x80);
    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x10);

    assert_eq!(dev.do_read(0, 1), 0x40);
    dev.do_write(0, 1, 0xf0);
    assert_eq!(dev.do_read(0, 1), 0x00);

    dev.reset_runtime();
    assert_eq!(dev.do_read(0x20, 1), 0xff);
    assert_eq!(dev.do_read(0x1020, 1), 0xff);
}

#[test]
fn test_pflash_cfi02_chip_erase_timer_completion_returns_to_array() {
    let dev = cfi02(vec![0x00; 8192]);

    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x80);
    dev.do_write(0x555, 1, 0xaa);
    dev.do_write(0x2aa, 1, 0x55);
    dev.do_write(0x555, 1, 0x10);
    assert_eq!(dev.do_read(0, 1), 0x40);

    dev.expire_timer();

    assert_eq!(dev.do_read(0x20, 1), 0xff);
    assert_eq!(dev.do_read(0x1020, 1), 0xff);
}
