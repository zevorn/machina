use std::io::Write;

use machina_core::address::GPA;
use machina_core::machine::{Machine, MachineOpts};
use machina_guest_loongarch::loongarch::csr::{
    CRMD_DA, CRMD_IE, CRMD_PG, CSR_CRMD,
};
use machina_hw_loongarch::boot::KERNEL_ENTRY_DEFAULT;
use machina_hw_loongarch::virt_machine::{
    LoongArchVirtMachine, VIRT_EIOINTC_BASE, VIRT_EIOINTC_SIZE, VIRT_IPI_BASE,
    VIRT_IPI_SIZE, VIRT_PCH_PIC_BASE, VIRT_PCH_PIC_SIZE, VIRT_RAM_BASE,
    VIRT_UART_BASE, VIRT_UART_SIZE, VIRT_VIRTIO_BASE, VIRT_VIRTIO_SIZE,
};

const EFI_SYSTEM_TABLE_SIGNATURE: u64 = 0x5453_5953_2049_4249;
const EFI_SYSTEM_TABLE_HEADER_SIZE: u32 = 120;
const EFI_CONFIG_TABLE_SIZE: u64 = 24;
const EFI_MEMORY_DESCRIPTOR_SIZE: u64 = 40;
const EFI_CONVENTIONAL_MEMORY: u32 = 7;

const DEVICE_TREE_GUID: [u8; 16] = [
    0xd5, 0x21, 0xb6, 0xb1, 0x9c, 0xf1, 0xa5, 0x41, 0x83, 0x0b, 0xd9, 0x15,
    0x2c, 0x69, 0xaa, 0xe0,
];
const LINUX_EFI_BOOT_MEMMAP_GUID: [u8; 16] = [
    0x3f, 0x68, 0x0f, 0x80, 0x8b, 0xd0, 0x3a, 0x42, 0xa2, 0x93, 0x96, 0x5c,
    0x3c, 0x6f, 0xe2, 0xb4,
];
const LINUX_EFI_INITRD_MEDIA_GUID: [u8; 16] = [
    0x27, 0xe4, 0x68, 0x55, 0xfc, 0x68, 0x3d, 0x4f, 0xac, 0x74, 0xca, 0x55,
    0x52, 0x31, 0xcc, 0x68,
];

fn default_opts() -> MachineOpts {
    MachineOpts {
        ram_size: 64 * 1024 * 1024,
        cpu_count: 1,
        kernel: None,
        bios: None,
        bios_builtin: false,
        append: None,
        nographic: false,
        drive: None,
        initrd: None,
        netdev: None,
    }
}

