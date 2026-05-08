use machina_core::address::GPA;
use machina_core::machine::Machine;
use machina_guest_riscv::riscv::csr::PrivLevel;
use machina_hw_core::loader;

use crate::boot::{
    self, BiosSource, DynamicInfo, K230_EMBEDDED_FW, K230_FW_FILENAME,
};
use crate::k230::{K230Machine, K230MemMap, K230_MEMMAP};
use crate::k230_dtb::{
    dtb_first_memory_region, fixup_k230_dtb, FdtMemoryRegion,
};

pub const K230_BOOTROM_BASE: u64 =
    K230_MEMMAP[K230MemMap::Bootrom as usize].base;
pub const K230_BOOTROM_SIZE: u64 =
    K230_MEMMAP[K230MemMap::Bootrom as usize].size;

struct LoadedImage {
    entry: u64,
    low_addr: u64,
    high_addr: u64,
}

struct LoadedFirmware {
    entry: u64,
    high_addr: u64,
    has_firmware: bool,
}

pub fn boot_k230(
    machine: &mut K230Machine,
) -> Result<(), Box<dyn std::error::Error>> {
    if machine.bios_builtin {
        return boot_k230_builtin(machine);
    }

    if machine.initrd_path().is_some() && machine.dtb_path().is_none() {
        return Err("-initrd requires -dtb for the k230 machine".into());
    }

    let linux_mem = load_linux_memory_window(machine)?;
    let firmware = load_firmware(machine)?;
    let kernel = load_kernel(machine, &firmware, linux_mem)?;
    let initrd_range = load_initrd(machine, kernel.as_ref(), linux_mem)?;
    let fdt_addr = load_and_fix_user_dtb(machine, initrd_range, linux_mem)?;
    apply_loaders(machine)?;
    write_k230_reset_vec(
        machine,
        firmware.entry,
        fdt_addr,
        kernel.as_ref().map_or(0, |image| image.entry),
        firmware.has_firmware,
    );
    machine.set_boot_cpu_pc(K230_BOOTROM_BASE, PrivLevel::Machine);
    Ok(())
}

fn boot_k230_builtin(
    machine: &mut K230Machine,
) -> Result<(), Box<dyn std::error::Error>> {
    if machine.initrd_path().is_some() && machine.dtb_path().is_none() {
        return Err("-initrd requires -dtb for the k230 machine".into());
    }

    let linux_mem = load_linux_memory_window(machine)?;
    let kernel = load_kernel_for_builtin(machine, linux_mem)?;
    let initrd_range = load_initrd(machine, Some(&kernel), linux_mem)?;
    let fdt_addr = load_and_fix_user_dtb(machine, initrd_range, linux_mem)?;
    apply_loaders(machine)?;
    write_k230_halt(machine);
    configure_builtin_sbi_entry(machine, kernel.entry, fdt_addr);
    Ok(())
}

fn load_kernel_for_builtin(
    machine: &K230Machine,
    linux_mem: Option<FdtMemoryRegion>,
) -> Result<LoadedImage, Box<dyn std::error::Error>> {
    let Some(path) = machine.kernel_path() else {
        return Err("builtin K230 boot requires -kernel".into());
    };
    let ddr = K230_MEMMAP[K230MemMap::Ddr as usize];
    let load_addr = linux_mem.map_or(ddr.base, |mem| mem.base);
    load_image_at(machine, path, load_addr)
}

fn load_firmware(
    machine: &K230Machine,
) -> Result<LoadedFirmware, Box<dyn std::error::Error>> {
    let ddr = K230_MEMMAP[K230MemMap::Ddr as usize];
    match boot::resolve_bios(&machine.bios_path) {
        BiosSource::None => Ok(LoadedFirmware {
            entry: ddr.base,
            high_addr: ddr.base,
            has_firmware: false,
        }),
        BiosSource::File(path) => {
            let image = load_firmware_data(machine, &std::fs::read(path)?)?;
            Ok(LoadedFirmware {
                entry: image.entry,
                high_addr: image.high_addr,
                has_firmware: true,
            })
        }
        BiosSource::Embedded => {
            if !K230_EMBEDDED_FW.is_empty() {
                let image = load_firmware_data(machine, K230_EMBEDDED_FW)?;
                Ok(LoadedFirmware {
                    entry: image.entry,
                    high_addr: image.high_addr,
                    has_firmware: true,
                })
            } else if let Some(path) = boot::find_firmware(K230_FW_FILENAME) {
                let image = load_firmware_data(machine, &std::fs::read(path)?)?;
                Ok(LoadedFirmware {
                    entry: image.entry,
                    high_addr: image.high_addr,
                    has_firmware: true,
                })
            } else {
                Err("no firmware found; use -bios <path>, set \
                     $MACHINA_DATADIR, or build with \
                     embed-firmware feature"
                    .into())
            }
        }
    }
}

fn load_image_at(
    machine: &K230Machine,
    path: &std::path::Path,
    load_addr: u64,
) -> Result<LoadedImage, Box<dyn std::error::Error>> {
    let data = std::fs::read(path)?;
    if data.starts_with(b"\x7fELF") {
        let info = loader::load_elf(&data, load_addr, machine.address_space())
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
        Ok(LoadedImage {
            entry: info.entry.0,
            low_addr: info.entry.0,
            high_addr: info.high_addr,
        })
    } else {
        let info = loader::load_binary(
            &data,
            GPA::new(load_addr),
            machine.address_space(),
        )?;
        Ok(LoadedImage {
            entry: info.entry.0,
            low_addr: load_addr,
            high_addr: info.high_addr,
        })
    }
}

