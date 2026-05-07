use machina_core::address::GPA;
use machina_core::machine::Machine;
use machina_guest_riscv::riscv::csr::PrivLevel;
use machina_hw_core::loader;

use crate::k230::{K230Machine, K230MemMap, K230_MEMMAP};
use crate::k230_dtb::fixup_k230_dtb;

pub const K230_BOOTROM_BASE: u64 =
    K230_MEMMAP[K230MemMap::Bootrom as usize].base;
pub const K230_BOOTROM_SIZE: u64 =
    K230_MEMMAP[K230MemMap::Bootrom as usize].size;

pub fn boot_k230(
    machine: &mut K230Machine,
) -> Result<(), Box<dyn std::error::Error>> {
    if machine.bios_builtin {
        return boot_k230_builtin(machine);
    }

    if machine.initrd_path().is_some() && machine.dtb_path().is_none() {
        return Err("-initrd requires -dtb for the k230 machine".into());
    }

    let start_addr = load_bios_or_kernel(machine)?;
    let initrd_range = load_initrd(machine)?;
    let fdt_addr = load_and_fix_user_dtb(machine, initrd_range)?;
    apply_loaders(machine)?;
    write_k230_reset_vec(machine, start_addr, fdt_addr);
    machine.set_boot_cpu_pc(K230_BOOTROM_BASE, PrivLevel::Machine);
    Ok(())
}

fn boot_k230_builtin(
    machine: &mut K230Machine,
) -> Result<(), Box<dyn std::error::Error>> {
    if machine.initrd_path().is_some() && machine.dtb_path().is_none() {
        return Err("-initrd requires -dtb for the k230 machine".into());
    }

    let entry = load_kernel_at_ddr(machine)?;
    let initrd_range = load_initrd(machine)?;
    let fdt_addr = load_and_fix_user_dtb(machine, initrd_range)?;
    apply_loaders(machine)?;
    write_k230_halt(machine);
    configure_builtin_sbi_entry(machine, entry, fdt_addr);
    Ok(())
}

fn load_kernel_at_ddr(
    machine: &K230Machine,
) -> Result<u64, Box<dyn std::error::Error>> {
    let Some(path) = machine.kernel_path() else {
        return Err("builtin K230 boot requires -kernel".into());
    };
    load_image_at_ddr(machine, path)
}

fn load_image_at_ddr(
    machine: &K230Machine,
    path: &std::path::Path,
) -> Result<u64, Box<dyn std::error::Error>> {
    let ddr = K230_MEMMAP[K230MemMap::Ddr as usize];
    let data = std::fs::read(path)?;
    if data.starts_with(b"\x7fELF") {
        let info = loader::load_elf(&data, ddr.base, machine.address_space())
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
        Ok(info.entry.0)
    } else {
        loader::load_binary(
            &data,
            GPA::new(ddr.base),
            machine.address_space(),
        )?;
        Ok(ddr.base)
    }
}

fn load_bios_or_kernel(
    machine: &K230Machine,
) -> Result<u64, Box<dyn std::error::Error>> {
    if let Some(path) = machine
        .bios_path()
        .filter(|path| path.to_str() != Some("none"))
    {
        return load_image_at_ddr(machine, path);
    }

    if let Some(path) = machine.kernel_path() {
        return load_image_at_ddr(machine, path);
    }

    let ddr = K230_MEMMAP[K230MemMap::Ddr as usize];
    Ok(ddr.base)
}

fn load_initrd(
    machine: &K230Machine,
) -> Result<Option<(u64, u64)>, Box<dyn std::error::Error>> {
    let Some(path) = machine.initrd_path() else {
        return Ok(None);
    };
    let data = std::fs::read(path)?;
    let start = 0x0a10_0000;
    let end = start + data.len() as u64;
    if end > machine.ram_size() {
        return Err("K230 initrd exceeds DDR".into());
    }
    loader::load_binary(&data, GPA::new(start), machine.address_space())?;
    Ok(Some((start, end)))
}