fn build_minimal_elf(entry: u64, p_paddr: u64, payload: &[u8]) -> Vec<u8> {
    let mut elf = vec![0u8; 64 + 56];

    elf[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
    elf[4] = 2;
    elf[5] = 1;
    elf[6] = 1;
    elf[16..18].copy_from_slice(&2u16.to_le_bytes());
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    elf[24..32].copy_from_slice(&entry.to_le_bytes());
    elf[32..40].copy_from_slice(&64u64.to_le_bytes());
    elf[52..54].copy_from_slice(&64u16.to_le_bytes());
    elf[54..56].copy_from_slice(&56u16.to_le_bytes());
    elf[56..58].copy_from_slice(&1u16.to_le_bytes());

    let ph = 64usize;
    let p_offset = 120u64;
    let p_filesz = payload.len() as u64;
    let p_memsz = p_filesz + 8;
    elf[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
    elf[ph + 8..ph + 16].copy_from_slice(&p_offset.to_le_bytes());
    elf[ph + 16..ph + 24].copy_from_slice(&p_paddr.to_le_bytes());
    elf[ph + 24..ph + 32].copy_from_slice(&p_paddr.to_le_bytes());
    elf[ph + 32..ph + 40].copy_from_slice(&p_filesz.to_le_bytes());
    elf[ph + 40..ph + 48].copy_from_slice(&p_memsz.to_le_bytes());

    elf.extend_from_slice(payload);
    elf
}

fn build_linux_image(entry: u64, load_offset: u64, payload: &[u8]) -> Vec<u8> {
    let mut image = vec![0u8; 64];
    image[0..2].copy_from_slice(&0x5a4du16.to_le_bytes());
    image[8..16].copy_from_slice(&entry.to_le_bytes());
    image[16..24].copy_from_slice(&((64 + payload.len()) as u64).to_le_bytes());
    image[24..32].copy_from_slice(&load_offset.to_le_bytes());
    image[56..60].copy_from_slice(&0x8182_23cdu32.to_le_bytes());
    image.extend_from_slice(payload);
    image
}

fn read_bytes(
    machine: &LoongArchVirtMachine,
    addr: u64,
    len: usize,
) -> Vec<u8> {
    (0..len)
        .map(|offset| {
            machine
                .address_space()
                .read(GPA::new(addr + offset as u64), 1) as u8
        })
        .collect()
}

fn read_guest_u32(machine: &LoongArchVirtMachine, addr: u64) -> u32 {
    u32::from_le_bytes(read_bytes(machine, addr, 4).try_into().unwrap())
}

fn read_guest_be_u32(machine: &LoongArchVirtMachine, addr: u64) -> u32 {
    u32::from_be_bytes(read_bytes(machine, addr, 4).try_into().unwrap())
}

fn read_guest_u64(machine: &LoongArchVirtMachine, addr: u64) -> u64 {
    u64::from_le_bytes(read_bytes(machine, addr, 8).try_into().unwrap())
}

fn read_be_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
}

#[derive(Debug)]
struct FdtProp {
    path: String,
    name: String,
    data: Vec<u8>,
}

fn read_cstr(data: &[u8], offset: usize) -> String {
    let end = data[offset..]
        .iter()
        .position(|b| *b == 0)
        .map(|pos| offset + pos)
        .unwrap();
    String::from_utf8(data[offset..end].to_vec()).unwrap()
}

fn parse_fdt_props(blob: &[u8]) -> Vec<FdtProp> {
    assert_eq!(read_be_u32(blob, 0), 0xd00d_feed);
    let off_struct = read_be_u32(blob, 8) as usize;
    let off_strings = read_be_u32(blob, 12) as usize;
    let size_strings = read_be_u32(blob, 32) as usize;
    let strings = &blob[off_strings..off_strings + size_strings];

    let mut props = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut offset = off_struct;
    loop {
        let token = read_be_u32(blob, offset);
        offset += 4;
        match token {
            1 => {
                let name = read_cstr(blob, offset);
                offset += name.len() + 1;
                offset = (offset + 3) & !3;
                if name.is_empty() {
                    stack.clear();
                } else {
                    stack.push(name);
                }
            }
            2 => {
                stack.pop();
            }
            3 => {
                let len = read_be_u32(blob, offset) as usize;
                let nameoff = read_be_u32(blob, offset + 4) as usize;
                offset += 8;
                let data = blob[offset..offset + len].to_vec();
                offset = (offset + len + 3) & !3;
                let path = if stack.is_empty() {
                    "/".to_string()
                } else {
                    format!("/{}", stack.join("/"))
                };
                props.push(FdtProp {
                    path,
                    name: read_cstr(strings, nameoff),
                    data,
                });
            }
            9 => break,
            other => panic!("unexpected FDT token {other:#x} at {offset:#x}"),
        }
    }
    props
}

fn fdt_prop<'a>(props: &'a [FdtProp], path: &str, name: &str) -> &'a [u8] {
    props
        .iter()
        .find(|prop| prop.path == path && prop.name == name)
        .unwrap_or_else(|| panic!("missing FDT property {path}:{name}"))
        .data
        .as_slice()
}

fn assert_fdt_string(props: &[FdtProp], path: &str, name: &str, value: &str) {
    let mut expected = value.as_bytes().to_vec();
    expected.push(0);
    assert_eq!(fdt_prop(props, path, name), expected.as_slice());
}

