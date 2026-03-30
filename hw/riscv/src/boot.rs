// Boot setup for the riscv64-ref machine.

use machina_core::machine::Machine;

use crate::ref_machine::{RefMachine, RAM_BASE};

/// Addresses and entry point produced by boot setup.
pub struct BootInfo {
    pub entry_pc: u64,
    pub fdt_addr: u64,
}

/// Load bios/kernel data into RAM and place the FDT blob
/// at the top of RAM.  Returns entry addresses.
pub fn setup_boot(
    machine: &RefMachine,
    bios_data: Option<&[u8]>,
    kernel_data: Option<&[u8]>,
) -> Result<BootInfo, Box<dyn std::error::Error>> {
    // Load BIOS at RAM_BASE (offset 0 in RAM).
    if let Some(bios) = bios_data {
        machine.write_ram(0, bios)?;
    }

    // Load kernel at RAM_BASE + 0x20_0000 (offset 0x20_0000
    // in RAM).
    const KERNEL_OFFSET: u64 = 0x20_0000;
    if let Some(kernel) = kernel_data {
        machine.write_ram(KERNEL_OFFSET, kernel)?;
    }

    // Place FDT at the top of RAM, aligned down to 8 bytes.
    let fdt = machine.fdt_blob();
    let fdt_len = fdt.len() as u64;
    let ram_size = machine.ram_size();
    if fdt_len > ram_size {
        return Err("FDT blob larger than available RAM".into());
    }
    let fdt_offset = (ram_size - fdt_len) & !0x7;
    machine.write_ram(fdt_offset, fdt)?;

    Ok(BootInfo {
        entry_pc: RAM_BASE,
        fdt_addr: RAM_BASE + fdt_offset,
    })
}