fn load_firmware_data(
    machine: &K230Machine,
    data: &[u8],
) -> Result<LoadedImage, Box<dyn std::error::Error>> {
    let ddr = K230_MEMMAP[K230MemMap::Ddr as usize];
    load_firmware_blob(machine, data, ddr.base)
}

fn load_firmware_blob(
    machine: &K230Machine,
    data: &[u8],
    load_addr: u64,
) -> Result<LoadedImage, Box<dyn std::error::Error>> {
    if boot::is_elf(data) {
        let info = loader::load_elf(data, load_addr, machine.address_space())
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
        Ok(LoadedImage {
            entry: info.entry.0,
            low_addr: info.entry.0,
            high_addr: info.high_addr,
        })
    } else {
        let info = loader::load_binary(
            data,
            GPA::new(load_addr),
            machine.address_space(),
        )?;
        Ok(LoadedImage {
            entry: load_addr,
            low_addr: load_addr,
            high_addr: info.high_addr,
        })
    }
}

fn load_kernel(
    machine: &K230Machine,
    firmware: &LoadedFirmware,
    linux_mem: Option<FdtMemoryRegion>,
) -> Result<Option<LoadedImage>, Box<dyn std::error::Error>> {
    let Some(path) = machine.kernel_path() else {
        return Ok(None);
    };
    let ddr = K230_MEMMAP[K230MemMap::Ddr as usize];
    let mem_base = linux_mem.map_or(ddr.base, |mem| mem.base);
    let load_addr = if firmware.has_firmware {
        align_up_2m(firmware.high_addr).max(mem_base)
    } else {
        mem_base
    };
    Ok(Some(load_image_at(machine, path, load_addr)?))
}

fn load_initrd(
    machine: &K230Machine,
    kernel: Option<&LoadedImage>,
    linux_mem: Option<FdtMemoryRegion>,
) -> Result<Option<(u64, u64)>, Box<dyn std::error::Error>> {
    let Some(path) = machine.initrd_path() else {
        return Ok(None);
    };
    let ddr = K230_MEMMAP[K230MemMap::Ddr as usize];
    let mem_base = linux_mem.map_or(ddr.base, |mem| mem.base);
    let mem_size = linux_mem.map_or(machine.ram_size(), |mem| mem.size);
    let mem_end = mem_base
        .checked_add(mem_size)
        .ok_or("K230 Linux memory window end overflows u64")?;
    let data = std::fs::read(path)?;

    let start = if let Some(kernel) = kernel {
        let base = kernel
            .low_addr
            .checked_add((mem_size / 2).min(512 * 1024 * 1024))
            .ok_or("K230 initrd start overflows u64")?;
        align_up_4k(base.max(kernel.high_addr).max(mem_base))
    } else {
        0x0a10_0000
    };
    let end = start
        .checked_add(data.len() as u64)
        .ok_or("K230 initrd range end overflows u64")?;
    if start < mem_base || end > mem_end {
        return Err("K230 initrd exceeds Linux memory window".into());
    }
    loader::load_binary(&data, GPA::new(start), machine.address_space())?;
    Ok(Some((start, end)))
}

fn load_linux_memory_window(
    machine: &K230Machine,
) -> Result<Option<FdtMemoryRegion>, Box<dyn std::error::Error>> {
    let Some(path) = machine.dtb_path() else {
        return Ok(None);
    };
    let blob = std::fs::read(path)?;
    Ok(dtb_first_memory_region(&blob)?)
}

fn load_and_fix_user_dtb(
    machine: &mut K230Machine,
    initrd_range: Option<(u64, u64)>,
    linux_mem: Option<FdtMemoryRegion>,
) -> Result<u64, Box<dyn std::error::Error>> {
    let Some(path) = machine.dtb_path() else {
        return Ok(0);
    };
    let blob = std::fs::read(path)?;
    let fixed = fixup_k230_dtb(&blob, initrd_range, machine.kernel_cmdline())?;
    let addr = place_dtb(machine, &fixed, linux_mem)?;
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
    linux_mem: Option<FdtMemoryRegion>,
) -> Result<u64, Box<dyn std::error::Error>> {
    let ddr = K230_MEMMAP[K230MemMap::Ddr as usize];
    let len = blob.len() as u64;
    if let Some(mem) = linux_mem {
        if len > mem.size {
            return Err("K230 DTB blob larger than Linux memory window".into());
        }
        let mem_end = mem
            .base
            .checked_add(mem.size)
            .ok_or("K230 Linux memory window end overflows u64")?;
        let addr = align_down_2m(
            mem_end
                .checked_sub(len)
                .ok_or("K230 DTB does not fit in Linux memory window")?,
        );
        if addr < mem.base {
            return Err("K230 DTB does not fit in Linux memory window".into());
        }
        loader::load_binary(blob, GPA::new(addr), machine.address_space())?;
        return Ok(addr);
    }

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

fn align_up_4k(value: u64) -> u64 {
    (value + 0xfff) & !0xfff
}

fn align_up_2m(value: u64) -> u64 {
    (value + 0x1f_ffff) & !0x1f_ffff
}

fn align_down_2m(value: u64) -> u64 {
    value & !0x1f_ffff
}

fn write_k230_reset_vec(
    machine: &K230Machine,
    start_addr: u64,
    fdt_addr: u64,
    kernel_entry: u64,
    has_firmware: bool,
) {
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

    let next_mode = if has_firmware { 1 } else { 3 };
    let dynamic = DynamicInfo::new(kernel_entry, next_mode);
    let bytes = dynamic.to_bytes();
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.add(40), bytes.len());
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