fn cells_for_pairs(pairs: &[(u64, u64)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (base, size) in pairs {
        out.extend_from_slice(&((*base >> 32) as u32).to_be_bytes());
        out.extend_from_slice(&((*base & 0xffff_ffff) as u32).to_be_bytes());
        out.extend_from_slice(&((*size >> 32) as u32).to_be_bytes());
        out.extend_from_slice(&((*size & 0xffff_ffff) as u32).to_be_bytes());
    }
    out
}

fn fdt_u64_prop(props: &[FdtProp], path: &str, name: &str) -> u64 {
    u64::from_be_bytes(fdt_prop(props, path, name).try_into().unwrap())
}

fn config_table_ptr(
    machine: &LoongArchVirtMachine,
    system_table_addr: u64,
    guid: [u8; 16],
) -> u64 {
    assert_eq!(
        read_guest_u64(machine, system_table_addr),
        EFI_SYSTEM_TABLE_SIGNATURE
    );
    assert_eq!(
        read_guest_u32(machine, system_table_addr + 12),
        EFI_SYSTEM_TABLE_HEADER_SIZE
    );
    let nr_tables = read_guest_u64(machine, system_table_addr + 104);
    let tables = read_guest_u64(machine, system_table_addr + 112);
    for index in 0..nr_tables {
        let entry = tables + index * EFI_CONFIG_TABLE_SIZE;
        if read_bytes(machine, entry, 16) == guid {
            return read_guest_u64(machine, entry + 16);
        }
    }
    panic!("missing EFI config table GUID {guid:02x?}");
}

fn boot_minimal_elf(
    opts: &mut MachineOpts,
) -> (LoongArchVirtMachine, tempfile::NamedTempFile) {
    let entry = VIRT_RAM_BASE + 0x20_0000;
    let segment_addr = VIRT_RAM_BASE + 0x30_0000;
    let elf = build_minimal_elf(entry, segment_addr, &[0x13, 0x57, 0x9b]);
    let mut kernel = tempfile::NamedTempFile::new().unwrap();
    kernel.write_all(&elf).unwrap();
    opts.kernel = Some(kernel.path().to_path_buf());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(opts).expect("init loongarch virt");
    machine.boot().expect("boot direct ELF");
    (machine, kernel)
}

#[test]
fn task43_direct_boot_loads_linux_image_header_at_load_offset() {
    let load_offset = 0x20_0000;
    let entry = load_offset;
    let payload = [0x13, 0x57, 0x9b, 0xdf];
    let image = build_linux_image(entry, load_offset, &payload);

    let mut kernel = tempfile::NamedTempFile::new().unwrap();
    kernel.write_all(&image).unwrap();

    let mut opts = default_opts();
    opts.kernel = Some(kernel.path().to_path_buf());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(&opts).expect("init loongarch virt");
    machine.boot().expect("boot LoongArch Linux Image");

    let load_addr = VIRT_RAM_BASE + load_offset;
    assert_eq!(read_bytes(&machine, load_addr, image.len()), image);
    assert_eq!(
        read_bytes(&machine, VIRT_RAM_BASE, payload.len()),
        vec![0; payload.len()]
    );

    let cpu = machine.cpu();
    let cpu = cpu.lock().unwrap();
    assert_eq!(cpu.pc(), VIRT_RAM_BASE + entry);
    assert_eq!(cpu.read_gpr(4), 1);
    assert_ne!(cpu.read_gpr(5), 0);
    assert_ne!(cpu.read_gpr(6), 0);
}

#[test]
fn task44_direct_boot_builds_efi_system_table_and_fdt() {
    let mut opts = default_opts();
    opts.append = Some("console=ttyS0 rdinit=/init".to_string());
    let (machine, _kernel) = boot_minimal_elf(&mut opts);

    let cpu = machine.cpu();
    let cpu = cpu.lock().unwrap();
    assert_eq!(cpu.read_gpr(4), 1);
    let cmdline_addr = cpu.read_gpr(5);
    let system_table_addr = cpu.read_gpr(6);
    drop(cpu);

    let mut expected_cmdline = opts.append.clone().unwrap().into_bytes();
    expected_cmdline.push(0);
    assert_eq!(
        read_bytes(&machine, cmdline_addr, expected_cmdline.len()),
        expected_cmdline
    );

    let memmap_addr = config_table_ptr(
        &machine,
        system_table_addr,
        LINUX_EFI_BOOT_MEMMAP_GUID,
    );
    assert_eq!(
        read_guest_u64(&machine, memmap_addr),
        EFI_MEMORY_DESCRIPTOR_SIZE
    );
    assert_eq!(
        read_guest_u64(&machine, memmap_addr + 8),
        EFI_MEMORY_DESCRIPTOR_SIZE
    );
    assert_eq!(read_guest_u32(&machine, memmap_addr + 16), 1);
    assert_eq!(
        read_guest_u32(&machine, memmap_addr + 40),
        EFI_CONVENTIONAL_MEMORY
    );
    assert_eq!(read_guest_u64(&machine, memmap_addr + 48), VIRT_RAM_BASE);
    assert_eq!(
        read_guest_u64(&machine, memmap_addr + 64),
        opts.ram_size / 4096
    );

    let fdt_addr =
        config_table_ptr(&machine, system_table_addr, DEVICE_TREE_GUID);
    let fdt_size = read_guest_be_u32(&machine, fdt_addr + 4) as usize;
    let fdt = read_bytes(&machine, fdt_addr, fdt_size);
    let props = parse_fdt_props(&fdt);

    assert_fdt_string(&props, "/", "compatible", "machina,loongarch64-virt");
    assert_fdt_string(
        &props,
        "/chosen",
        "bootargs",
        opts.append.as_ref().unwrap(),
    );
    assert_eq!(
        fdt_prop(&props, &format!("/memory@{VIRT_RAM_BASE:x}"), "reg"),
        cells_for_pairs(&[(VIRT_RAM_BASE, opts.ram_size)]).as_slice()
    );
    assert_eq!(
        fdt_prop(&props, "/cpus", "#address-cells"),
        1u32.to_be_bytes().as_slice()
    );
    assert_eq!(
        fdt_prop(&props, "/cpus", "#size-cells"),
        0u32.to_be_bytes().as_slice()
    );
    assert_fdt_string(&props, "/cpus/cpu@0", "device_type", "cpu");
    assert_fdt_string(&props, "/cpus/cpu@0", "compatible", "loongarch,la464");
    assert_eq!(
        fdt_prop(&props, "/cpus/cpu@0", "reg"),
        0u32.to_be_bytes().as_slice()
    );
    assert_fdt_string(
        &props,
        "/cpuic",
        "compatible",
        "loongson,cpu-interrupt-controller",
    );
    assert!(fdt_prop(&props, "/cpuic", "interrupt-controller").is_empty());
    assert_eq!(
        fdt_prop(&props, &format!("/ipi@{VIRT_IPI_BASE:x}"), "reg"),
        cells_for_pairs(&[(VIRT_IPI_BASE, VIRT_IPI_SIZE)]).as_slice()
    );
    assert_eq!(
        fdt_prop(&props, &format!("/eiointc@{VIRT_EIOINTC_BASE:x}"), "reg"),
        cells_for_pairs(&[(VIRT_EIOINTC_BASE, VIRT_EIOINTC_SIZE)]).as_slice()
    );
    assert_eq!(
        fdt_prop(&props, &format!("/platic@{VIRT_PCH_PIC_BASE:x}"), "reg"),
        cells_for_pairs(&[(VIRT_PCH_PIC_BASE, VIRT_PCH_PIC_SIZE)]).as_slice()
    );
    assert_eq!(
        fdt_prop(&props, &format!("/serial@{VIRT_UART_BASE:x}"), "reg"),
        cells_for_pairs(&[(VIRT_UART_BASE, VIRT_UART_SIZE)]).as_slice()
    );
    assert!(
        !props
            .iter()
            .any(|prop| prop.path
                == format!("/virtio_mmio@{VIRT_VIRTIO_BASE:x}")),
        "virtio node must not be emitted when no virtio device is present"
    );
}

#[test]
fn task44_direct_boot_adds_initrd_and_optional_virtio_to_fdt() {
    let mut drive = tempfile::NamedTempFile::new().unwrap();
    drive.write_all(&[0u8; 512]).unwrap();
    let initrd_bytes = [0xaa, 0xbb, 0xcc, 0xdd, 0xee];
    let mut initrd = tempfile::NamedTempFile::new().unwrap();
    initrd.write_all(&initrd_bytes).unwrap();

    let mut opts = default_opts();
    opts.drive = Some(drive.path().to_path_buf());
    opts.initrd = Some(initrd.path().to_path_buf());
    let (machine, _kernel) = boot_minimal_elf(&mut opts);

    let system_table_addr = machine.cpu().lock().unwrap().read_gpr(6);
    let initrd_table_addr = config_table_ptr(
        &machine,
        system_table_addr,
        LINUX_EFI_INITRD_MEDIA_GUID,
    );
    let initrd_start = read_guest_u64(&machine, initrd_table_addr);
    let initrd_size = read_guest_u64(&machine, initrd_table_addr + 8);
    assert_eq!(initrd_size, initrd_bytes.len() as u64);
    assert_eq!(
        read_bytes(&machine, initrd_start, initrd_bytes.len()),
        initrd_bytes
    );

    let fdt_addr =
        config_table_ptr(&machine, system_table_addr, DEVICE_TREE_GUID);
    let fdt_size = read_guest_be_u32(&machine, fdt_addr + 4) as usize;
    let props = parse_fdt_props(&read_bytes(&machine, fdt_addr, fdt_size));
    assert_eq!(
        fdt_u64_prop(&props, "/chosen", "linux,initrd-start"),
        initrd_start
    );
    assert_eq!(
        fdt_u64_prop(&props, "/chosen", "linux,initrd-end"),
        initrd_start + initrd_size
    );
    assert_eq!(
        fdt_prop(&props, &format!("/virtio_mmio@{VIRT_VIRTIO_BASE:x}"), "reg"),
        cells_for_pairs(&[(VIRT_VIRTIO_BASE, VIRT_VIRTIO_SIZE)]).as_slice()
    );
}

#[test]
fn task44_direct_boot_rejects_oversized_cmdline_boot_data() {
    let mut opts = default_opts();
    opts.append = Some("x".repeat(4096));
    let entry = VIRT_RAM_BASE + 0x20_0000;
    let elf = build_minimal_elf(entry, VIRT_RAM_BASE + 0x30_0000, &[0x11]);
    let mut kernel = tempfile::NamedTempFile::new().unwrap();
    kernel.write_all(&elf).unwrap();
    opts.kernel = Some(kernel.path().to_path_buf());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(&opts).expect("init loongarch virt");
    let err = machine.boot().expect_err("oversized cmdline must fail");
    assert!(
        err.to_string().contains("command line"),
        "unexpected error: {err}"
    );
}

#[test]
fn task44_direct_boot_rejects_kernel_overlapping_boot_data_window() {
    let mut opts = default_opts();
    opts.ram_size = 4 * 1024 * 1024;
    let boot_base = VIRT_RAM_BASE + opts.ram_size - 0x2_0000;
    let entry = VIRT_RAM_BASE + 0x20_0000;
    let elf = build_minimal_elf(entry, boot_base, &[0x5a, 0x6b, 0x7c]);
    let mut kernel = tempfile::NamedTempFile::new().unwrap();
    kernel.write_all(&elf).unwrap();
    opts.kernel = Some(kernel.path().to_path_buf());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(&opts).expect("init loongarch virt");
    let err = machine
        .boot()
        .expect_err("kernel overlapping boot data must fail");
    let err = err.to_string();
    assert!(
        err.contains("overlap") && err.contains("boot data"),
        "unexpected error: {err}"
    );
}

#[test]
fn task44_direct_boot_rejects_initrd_overlapping_raw_kernel() {
    let mut kernel = tempfile::NamedTempFile::new().unwrap();
    kernel.write_all(&vec![0x5a; 0x1_0000]).unwrap();
    let mut initrd = tempfile::NamedTempFile::new().unwrap();
    initrd.write_all(&vec![0xa5; 0x21_0000]).unwrap();

    let mut opts = default_opts();
    opts.ram_size = 0x23_0000;
    opts.kernel = Some(kernel.path().to_path_buf());
    opts.initrd = Some(initrd.path().to_path_buf());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(&opts).expect("init loongarch virt");
    let err = machine
        .boot()
        .expect_err("initrd overlapping raw kernel must fail");
    let err = err.to_string();
    assert!(
        err.contains("overlap") && err.contains("initrd"),
        "unexpected error: {err}"
    );
}

#[test]
fn task43_direct_boot_loads_elf_and_sets_initial_cpu_state() {
    let entry = VIRT_RAM_BASE + 0x20_0000;
    let segment_addr = VIRT_RAM_BASE + 0x30_0000;
    let payload = [0xde, 0xad, 0xbe, 0xef, 0xca];
    let elf = build_minimal_elf(entry, segment_addr, &payload);

    let mut kernel = tempfile::NamedTempFile::new().unwrap();
    kernel.write_all(&elf).unwrap();

    let mut opts = default_opts();
    opts.kernel = Some(kernel.path().to_path_buf());
    opts.append = Some("console=ttyS0 rdinit=/init".to_string());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(&opts).expect("init loongarch virt");
    for offset in payload.len()..payload.len() + 8 {
        machine.address_space().write(
            GPA::new(segment_addr + offset as u64),
            1,
            0xff,
        );
    }

    machine.boot().expect("boot direct ELF");

    assert_eq!(read_bytes(&machine, segment_addr, payload.len()), payload);
    assert_eq!(
        read_bytes(&machine, segment_addr + payload.len() as u64, 8),
        vec![0; 8],
        "ELF BSS must be zero-filled"
    );

    let cpu = machine.cpu();
    let cpu = cpu.lock().unwrap();
    assert_eq!(cpu.pc(), entry);
    let crmd = cpu.csr_read(CSR_CRMD);
    assert_eq!(crmd & 0x3, 0, "direct boot starts in PLV0");
    assert_ne!(crmd & CRMD_DA, 0, "direct boot must start in DA mode");
    assert_eq!(crmd & CRMD_PG, 0, "direct boot must leave paging disabled");
    assert_eq!(crmd & CRMD_IE, 0, "direct boot must leave interrupts off");
    assert_eq!(cpu.read_gpr(4), 1, "a0 must carry EFI boot mode");

    let cmdline_addr = cpu.read_gpr(5);
    let system_table_addr = cpu.read_gpr(6);
    let cmdline_len = opts.append.as_ref().unwrap().len() as u64 + 1;
    assert!(
        machine.address_space().is_mapped(GPA::new(cmdline_addr), 1)
            && machine
                .address_space()
                .is_mapped(GPA::new(cmdline_addr + cmdline_len - 1), 1),
        "a1 must point at the guest command line"
    );
    assert!(
        machine
            .address_space()
            .is_mapped(GPA::new(system_table_addr), 8),
        "a2 must point at guest boot-system-table storage"
    );
    drop(cpu);

    let mut expected_cmdline = opts.append.clone().unwrap().into_bytes();
    expected_cmdline.push(0);
    assert_eq!(
        read_bytes(&machine, cmdline_addr, expected_cmdline.len()),
        expected_cmdline
    );
}

#[test]
fn task43_direct_boot_loads_raw_image_at_default_entry() {
    let image = [0x11, 0x22, 0x33, 0x44, 0x55];
    let mut kernel = tempfile::NamedTempFile::new().unwrap();
    kernel.write_all(&image).unwrap();

    let mut opts = default_opts();
    opts.kernel = Some(kernel.path().to_path_buf());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(&opts).expect("init loongarch virt");
    machine.boot().expect("boot raw image");

    assert_eq!(
        read_bytes(&machine, KERNEL_ENTRY_DEFAULT, image.len()),
        image
    );
    assert_eq!(
        read_bytes(&machine, VIRT_RAM_BASE, image.len()),
        vec![0; image.len()]
    );
    let cpu = machine.cpu();
    let cpu = cpu.lock().unwrap();
    assert_eq!(cpu.pc(), KERNEL_ENTRY_DEFAULT);
    assert_eq!(cpu.read_gpr(4), 1);
    assert_ne!(cpu.read_gpr(5), 0);
    assert_ne!(cpu.read_gpr(6), 0);
}

#[test]
fn task43_direct_boot_rejects_bad_linux_image_magic() {
    let mut image = build_linux_image(0x20_0000, 0x20_0000, &[0xaa]);
    image[56..60].copy_from_slice(&0u32.to_le_bytes());
    let mut kernel = tempfile::NamedTempFile::new().unwrap();
    kernel.write_all(&image).unwrap();

    let mut opts = default_opts();
    opts.kernel = Some(kernel.path().to_path_buf());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(&opts).expect("init loongarch virt");
    let err = machine.boot().expect_err("bad Linux Image magic must fail");
    assert!(
        err.to_string().contains("LoongArch Linux Image"),
        "unexpected error: {err}"
    );
}

#[test]
fn task43_direct_boot_rejects_out_of_ram_raw_image() {
    let image = [0x66, 0x77, 0x88, 0x99];
    let mut kernel = tempfile::NamedTempFile::new().unwrap();
    kernel.write_all(&image).unwrap();

    let mut opts = default_opts();
    opts.ram_size = 1024 * 1024;
    opts.kernel = Some(kernel.path().to_path_buf());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(&opts).expect("init loongarch virt");
    let err = machine.boot().expect_err("out-of-RAM raw image must fail");
    assert!(
        err.to_string().contains("outside LoongArch RAM"),
        "unexpected error: {err}"
    );
}
