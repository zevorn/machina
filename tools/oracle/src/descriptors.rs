//! Device descriptors for QEMU qtest probing.
//!
//! Each device descriptor provides the metadata needed to probe a device's
//! guest-visible state at runtime via QEMU qtest — machine type, MMIO base
//! address, register offsets, and scenario write sequences.
//!
//! These descriptors tell the probe HOW to obtain values, not what values
//! to expect. The expected values come from QEMU at runtime.

use crate::qemu::{DeviceDescriptor, ScenarioDescriptor};

// -- sifive_e_prci (SiFive E PRCI) --

const SIFIVE_E_PRCI_REGS: &[(&str, u64, u8)] = &[
    ("HFROSCCFG", 0x00, 4),
    ("HFXOSCCFG", 0x04, 4),
    ("PLLCFG", 0x08, 4),
    ("PLLOUTDIV", 0x0C, 4),
];

const SIFIVE_E_PRCI_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write PLLCFG",
    writes: &[(0x08, 0x9234_5678, 4)],
}];

pub const SIFIVE_E_PRCI: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "sifive_e",
    arch_hint: "riscv",
    qemu_extra_args: &[],
    mmio_base: 0x1000_8000,
    registers: SIFIVE_E_PRCI_REGS,
    scenarios: SIFIVE_E_PRCI_SCENARIOS,
};

// -- sifive_u_prci (SiFive U PRCI) --

const SIFIVE_U_PRCI_REGS: &[(&str, u64, u8)] = &[
    ("HFXOSCCFG", 0x00, 4),
    ("COREPLLCFG0", 0x04, 4),
    ("DDRPLLCFG0", 0x0C, 4),
    ("DDRPLLCFG1", 0x10, 4),
    ("GEMGXLPLLCFG0", 0x1C, 4),
    ("GEMGXLPLLCFG1", 0x20, 4),
    ("CORECLKSEL", 0x24, 4),
    ("DEVICESRESET", 0x28, 4),
    ("CLKMUXSTATUS", 0x2C, 4),
];

const SIFIVE_U_PRCI_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write COREPLLCFG0",
    writes: &[(0x04, 0x0ABC_DEF0 | (1 << 25) | (1 << 31), 4)],
}];

pub const SIFIVE_U_PRCI: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "sifive_u",
    arch_hint: "riscv",
    qemu_extra_args: &[],
    mmio_base: 0x1000_0000,
    registers: SIFIVE_U_PRCI_REGS,
    scenarios: SIFIVE_U_PRCI_SCENARIOS,
};

/// Map a device name string to its descriptor.
pub fn get_descriptor(name: &str) -> Option<&'static DeviceDescriptor> {
    match name {
        "sifive_e_prci" => Some(&SIFIVE_E_PRCI),
        "sifive_u_prci" => Some(&SIFIVE_U_PRCI),
        // Additional devices will be added incrementally.
        _ => None,
    }
}