fn load_and_fix_user_dtb(
    machine: &mut K230Machine,
    initrd_range: Option<(u64, u64)>,
) -> Result<u64, Box<dyn std::error::Error>> {
    let Some(path) = machine.dtb_path() else {
        return Ok(0);
    };
    let blob = std::fs::read(path)?;
    let fixed = fixup_k230_dtb(&blob, initrd_range, machine.kernel_cmdline())?;
    let addr = place_dtb(machine, &fixed)?;
    machine.set_dtb_blob(fixed);
    Ok(addr)
}

fn apply_loaders(
    machine: &K230Machine,
) -> Result<(), Box<dyn std::error::Error>> {
    let ddr = K230_MEMMAP[K230MemMap::Ddr as usize];
    let ddr_end = ddr.base + machine.ram_size();
    for loader_spec in machine.loaders() {
        if !loader_spec.force_raw {
            return Err("k230 loader requires force-raw=on".into());
        }
        let data = std::fs::read(&loader_spec.file)?;
        let end = loader_spec
            .addr
            .checked_add(data.len() as u64)
            .ok_or("k230 loader range end overflows u64")?;
        if loader_spec.addr < ddr.base || end > ddr_end {
            return Err(format!(
                "k230 loader range {:#x}..{:#x} is outside DDR {:#x}..{:#x}",
                loader_spec.addr, end, ddr.base, ddr_end
            )
            .into());
        }
        loader::load_binary(
            &data,
            GPA::new(loader_spec.addr),
            machine.address_space(),
        )?;
    }
    Ok(())
}

fn place_dtb(
    machine: &K230Machine,
    blob: &[u8],
) -> Result<u64, Box<dyn std::error::Error>> {
    let ddr = K230_MEMMAP[K230MemMap::Ddr as usize];
    let len = blob.len() as u64;
    if len > machine.ram_size() {
        return Err("K230 DTB blob larger than DDR".into());
    }
    let margin = 0x1_0000u64;
    let offset = machine
        .ram_size()
        .checked_sub(margin + len)
        .ok_or("K230 DTB does not fit in DDR")?
        & !0x7;
    let addr = ddr.base + offset;
    loader::load_binary(blob, GPA::new(addr), machine.address_space())?;
    Ok(addr)
}

fn write_k230_reset_vec(machine: &K230Machine, start_addr: u64, fdt_addr: u64) {
    let reset_vec: [u32; 10] = [
        0x0000_0297,
        0x0282_8613,
        0xf140_2573,
        0x0202_b583,
        0x0182_b283,
        0x0002_8067,
        start_addr as u32,
        (start_addr >> 32) as u32,
        fdt_addr as u32,
        (fdt_addr >> 32) as u32,
    ];

    let bootrom = machine.bootrom_block();
    let ptr = bootrom.as_ptr();
    for (index, word) in reset_vec.iter().enumerate() {
        let bytes = word.to_le_bytes();
        unsafe {
            let dst = ptr.add(index * 4);
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, 4);
        }
    }
}

fn write_k230_halt(machine: &K230Machine) {
    let halt: u32 = 0x0000_006f;
    let bytes = halt.to_le_bytes();
    let bootrom = machine.bootrom_block();
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), bootrom.as_ptr(), 4);
    }
}

fn configure_builtin_sbi_entry(
    machine: &K230Machine,
    entry: u64,
    fdt_addr: u64,
) {
    let mut cpus = machine.cpus_lock();
    if let Some(Some(cpu)) = cpus.get_mut(0) {
        cpu.pc = entry;
        cpu.set_priv(PrivLevel::Supervisor);
        cpu.gpr[10] = 0;
        cpu.gpr[11] = fdt_addr;
        cpu.csr.mideleg = 0x0222;
        cpu.csr.medeleg = 0xb1ff;
        cpu.csr.mie = 1 << 7;
        cpu.csr.pmpaddr[0] = u64::MAX;
        cpu.csr.pmpcfg[0] = 0x1f;
        cpu.pmp.sync_from_csr(&cpu.csr.pmpcfg, &cpu.csr.pmpaddr);
    }
}
