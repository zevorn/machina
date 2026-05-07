//! Device descriptors for QEMU qtest probing.
//!
//! Each device descriptor provides the metadata needed to probe a device's
//! guest-visible state at runtime via QEMU qtest — machine type, MMIO base
//! address, register offsets, and scenario write sequences.
//!
//! These descriptors tell the probe HOW to obtain values, not what values
//! to expect. The expected values come from QEMU at runtime.

use crate::qemu::{
    DeviceDescriptor, QtestDeviceDescriptor, QtestScenarioDescriptor,
    ScenarioDescriptor,
};

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

// -- plic (RISC-V virt interrupt controller) --

const PLIC_REGS: &[(&str, u64, u8)] = &[
    ("PRIORITY1", 0x0000_0004, 4),
    ("PRIORITY2", 0x0000_0008, 4),
    ("PRIORITY5", 0x0000_0014, 4),
    ("PRIORITY5_UNALIGNED", 0x0000_0015, 4),
    ("PENDING0", 0x0000_1000, 4),
    ("ENABLE0", 0x0000_2000, 4),
    ("THRESHOLD0", 0x0020_0000, 4),
    ("CLAIM0", 0x0020_0004, 4),
];

const PLIC_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write priority enable threshold",
        writes: &[
            (0x0000_0004, 0x0000_0007, 4),
            (0x0000_0008, 0x0000_0003, 4),
            (0x0000_2000, 0x0000_0006, 4),
            (0x0020_0000, 0x0000_0005, 4),
        ],
    },
    ScenarioDescriptor {
        name: "unaligned priority access",
        writes: &[(0x0000_0014, 0x0000_0007, 4), (0x0000_0015, 0x0000_0003, 4)],
    },
];

pub const PLIC: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "riscv",
    qemu_extra_args: &["-bios", "none"],
    mmio_base: 0x0c00_0000,
    registers: PLIC_REGS,
    scenarios: PLIC_SCENARIOS,
};

// -- riscv_aplic (RISC-V virt AIA direct interrupt controller) --

const RISCV_APLIC_REGS: &[(&str, u64, u8)] = &[
    ("DOMAINCFG", 0x0000, 4),
    ("SOURCECFG1", 0x0004, 4),
    ("SETIP0", 0x1c00, 4),
    ("SETIE0", 0x1e00, 4),
    ("TARGET1", 0x3004, 4),
    ("IDELIVERY0", 0x4000, 4),
    ("ITHRESHOLD0", 0x4008, 4),
];

const RISCV_APLIC_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write direct mode regs",
    writes: &[
        (0x0000, 0x0000_0100, 4),
        (0x0004, 0x0000_0004, 4),
        (0x1cdc, 0x0000_0001, 4),
        (0x1edc, 0x0000_0001, 4),
        (0x3004, 0x0001_2345, 4),
        (0x4000, 0x0000_0001, 4),
        (0x4008, 0x0000_0005, 4),
    ],
}];

pub const RISCV_APLIC: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt,aia=aplic",
    arch_hint: "riscv",
    qemu_extra_args: &["-bios", "none"],
    mmio_base: 0x0c00_0000,
    registers: RISCV_APLIC_REGS,
    scenarios: RISCV_APLIC_SCENARIOS,
};

// -- riscv_imsic (RISC-V virt AIA incoming MSI controller) --

const RISCV_IMSIC_REGS: &[(&str, u64, u8)] =
    &[("LE_DOORBELL", 0x00, 4), ("BE_DOORBELL", 0x04, 4)];

const RISCV_IMSIC_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write msi doorbells",
    writes: &[(0x00, 0x0000_0005, 4), (0x04, 0x0300_0000, 4)],
}];

pub const RISCV_IMSIC: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt,aia=aplic-imsic",
    arch_hint: "riscv",
    qemu_extra_args: &["-bios", "none"],
    mmio_base: 0x2400_0000,
    registers: RISCV_IMSIC_REGS,
    scenarios: RISCV_IMSIC_SCENARIOS,
};

// -- riscv_cmgcr (Boston-aia coherent manager global registers) --

const RISCV_CMGCR_REGS: &[(&str, u64, u8)] = &[
    ("GCR_CONFIG", 0x0000, 8),
    ("GCR_BASE", 0x0008, 8),
    ("GCR_REV", 0x0030, 8),
    ("GCR_CPC_STATUS", 0x00f0, 8),
    ("GCR_L2_CONFIG", 0x0130, 8),
];

const RISCV_CMGCR_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write masked gcr base",
    writes: &[(0x0008, 0x1fb8_1234, 8)],
}];

pub const RISCV_CMGCR: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "boston-aia",
    arch_hint: "riscv-boston",
    qemu_extra_args: &["-kernel", "/bin/true"],
    mmio_base: 0x1fb8_0000,
    registers: RISCV_CMGCR_REGS,
    scenarios: RISCV_CMGCR_SCENARIOS,
};

// -- riscv_cpc (Boston-aia cluster power controller) --

const RISCV_CPC_REGS: &[(&str, u64, u8)] =
    &[("CM_STAT_CONF", 0x1008, 8), ("CL0_STAT_CONF", 0x2008, 8)];

const RISCV_CPC_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "run and stop vp0",
    writes: &[(0x2028, 0x0000_0001, 8), (0x2020, 0x0000_0001, 8)],
}];

pub const RISCV_CPC: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "boston-aia",
    arch_hint: "riscv-boston",
    qemu_extra_args: &["-kernel", "/bin/true"],
    mmio_base: 0x1fb8_8000,
    registers: RISCV_CPC_REGS,
    scenarios: RISCV_CPC_SCENARIOS,
};

// -- aclint (RISC-V virt local interruptor) --

const ACLINT_REGS: &[(&str, u64, u8)] = &[
    ("MSIP0", 0x0000, 4),
    ("MSIP1", 0x0004, 4),
    ("MTIMECMP0", 0x4000, 8),
];

const ACLINT_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write msip and mtimecmp",
    writes: &[
        (0xbff8, 0x0000_1000, 8),
        (0x4000, 0x0000_0010, 8),
        (0x0000, 0x0000_00ff, 4),
    ],
}];

pub const ACLINT: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "riscv",
    qemu_extra_args: &["-bios", "none"],
    mmio_base: 0x0200_0000,
    registers: ACLINT_REGS,
    scenarios: ACLINT_SCENARIOS,
};

// -- sifive_test (RISC-V virt finisher) --

const SIFIVE_TEST_REGS: &[(&str, u64, u8)] = &[("FINISHER", 0x00, 4)];

pub const SIFIVE_TEST: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "riscv",
    qemu_extra_args: &["-bios", "none"],
    mmio_base: 0x0010_0000,
    registers: SIFIVE_TEST_REGS,
    scenarios: &[],
};

// -- unimp (dummy unimplemented MMIO region) --

const UNIMP_REGS: &[(&str, u64, u8)] = &[
    ("READ0", 0x00, 4),
    ("READ4", 0x04, 4),
    ("READB0", 0x00, 1),
    ("READW2", 0x02, 2),
];

const UNIMP_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write ignored",
    writes: &[(0x00, 0xdead_beef, 4), (0x08, 0x1234_5678, 4)],
}];

pub const UNIMP: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "mps3-an547",
    arch_hint: "arm",
    qemu_extra_args: &["-S"],
    mmio_base: 0x5002_2000,
    registers: UNIMP_REGS,
    scenarios: UNIMP_SCENARIOS,
};

// -- gpio_key (ARM virt power-button GPIO key) --

const GPIO_KEY_QTEST_SCENARIOS: &[QtestScenarioDescriptor] =
    &[QtestScenarioDescriptor {
        name: "press and release",
        commands: &[
            "irq_intercept_in /machine/unattached/device[6]",
            "set_irq_in /machine/unattached/device[7] unnamed-gpio-in 0 1",
            "clock_step 100000000",
        ],
    }];

pub const GPIO_KEY_QTEST: QtestDeviceDescriptor = QtestDeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "aarch64",
    qemu_extra_args: &["-accel", "qtest"],
    scenarios: GPIO_KEY_QTEST_SCENARIOS,
};

// -- led (qtest GPIO input + trace-observed intensity) --

const LED_QTEST_SCENARIOS: &[QtestScenarioDescriptor] =
    &[QtestScenarioDescriptor {
        name: "set gpio high then low",
        commands: &[
            "set_irq_in /machine/peripheral/led0 unnamed-gpio-in 0 1",
            "set_irq_in /machine/peripheral/led0 unnamed-gpio-in 0 0",
        ],
    }];

pub const LED_QTEST: QtestDeviceDescriptor = QtestDeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "aarch64",
    qemu_extra_args: &[
        "-accel",
        "qtest",
        "-device",
        "led,id=led0,color=green,description=status",
        "-trace",
        "led_set_intensity",
    ],
    scenarios: LED_QTEST_SCENARIOS,
};

// -- gpio_pwr (ARM virt secure GPIO power controller) --

const GPIO_PWR_QTEST_SCENARIOS: &[QtestScenarioDescriptor] =
    &[QtestScenarioDescriptor {
        name: "shutdown trigger",
        commands: &["set_irq_in /machine/unattached/device[10] shutdown 0 1"],
    }];

pub const GPIO_PWR_QTEST: QtestDeviceDescriptor = QtestDeviceDescriptor {
    qemu_machine: "virt,secure=on",
    arch_hint: "aarch64",
    qemu_extra_args: &[
        "-accel",
        "qtest",
        "-trace",
        "qemu_system_shutdown_request",
    ],
    scenarios: GPIO_PWR_QTEST_SCENARIOS,
};

// -- pvpanic (x86 ISA pvpanic I/O port) --

const PVPANIC_REGS: &[(&str, u64, u8)] = &[
    ("EVENTS", 0x00, 1),
    ("EVENTS_W", 0x00, 2),
    ("EVENTS_L", 0x00, 4),
];

const PVPANIC_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write PANICKED",
    writes: &[(0x00, 0x01, 1)],
}];

pub const PVPANIC: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "pc",
    arch_hint: "x86_64-ioport",
    qemu_extra_args: &["-device", "pvpanic,ioport=0x505"],
    mmio_base: 0x505,
    registers: PVPANIC_REGS,
    scenarios: PVPANIC_SCENARIOS,
};

const PVPANIC_MMIO_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write SHUTDOWN",
    writes: &[(0x00, 0x04, 1)],
}];

pub const PVPANIC_MMIO: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "pc",
    arch_hint: "x86_64-ioport",
    qemu_extra_args: &["-device", "pvpanic,ioport=0x505"],
    mmio_base: 0x505,
    registers: PVPANIC_REGS,
    scenarios: PVPANIC_MMIO_SCENARIOS,
};

// -- virt_ctrl (m68k virt system controller) --

const VIRT_CTRL_REGS: &[(&str, u64, u8)] =
    &[("FEATURES", 0x00, 4), ("CMD", 0x04, 4)];

const VIRT_CTRL_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write CMD_NOOP",
    writes: &[(0x04, 0x0000_0000, 4)],
}];

pub const VIRT_CTRL: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "m68k",
    qemu_extra_args: &[],
    mmio_base: 0xff00_9000,
    registers: VIRT_CTRL_REGS,
    scenarios: VIRT_CTRL_SCENARIOS,
};

// -- loongarch_ipi (LoongArch virt IPI) --

const LOONGARCH_IPI_REGS: &[(&str, u64, u8)] = &[
    ("STATUS", 0x000, 4),
    ("ENABLE", 0x004, 4),
    ("MAILBOX0", 0x020, 8),
];

const LOONGARCH_IPI_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write enable mailbox",
        writes: &[(0x004, 0x0000_0001, 4), (0x020, 0x1122_3344_5566_7788, 8)],
    },
    ScenarioDescriptor {
        name: "send_ipi_to_cpu0",
        writes: &[(0x040, 0x0000_0003, 8)],
    },
];

pub const LOONGARCH_IPI: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "loongarch",
    qemu_extra_args: &[],
    mmio_base: 0x0100_0000,
    registers: LOONGARCH_IPI_REGS,
    scenarios: LOONGARCH_IPI_SCENARIOS,
};

// -- loongarch_dintc (LoongArch virt doorbell interrupt controller) --

const LOONGARCH_DINTC_REGS: &[(&str, u64, u8)] =
    &[("ZERO0", 0x0000, 4), ("CPU1_VEC5", 0x1050, 4)];

const LOONGARCH_DINTC_SCENARIOS: &[ScenarioDescriptor] =
    &[ScenarioDescriptor {
        name: "send cpu0 vector 5",
        writes: &[(0x0050, 0x0000_0000, 4)],
    }];

pub const LOONGARCH_DINTC: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "loongarch",
    qemu_extra_args: &[],
    mmio_base: 0x2fe0_0000,
    registers: LOONGARCH_DINTC_REGS,
    scenarios: LOONGARCH_DINTC_SCENARIOS,
};

// -- liointc (Loongson local I/O interrupt controller) --

const LIOINTC_REGS: &[(&str, u64, u8)] = &[
    ("MAPPER3", 0x03, 1),
    ("ISR", 0x20, 4),
    ("IEN", 0x24, 4),
    ("PER_CORE_ISR0", 0x40, 4),
];

const LIOINTC_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "map irq3 enable",
    writes: &[(0x03, 0x11, 1), (0x28, 0x0000_0008, 4)],
}];

pub const LIOINTC: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "loongson3-virt",
    arch_hint: "mips64el",
    qemu_extra_args: &["-bios", "/bin/true"],
    mmio_base: 0x3ff0_1400,
    registers: LIOINTC_REGS,
    scenarios: LIOINTC_SCENARIOS,
};

// -- loongarch_pch_msi (LoongArch virt PCH MSI doorbell) --

const LOONGARCH_PCH_MSI_REGS: &[(&str, u64, u8)] =
    &[("MSI0", 0x00, 4), ("MSI1", 0x04, 4)];

const LOONGARCH_PCH_MSI_SCENARIOS: &[ScenarioDescriptor] =
    &[ScenarioDescriptor {
        name: "write msi vectors",
        writes: &[(0x00, 0x20, 4), (0x04, 0x25, 4)],
    }];

pub const LOONGARCH_PCH_MSI: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "loongarch",
    qemu_extra_args: &[],
    mmio_base: 0x2ff0_0000,
    registers: LOONGARCH_PCH_MSI_REGS,
    scenarios: LOONGARCH_PCH_MSI_SCENARIOS,
};

// -- pch_pic (LoongArch virt PCH interrupt controller) --

const PCH_PIC_REGS: &[(&str, u64, u8)] = &[
    ("ID", 0x000, 8),
    ("INT_MASK", 0x020, 8),
    ("HTMSI_EN", 0x040, 8),
    ("INT_EDGE", 0x060, 8),
    ("ROUTE0", 0x100, 4),
    ("HTMSI_VEC0", 0x200, 4),
    ("INT_STATUS", 0x3a0, 8),
    ("INT_POL", 0x3e0, 8),
];

const PCH_PIC_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write mask route vector polarity",
    writes: &[
        (0x020, 0xffff_ffff_ffff_fffb, 8),
        (0x040, 0x0000_0000_0000_0004, 8),
        (0x060, 0x0000_0000_0000_0004, 8),
        (0x102, 0x05, 1),
        (0x202, 0x23, 1),
        (0x3e0, 0x0000_0000_0000_0004, 8),
    ],
}];

pub const PCH_PIC: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "loongarch",
    qemu_extra_args: &[],
    mmio_base: 0x1000_0000,
    registers: PCH_PIC_REGS,
    scenarios: PCH_PIC_SCENARIOS,
};

// -- eiointc (LoongArch virt extended interrupt controller) --

const EIOINTC_REGS: &[(&str, u64, u8)] = &[
    ("ZERO", 0x000, 4),
    ("NODEMAP0", 0x0a0, 4),
    ("IPMAP0", 0x0c0, 4),
    ("ENABLE0", 0x200, 4),
    ("BOUNCE0", 0x280, 4),
    ("CORE_ISR0", 0x400, 4),
    ("COREMAP0", 0x800, 4),
];

const EIOINTC_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write routing regs",
    writes: &[
        (0x0a0, 0x0102_0304, 4),
        (0x0c0, 0x08, 1),
        (0x200, 0x0000_0004, 4),
        (0x280, 0x0000_0002, 4),
        (0x800, 0x01, 1),
    ],
}];

pub const EIOINTC: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "loongarch",
    qemu_extra_args: &[],
    mmio_base: 0x0200_0000,
    registers: EIOINTC_REGS,
    scenarios: EIOINTC_SCENARIOS,
};

// -- uart16550 (RISC-V virt UART0) --

const UART16550_REGS: &[(&str, u64, u8)] = &[
    ("DLL", 0x00, 1),
    ("DLM", 0x01, 1),
    ("IIR", 0x02, 1),
    ("LCR", 0x03, 1),
    ("LSR", 0x05, 1),
    ("SCR", 0x07, 1),
];

const UART16550_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write divisor latch",
    writes: &[
        (0x03, 0x80, 1),
        (0x00, 0x34, 1),
        (0x01, 0x12, 1),
        (0x07, 0x5a, 1),
    ],
}];

pub const UART16550: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "riscv",
    qemu_extra_args: &["-bios", "none"],
    mmio_base: 0x1000_0000,
    registers: UART16550_REGS,
    scenarios: UART16550_SCENARIOS,
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

// -- pflash_cfi01 (RISC-V virt pflash0) --

const PFLASH_CFI01_REGS: &[(&str, u64, u8)] = &[
    ("READ_0000", 0x00, 1),
    ("READ_0004", 0x04, 1),
    ("READ_0020", 0x20, 1),
    ("READ_0040", 0x40, 1),
    ("READ_0044", 0x44, 1),
    ("READ_0048", 0x48, 1),
];

const PFLASH_CFI01_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "cfi query",
        writes: &[(0x55, 0x98, 1)],
    },
    ScenarioDescriptor {
        name: "id query",
        writes: &[(0x00, 0x90, 1)],
    },
    ScenarioDescriptor {
        name: "erase then program",
        writes: &[
            (0x20, 0x20, 1),
            (0x20, 0xd0, 1),
            (0x00, 0xff, 1),
            (0x00, 0x40, 1),
            (0x20, 0x0f, 1),
            (0x00, 0xff, 1),
        ],
    },
];

pub const PFLASH_CFI01: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "riscv",
    qemu_extra_args: &["-bios", "none"],
    mmio_base: 0x2000_0000,
    registers: PFLASH_CFI01_REGS,
    scenarios: PFLASH_CFI01_SCENARIOS,
};

// -- pflash_cfi02 (ARM Zynq pflash) --

const PFLASH_CFI02_REGS: &[(&str, u64, u8)] = &[
    ("READ_0000", 0x00, 1),
    ("READ_0001", 0x01, 1),
    ("READ_000e", 0x0e, 1),
    ("READ_000f", 0x0f, 1),
    ("READ_0010", 0x10, 1),
    ("READ_0011", 0x11, 1),
    ("READ_0012", 0x12, 1),
    ("READ_0020", 0x20, 1),
];

const PFLASH_CFI02_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "cfi query",
        writes: &[(0x55, 0x98, 1)],
    },
    ScenarioDescriptor {
        name: "id query",
        writes: &[(0x555, 0xaa, 1), (0x2aa, 0x55, 1), (0x555, 0x90, 1)],
    },
    ScenarioDescriptor {
        name: "erase then program",
        writes: &[
            (0x555, 0xaa, 1),
            (0x2aa, 0x55, 1),
            (0x555, 0x80, 1),
            (0x555, 0xaa, 1),
            (0x2aa, 0x55, 1),
            (0x20, 0x30, 1),
            (0x00, 0xf0, 1),
            (0x555, 0xaa, 1),
            (0x2aa, 0x55, 1),
            (0x555, 0xa0, 1),
            (0x20, 0x0f, 1),
        ],
    },
];

pub const PFLASH_CFI02: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "xilinx-zynq-a9",
    arch_hint: "arm",
    qemu_extra_args: &[],
    mmio_base: 0xe200_0000,
    registers: PFLASH_CFI02_REGS,
    scenarios: PFLASH_CFI02_SCENARIOS,
};

// -- m25p80 (SiFive U SPI0 flash) --

const M25P80_REGS: &[(&str, u64, u8)] = &[
    ("RX_00", 0x4c, 4),
    ("RX_01", 0x4c, 4),
    ("RX_02", 0x4c, 4),
    ("RX_03", 0x4c, 4),
];

const M25P80_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "jedec id",
    writes: &[
        (0x18, 0x02, 4),
        (0x48, 0x9f, 4),
        (0x48, 0x00, 4),
        (0x48, 0x00, 4),
        (0x48, 0x00, 4),
        (0x18, 0x03, 4),
    ],
}];

pub const M25P80: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "sifive_u",
    arch_hint: "riscv",
    qemu_extra_args: &[],
    mmio_base: 0x1004_0000,
    registers: M25P80_REGS,
    scenarios: M25P80_SCENARIOS,
};

// -- PL022 (ARM MPS2 SPI controller) --

const PL022_REGS: &[(&str, u64, u8)] = &[
    ("CR0", 0x00, 4),
    ("CR0_LO8", 0x00, 1),
    ("CR0_LO16", 0x00, 2),
    ("CR1", 0x04, 4),
    ("DR", 0x08, 4),
    ("SR", 0x0c, 4),
    ("CPSR", 0x10, 4),
    ("CPSR_LO8", 0x10, 1),
    ("IMSC", 0x14, 4),
    ("RIS", 0x18, 4),
    ("MIS", 0x1c, 4),
    ("PID0", 0xfe0, 4),
    ("PID_UNALIGNED1", 0xfe1, 4),
    ("PID_UNALIGNED2", 0xfe2, 4),
    ("PID_UNALIGNED3", 0xfe3, 4),
    ("PID1", 0xfe4, 4),
    ("PID2", 0xfe8, 4),
    ("PID3", 0xfec, 4),
    ("CID0", 0xff0, 4),
    ("CID1", 0xff4, 4),
    ("CID2", 0xff8, 4),
    ("CID3", 0xffc, 4),
];

const PL022_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "loopback fifo",
        writes: &[
            (0x00, 0x07, 4),
            (0x04, 0x03, 4),
            (0x10, 0xfe, 4),
            (0x14, 0x08, 4),
            (0x08, 0xab, 4),
        ],
    },
    ScenarioDescriptor {
        name: "narrow access regs",
        writes: &[(0x00, 0x1234_5678, 4), (0x10, 0x1234_5678, 2)],
    },
];

pub const PL022: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "mps2-an385",
    arch_hint: "arm",
    qemu_extra_args: &["-S"],
    mmio_base: 0x4002_5000,
    registers: PL022_REGS,
    scenarios: PL022_SCENARIOS,
};

// -- sifive_uart (SiFive U UART0) --

const SIFIVE_UART_REGS: &[(&str, u64, u8)] = &[
    ("TXFIFO", 0x00, 4),
    ("RXFIFO", 0x04, 4),
    ("TXCTRL", 0x08, 4),
    ("RXCTRL", 0x0c, 4),
    ("IE", 0x10, 4),
    ("IP", 0x14, 4),
    ("DIV", 0x18, 4),
];

const SIFIVE_UART_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write control regs",
    writes: &[
        (0x08, 0x0003_0001, 4),
        (0x0c, 0x0002_0001, 4),
        (0x10, 0x0000_0003, 4),
        (0x18, 0x0000_1234, 4),
    ],
}];

pub const SIFIVE_UART: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "sifive_u",
    arch_hint: "riscv",
    qemu_extra_args: &[],
    mmio_base: 0x1001_0000,
    registers: SIFIVE_UART_REGS,
    scenarios: SIFIVE_UART_SCENARIOS,
};

// -- riscv_htif (RISC-V Spike HTIF console) --

const RISCV_HTIF_REGS: &[(&str, u64, u8)] = &[
    ("FROMHOST_LO", 0x00, 4),
    ("FROMHOST_B0", 0x00, 1),
    ("FROMHOST_W0", 0x00, 2),
    ("FROMHOST_B1", 0x01, 1),
    ("FROMHOST_W2", 0x02, 2),
    ("FROMHOST_HI", 0x04, 4),
    ("TOHOST_LO", 0x08, 4),
    ("TOHOST_HI", 0x0c, 4),
];

const RISCV_HTIF_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "console putc",
        writes: &[(0x08, 0x41, 4), (0x0c, 0x0101_0000, 4)],
    },
    ScenarioDescriptor {
        name: "narrow write regs",
        writes: &[(0x08, 0x1234_5678, 1), (0x00, 0x1234_5678, 2)],
    },
];

pub const RISCV_HTIF: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "spike",
    arch_hint: "riscv",
    qemu_extra_args: &["-bios", "none"],
    mmio_base: 0x0100_0000,
    registers: RISCV_HTIF_REGS,
    scenarios: RISCV_HTIF_SCENARIOS,
};

// -- sifive_spi (SiFive U SPI0 controller) --

const SIFIVE_SPI_REGS: &[(&str, u64, u8)] = &[
    ("SCKDIV", 0x00, 4),
    ("SCKDIV_UNALIGNED", 0x01, 4),
    ("CSID", 0x10, 4),
    ("CSDEF", 0x14, 4),
    ("CSDEF_UNALIGNED", 0x15, 4),
    ("CSMODE", 0x18, 4),
    ("DELAY0", 0x28, 4),
    ("DELAY1", 0x2c, 4),
    ("TXDATA", 0x48, 4),
    ("RXDATA", 0x4c, 4),
    ("IE", 0x70, 4),
    ("IP", 0x74, 4),
];

pub const SIFIVE_SPI: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "sifive_u",
    arch_hint: "riscv",
    qemu_extra_args: &[],
    mmio_base: 0x1004_0000,
    registers: SIFIVE_SPI_REGS,
    scenarios: &[ScenarioDescriptor {
        name: "unaligned access ignored",
        writes: &[
            (0x01, 0x0000_0007, 4),
            (0x14, 0x0000_0000, 4),
            (0x15, 0x0000_0001, 4),
        ],
    }],
};

// -- ssi-sd (SiFive U SPI1-attached SD card adapter) --

const SSI_SD_REGS: &[(&str, u64, u8)] = &[
    ("RX_00", 0x4c, 4),
    ("RX_01", 0x4c, 4),
    ("RX_02", 0x4c, 4),
    ("RX_03", 0x4c, 4),
    ("RX_04", 0x4c, 4),
    ("RX_05", 0x4c, 4),
    ("RX_06", 0x4c, 4),
    ("RX_07", 0x4c, 4),
];

const SSI_SD_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "cmd8 response prefix",
    writes: &[
        (0x18, 0x02, 4),
        (0x48, 0x48, 4),
        (0x48, 0x00, 4),
        (0x48, 0x00, 4),
        (0x48, 0x01, 4),
        (0x48, 0xaa, 4),
        (0x48, 0x87, 4),
        (0x48, 0xff, 4),
        (0x48, 0xff, 4),
    ],
}];

pub const SSI_SD: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "sifive_u",
    arch_hint: "riscv",
    qemu_extra_args: &[
        "-drive",
        "if=sd,driver=null-co,read-zeroes=on,size=64M",
    ],
    mmio_base: 0x1005_0000,
    registers: SSI_SD_REGS,
    scenarios: SSI_SD_SCENARIOS,
};

// -- sifive_pwm (SiFive U PWM0) --

const SIFIVE_PWM_REGS: &[(&str, u64, u8)] = &[
    ("CONFIG", 0x00, 4),
    ("CONFIG_LO8", 0x00, 1),
    ("CONFIG_LO16", 0x00, 2),
    ("COUNT", 0x08, 4),
    ("PWMS", 0x10, 4),
    ("PWMCMP0", 0x20, 4),
    ("PWMCMP0_LO8", 0x20, 1),
    ("PWMCMP0_LO16", 0x20, 2),
    ("PWMCMP0_UNALIGNED1", 0x21, 4),
    ("PWMCMP0_UNALIGNED2", 0x22, 4),
    ("PWMCMP0_UNALIGNED3", 0x23, 4),
    ("PWMCMP1", 0x24, 4),
    ("PWMCMP2", 0x28, 4),
    ("PWMCMP3", 0x2c, 4),
];

const SIFIVE_PWM_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write compare regs",
        writes: &[
            (0x20, 0x1234_5678, 4),
            (0x24, 0x0001_ffff, 4),
            (0x28, 0x0002_0000, 4),
            (0x2c, 0xffff_ffff, 4),
        ],
    },
    ScenarioDescriptor {
        name: "unaligned wide access",
        writes: &[(0x21, 0x0102_0304, 4)],
    },
];

pub const SIFIVE_PWM: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "sifive_u",
    arch_hint: "riscv",
    qemu_extra_args: &[],
    mmio_base: 0x1002_0000,
    registers: SIFIVE_PWM_REGS,
    scenarios: SIFIVE_PWM_SCENARIOS,
};

// -- sse_counter (Arm SSE-300 system counter) --

const SSE_COUNTER_CONTROL_DELTA: u64 = 0x0fff_f000;

const SSE_COUNTER_REGS: &[(&str, u64, u8)] = &[
    ("CNTCR", SSE_COUNTER_CONTROL_DELTA, 4),
    ("CNTCV_LO", SSE_COUNTER_CONTROL_DELTA + 0x08, 4),
    ("CNTCV_HI", SSE_COUNTER_CONTROL_DELTA + 0x0c, 4),
    ("CNTSCR", SSE_COUNTER_CONTROL_DELTA + 0x10, 4),
    ("CNTID", SSE_COUNTER_CONTROL_DELTA + 0x1c, 4),
    ("CNTSCR0", SSE_COUNTER_CONTROL_DELTA + 0xd0, 4),
    ("PID4", SSE_COUNTER_CONTROL_DELTA + 0xfd0, 4),
    ("PID0", SSE_COUNTER_CONTROL_DELTA + 0xfe0, 4),
    ("STATUS_CNTCV_LO", 0x00, 4),
    ("STATUS_CNTCV_HI", 0x04, 4),
];

const SSE_COUNTER_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write counter regs",
    writes: &[
        (SSE_COUNTER_CONTROL_DELTA, 0x14, 4),
        (SSE_COUNTER_CONTROL_DELTA + 0x08, 0x1234_5678, 4),
        (SSE_COUNTER_CONTROL_DELTA + 0x0c, 0x9abc_def0, 4),
        (SSE_COUNTER_CONTROL_DELTA + 0xd0, 0x0100_0001, 4),
    ],
}];

pub const SSE_COUNTER: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "mps3-an547",
    arch_hint: "arm",
    qemu_extra_args: &[],
    mmio_base: 0x4810_1000,
    registers: SSE_COUNTER_REGS,
    scenarios: SSE_COUNTER_SCENARIOS,
};

// -- sse_timer (Arm SSE-300 system timer) --

const SSE_TIMER_SECPPC_NS: u64 = 0x0808_0070;
const SSE_TIMER_SECPPC_NSP: u64 = 0x0808_00b0;

const SSE_TIMER_REGS: &[(&str, u64, u8)] = &[
    ("CNTFRQ", 0x10, 4),
    ("CNTP_CVAL_LO", 0x20, 4),
    ("CNTP_CVAL_HI", 0x24, 4),
    ("CNTP_TVAL", 0x28, 4),
    ("CNTP_CTL", 0x2c, 4),
    ("CNTP_AIVAL_RELOAD", 0x48, 4),
    ("CNTP_AIVAL_CTL", 0x4c, 4),
    ("CNTP_CFG", 0x50, 4),
    ("PID4", 0xfd0, 4),
    ("PID0", 0xfe0, 4),
];

const SSE_TIMER_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write timer regs",
        writes: &[
            (SSE_TIMER_SECPPC_NS, 0x3f, 4),
            (SSE_TIMER_SECPPC_NSP, 0x3f, 4),
            (0x10, 0x1234_5678, 4),
            (0x20, 0x89ab_cdef, 4),
            (0x24, 0x0123_4567, 4),
            (0x2c, 0x0000_0003, 4),
            (0x48, 0x0000_55aa, 4),
            (0x4c, 0x0000_0001, 4),
        ],
    },
    ScenarioDescriptor {
        name: "tval signed write",
        writes: &[
            (SSE_TIMER_SECPPC_NS, 0x3f, 4),
            (SSE_TIMER_SECPPC_NSP, 0x3f, 4),
            (0x28, 0xffff_fffe, 4),
        ],
    },
];

pub const SSE_TIMER: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "mps3-an547",
    arch_hint: "arm",
    qemu_extra_args: &[],
    mmio_base: 0x4800_0000,
    registers: SSE_TIMER_REGS,
    scenarios: SSE_TIMER_SCENARIOS,
};

// -- sifive_gpio (SiFive U GPIO) --

const SIFIVE_GPIO_REGS: &[(&str, u64, u8)] = &[
    ("VALUE", 0x00, 4),
    ("INPUT_EN", 0x04, 4),
    ("INPUT_EN_LO8", 0x04, 1),
    ("INPUT_EN_LO16", 0x04, 2),
    ("INPUT_EN_UNALIGNED1", 0x05, 4),
    ("INPUT_EN_UNALIGNED2", 0x06, 4),
    ("INPUT_EN_UNALIGNED3", 0x07, 4),
    ("OUTPUT_EN", 0x08, 4),
    ("PORT", 0x0c, 4),
    ("PUE", 0x10, 4),
    ("DS", 0x14, 4),
];

const SIFIVE_GPIO_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write pin config",
        writes: &[
            (0x04, 0x0000_000f, 4),
            (0x08, 0x0000_0033, 4),
            (0x0c, 0x0000_0055, 4),
            (0x10, 0x0000_00aa, 4),
            (0x14, 0x0000_00ff, 4),
        ],
    },
    ScenarioDescriptor {
        name: "narrow access regs",
        writes: &[
            (0x04, 0x1234_5678, 4),
            (0x08, 0x1234_5678, 1),
            (0x0c, 0x1234_5678, 2),
        ],
    },
    ScenarioDescriptor {
        name: "unaligned wide access",
        writes: &[(0x05, 0x0102_0304, 4)],
    },
];

pub const SIFIVE_GPIO: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "sifive_u",
    arch_hint: "riscv",
    qemu_extra_args: &[],
    mmio_base: 0x1006_0000,
    registers: SIFIVE_GPIO_REGS,
    scenarios: SIFIVE_GPIO_SCENARIOS,
};

// -- sifive_e_aon (SiFive E always-on watchdog) --

const SIFIVE_E_AON_REGS: &[(&str, u64, u8)] = &[
    ("WDOGCFG", 0x00, 4),
    ("WDOGCOUNT", 0x08, 4),
    ("WDOGS", 0x10, 4),
    ("WDOGFEED", 0x18, 4),
    ("WDOGKEY", 0x1c, 4),
    ("WDOGCMP0", 0x20, 4),
    ("RTC", 0x40, 4),
    ("LFROSC", 0x70, 4),
];

const SIFIVE_E_AON_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write compare",
        writes: &[(0x1c, 0x0051_f15e, 4), (0x20, 0x1234, 4)],
    },
    ScenarioDescriptor {
        name: "scale wdogs",
        writes: &[
            (0x1c, 0x0051_f15e, 4),
            (0x08, 0x80, 4),
            (0x1c, 0x0051_f15e, 4),
            (0x00, 0x03, 4),
        ],
    },
];

pub const SIFIVE_E_AON: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "sifive_e",
    arch_hint: "riscv",
    qemu_extra_args: &["-S"],
    mmio_base: 0x1000_0000,
    registers: SIFIVE_E_AON_REGS,
    scenarios: SIFIVE_E_AON_SCENARIOS,
};

// -- PL061 (ARM virt GPIO) --

const PL061_REGS: &[(&str, u64, u8)] = &[
    ("DATA_3FC", 0x3fc, 4),
    ("DIR", 0x400, 4),
    ("ISENSE", 0x404, 4),
    ("IBE", 0x408, 4),
    ("IEV", 0x40c, 4),
    ("IM", 0x410, 4),
    ("RIS", 0x414, 4),
    ("MIS", 0x418, 4),
    ("AFSEL", 0x420, 4),
    ("DR2R", 0x500, 4),
    ("DR4R", 0x504, 4),
    ("DR8R", 0x508, 4),
    ("ODR", 0x50c, 4),
    ("PUR", 0x510, 4),
    ("PDR", 0x514, 4),
    ("SLR", 0x518, 4),
    ("DEN", 0x51c, 4),
    ("LOCK", 0x520, 4),
    ("CR", 0x524, 4),
    ("AMSEL", 0x528, 4),
    ("PID0", 0xfe0, 4),
    ("PID_UNALIGNED1", 0xfe1, 4),
    ("PID_UNALIGNED2", 0xfe2, 4),
    ("PID_UNALIGNED3", 0xfe3, 4),
    ("PID1", 0xfe4, 4),
    ("PID2", 0xfe8, 4),
    ("PID3", 0xfec, 4),
    ("CID0", 0xff0, 4),
    ("CID1", 0xff4, 4),
    ("CID2", 0xff8, 4),
    ("CID3", 0xffc, 4),
];

const PL061_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write gpio regs",
        writes: &[
            (0x400, 0x0f, 4),
            (0x3fc, 0x05, 4),
            (0x404, 0x01, 4),
            (0x408, 0x02, 4),
            (0x40c, 0x03, 4),
            (0x410, 0x03, 4),
            (0x420, 0x0f, 4),
        ],
    },
    ScenarioDescriptor {
        name: "luminary regs ignored",
        writes: &[
            (0x500, 0x12, 4),
            (0x504, 0x34, 4),
            (0x508, 0x56, 4),
            (0x50c, 0x78, 4),
            (0x510, 0x9a, 4),
            (0x514, 0xbc, 4),
            (0x518, 0xde, 4),
            (0x51c, 0xf0, 4),
            (0x520, 0x0acce551, 4),
            (0x524, 0x0f, 4),
            (0x528, 0x33, 4),
        ],
    },
    ScenarioDescriptor {
        name: "unaligned wide access",
        writes: &[(0x400, 0xff, 4), (0x000, 0x00, 4), (0x001, 0x0102_0304, 4)],
    },
];

pub const PL061: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "arm",
    qemu_extra_args: &[],
    mmio_base: 0x0903_0000,
    registers: PL061_REGS,
    scenarios: PL061_SCENARIOS,
};

// -- sifive_u_otp (SiFive U OTP) --

const SIFIVE_U_OTP_REGS: &[(&str, u64, u8)] = &[
    ("PA", 0x00, 4),
    ("PAIO", 0x04, 4),
    ("PAS", 0x08, 4),
    ("PCE", 0x0c, 4),
    ("PDIN", 0x14, 4),
    ("PDOUT", 0x18, 4),
    ("PDSTB", 0x1c, 4),
    ("PTRIM", 0x34, 4),
    ("PWE", 0x38, 4),
];

const SIFIVE_U_OTP_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "program bit",
        writes: &[
            (0x00, 0x0000_0000, 4),
            (0x04, 0x0000_0005, 4),
            (0x08, 0x0000_0000, 4),
            (0x14, 0x0000_0000, 4),
            (0x0c, 0x0000_0001, 4),
            (0x1c, 0x0000_0001, 4),
            (0x34, 0x0000_0001, 4),
            (0x38, 0x0000_0001, 4),
        ],
    },
    ScenarioDescriptor {
        name: "pdin value shift",
        writes: &[
            (0x00, 0x0000_00fc, 4),
            (0x04, 0x0000_0005, 4),
            (0x14, 0x0000_0002, 4),
            (0x38, 0x0000_0001, 4),
            (0x0c, 0x0000_0001, 4),
            (0x1c, 0x0000_0001, 4),
            (0x34, 0x0000_0001, 4),
        ],
    },
];

pub const SIFIVE_U_OTP: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "sifive_u",
    arch_hint: "riscv",
    qemu_extra_args: &[],
    mmio_base: 0x1007_0000,
    registers: SIFIVE_U_OTP_REGS,
    scenarios: SIFIVE_U_OTP_SCENARIOS,
};

// -- fw_cfg (RISC-V virt MMIO) --

const FW_CFG_REGS: &[(&str, u64, u8)] = &[
    ("DATA_0", 0x00, 1),
    ("DATA_1", 0x00, 1),
    ("DATA_2", 0x00, 1),
    ("DATA_3", 0x00, 1),
];

const FW_CFG_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "signature",
    writes: &[(0x08, 0x0000, 2)],
}];

pub const FW_CFG: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "riscv",
    qemu_extra_args: &["-bios", "none"],
    mmio_base: 0x1010_0000,
    registers: FW_CFG_REGS,
    scenarios: FW_CFG_SCENARIOS,
};

// -- PL181 (ARM Versatile Express MMCI) --

const PL181_REGS: &[(&str, u64, u8)] = &[
    ("POWER", 0x00, 4),
    ("CLOCK", 0x04, 4),
    ("ARGUMENT", 0x08, 4),
    ("ARGUMENT_UNALIGNED1", 0x09, 4),
    ("ARGUMENT_UNALIGNED2", 0x0a, 4),
    ("ARGUMENT_UNALIGNED3", 0x0b, 4),
    ("COMMAND", 0x0c, 4),
];

const PL181_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write control regs",
        writes: &[
            (0x00, 0x0000_0003, 4),
            (0x04, 0x0000_01ff, 4),
            (0x08, 0xdead_beef, 4),
        ],
    },
    ScenarioDescriptor {
        name: "unaligned wide access",
        writes: &[(0x09, 0x0102_0304, 4)],
    },
];

pub const PL181: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "vexpress-a9",
    arch_hint: "arm",
    qemu_extra_args: &["-audio", "none"],
    mmio_base: 0x1000_5000,
    registers: PL181_REGS,
    scenarios: PL181_SCENARIOS,
};

// -- sdhci (Xilinx Zynq generic SD host controller) --

const SDHCI_REGS: &[(&str, u64, u8)] = &[
    ("BLOCK_SIZE", 0x04, 2),
    ("BLOCK_COUNT", 0x06, 2),
    ("ARGUMENT", 0x08, 4),
    ("COMMAND", 0x0e, 2),
    ("SOFTWARE_RESET", 0x2f, 1),
    ("NORMAL_INT_STATUS", 0x30, 2),
    ("ERROR_INT_STATUS", 0x32, 2),
    ("NORMAL_INT_ENABLE", 0x34, 2),
    ("NORMAL_INT_SIGNAL_ENABLE", 0x38, 2),
];

const SDHCI_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write interrupt enables",
    writes: &[(0x34, 0xffff, 2), (0x38, 0x00ff, 2)],
}];

pub const SDHCI: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "xilinx-zynq-a9",
    arch_hint: "arm",
    qemu_extra_args: &["-S"],
    mmio_base: 0xe010_0000,
    registers: SDHCI_REGS,
    scenarios: SDHCI_SCENARIOS,
};

// -- sd-card (Xilinx Zynq SDHCI-backed SD memory card) --

const SD_CARD_REGS: &[(&str, u64, u8)] =
    &[("RESPONSE0", 0x10, 4), ("NORMAL_INT_STATUS", 0x30, 2)];

const SD_CARD_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "cmd8 interface condition",
    writes: &[
        (0x2c, 0x0007, 2),
        (0x30, 0xffff, 2),
        (0x34, 0x0001, 2),
        (0x08, 0x01aa, 4),
        (0x0e, 0x0802, 2),
    ],
}];

pub const SD_CARD: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "xilinx-zynq-a9",
    arch_hint: "arm",
    qemu_extra_args: &[
        "-drive",
        "if=none,id=sd0,driver=null-co,read-zeroes=on,size=64M",
        "-device",
        "sd-card,drive=sd0",
    ],
    mmio_base: 0xe010_1000,
    registers: SD_CARD_REGS,
    scenarios: SD_CARD_SCENARIOS,
};

// -- PL011 (ARM PrimeCell UART) --

const PL011_REGS: &[(&str, u64, u8)] = &[
    ("FR", 0x18, 4),
    ("IBRD", 0x24, 4),
    ("IBRD_LO8", 0x24, 1),
    ("IBRD_LO16", 0x24, 2),
    ("FBRD", 0x28, 4),
    ("LCRH", 0x2c, 4),
    ("CR", 0x30, 4),
    ("IFLS", 0x34, 4),
    ("IMSC", 0x38, 4),
    ("RIS", 0x3c, 4),
    ("MIS", 0x40, 4),
    ("DMACR", 0x48, 4),
    ("PID0", 0xfe0, 4),
    ("PID_UNALIGNED1", 0xfe1, 4),
    ("PID_UNALIGNED2", 0xfe2, 4),
    ("PID_UNALIGNED3", 0xfe3, 4),
    ("PID1", 0xfe4, 4),
    ("PID2", 0xfe8, 4),
    ("PID3", 0xfec, 4),
];

const PL011_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write control regs",
        writes: &[
            (0x24, 0xffff_ffff, 4),
            (0x28, 0xffff_ffff, 4),
            (0x2c, 0x0000_0070, 4),
            (0x30, 0x0000_0380, 4),
            (0x34, 0x0000_003f, 4),
            (0x38, 0x0000_07f2, 4),
        ],
    },
    ScenarioDescriptor {
        name: "narrow access regs",
        writes: &[
            (0x24, 0x1234_5678, 4),
            (0x28, 0x1234_5678, 1),
            (0x30, 0x1234_5678, 2),
        ],
    },
    ScenarioDescriptor {
        name: "unaligned wide access",
        writes: &[(0x25, 0x0102_0304, 4)],
    },
];

pub const PL011: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "vexpress-a9",
    arch_hint: "arm",
    qemu_extra_args: &["-audio", "none"],
    mmio_base: 0x1000_9000,
    registers: PL011_REGS,
    scenarios: PL011_SCENARIOS,
};

// -- PL031 (ARM PrimeCell RTC) --

const PL031_REGS: &[(&str, u64, u8)] = &[
    ("MR", 0x04, 4),
    ("MR_LO8", 0x04, 1),
    ("MR_LO16", 0x04, 2),
    ("LR", 0x08, 4),
    ("LR_LO8", 0x08, 1),
    ("LR_LO16", 0x08, 2),
    ("CR", 0x0c, 4),
    ("IMSC", 0x10, 4),
    ("RIS", 0x14, 4),
    ("MIS", 0x18, 4),
    ("PID0", 0xfe0, 4),
    ("PID_UNALIGNED1", 0xfe1, 4),
    ("PID_UNALIGNED2", 0xfe2, 4),
    ("PID_UNALIGNED3", 0xfe3, 4),
    ("PID1", 0xfe4, 4),
    ("PID2", 0xfe8, 4),
    ("PID3", 0xfec, 4),
    ("CID0", 0xff0, 4),
    ("CID1", 0xff4, 4),
    ("CID2", 0xff8, 4),
    ("CID3", 0xffc, 4),
];

const PL031_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write alarm regs",
        writes: &[
            (0x04, 0x1234_5678, 4),
            (0x08, 0x0000_0011, 4),
            (0x0c, 0x0000_0001, 4),
            (0x10, 0x0000_0001, 4),
        ],
    },
    ScenarioDescriptor {
        name: "zero load alarm",
        writes: &[(0x08, 0x0000_0000, 4), (0x10, 0x0000_0001, 4)],
    },
    ScenarioDescriptor {
        name: "narrow access regs",
        writes: &[
            (0x08, 0x1234_5678, 4),
            (0x04, 0x1234_5678, 1),
            (0x08, 0x1234_5678, 2),
        ],
    },
];

pub const PL031: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "vexpress-a9",
    arch_hint: "arm",
    qemu_extra_args: &["-audio", "none"],
    mmio_base: 0x1001_7000,
    registers: PL031_REGS,
    scenarios: PL031_SCENARIOS,
};

// -- goldfish_rtc (RISC-V virt RTC) --

const GOLDFISH_RTC_REGS: &[(&str, u64, u8)] = &[
    ("ALARM_LOW", 0x08, 4),
    ("ALARM_HIGH", 0x0c, 4),
    ("IRQ_ENABLED", 0x10, 4),
    ("ALARM_STATUS", 0x18, 4),
];

const GOLDFISH_RTC_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write alarm regs",
        writes: &[
            (0x04, 0x0000_0000, 4),
            (0x00, 0x0000_0000, 4),
            (0x0c, 0x0000_0001, 4),
            (0x08, 0x2345_6789, 4),
            (0x10, 0x0000_0001, 4),
        ],
    },
    ScenarioDescriptor {
        name: "time write does not fire alarm",
        writes: &[
            (0x04, 0x0000_0000, 4),
            (0x00, 0x0000_0000, 4),
            (0x0c, 0x0000_0001, 4),
            (0x08, 0x0000_0000, 4),
            (0x10, 0x0000_0001, 4),
            (0x04, 0x0000_0002, 4),
        ],
    },
];

pub const GOLDFISH_RTC: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "riscv",
    qemu_extra_args: &["-bios", "none"],
    mmio_base: 0x0010_1000,
    registers: GOLDFISH_RTC_REGS,
    scenarios: GOLDFISH_RTC_SCENARIOS,
};

// -- ls7a_rtc (LoongArch virt RTC) --

const LS7A_RTC_REGS: &[(&str, u64, u8)] = &[
    ("TOYMATCH0", 0x34, 4),
    ("TOYMATCH1", 0x38, 4),
    ("TOYMATCH2", 0x3c, 4),
    ("RTCCTRL", 0x40, 4),
    ("RTCMATCH0", 0x6c, 4),
    ("RTCMATCH1", 0x70, 4),
    ("RTCMATCH2", 0x74, 4),
];

const LS7A_RTC_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write match regs",
        writes: &[
            (0x40, 0x0000_2900, 4),
            (0x34, 0x0012_3456, 4),
            (0x38, 0x0065_4321, 4),
            (0x6c, 0x0000_1234, 4),
            (0x70, 0x0000_5678, 4),
        ],
    },
    ScenarioDescriptor {
        name: "rtc past match preserved",
        writes: &[
            (0x40, 0x0000_2900, 4),
            (0x64, 0x0000_0100, 4),
            (0x6c, 0x0000_0080, 4),
        ],
    },
    ScenarioDescriptor {
        name: "rtc time write preserves match",
        writes: &[
            (0x40, 0x0000_2900, 4),
            (0x6c, 0x0000_0080, 4),
            (0x64, 0x0000_0100, 4),
        ],
    },
    ScenarioDescriptor {
        name: "toy current match preserved",
        writes: &[(0x40, 0x0000_0900, 4), (0x34, 0x0420_0000, 4)],
    },
    ScenarioDescriptor {
        name: "toy time write preserves match",
        writes: &[
            (0x40, 0x0000_0900, 4),
            (0x34, 0x0420_0000, 4),
            (0x24, 0x0420_0000, 4),
        ],
    },
];

pub const LS7A_RTC: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "virt",
    arch_hint: "loongarch",
    qemu_extra_args: &[],
    mmio_base: 0x100d_0100,
    registers: LS7A_RTC_REGS,
    scenarios: LS7A_RTC_SCENARIOS,
};

// -- ds1338 (Raspberry Pi I2C RTC/NVRAM) --

const DS1338_REGS: &[(&str, u64, u8)] = &[("NVRAM10", 0x10, 4)];

const DS1338_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write and read nvram",
    writes: &[
        (0x0c, 0x68, 4),
        (0x08, 2, 4),
        (0x00, 0x83b0, 4),
        (0x10, 0x0a, 4),
        (0x10, 0xab, 4),
        (0x04, 0x0302, 4),
        (0x0c, 0x68, 4),
        (0x08, 1, 4),
        (0x00, 0x83b0, 4),
        (0x10, 0x0a, 4),
        (0x08, 1, 4),
        (0x00, 0x85b1, 4),
    ],
}];

pub const DS1338: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "raspi3b",
    arch_hint: "aarch64",
    qemu_extra_args: &["-device", "ds1338,address=0x68,bus=i2c-bus.0"],
    mmio_base: 0x3f20_5000,
    registers: DS1338_REGS,
    scenarios: DS1338_SCENARIOS,
};

// -- at24c-eeprom (Raspberry Pi I2C EEPROM) --

const EEPROM_AT24C_REGS: &[(&str, u64, u8)] = &[("DATA20", 0x10, 4)];

const EEPROM_AT24C_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write and read byte",
    writes: &[
        (0x0c, 0x50, 4),
        (0x08, 2, 4),
        (0x00, 0x83b0, 4),
        (0x10, 0x20, 4),
        (0x10, 0xaa, 4),
        (0x04, 0x0302, 4),
        (0x0c, 0x50, 4),
        (0x08, 1, 4),
        (0x00, 0x83b0, 4),
        (0x10, 0x20, 4),
        (0x08, 1, 4),
        (0x00, 0x85b1, 4),
    ],
}];

pub const EEPROM_AT24C: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "raspi3b",
    arch_hint: "aarch64",
    qemu_extra_args: &[
        "-device",
        "at24c-eeprom,address=0x50,bus=i2c-bus.0,rom-size=256",
    ],
    mmio_base: 0x3f20_5000,
    registers: EEPROM_AT24C_REGS,
    scenarios: EEPROM_AT24C_SCENARIOS,
};

// -- smbus_eeprom (Malta PIIX4 SMBus EEPROM) --

const SMBUS_EEPROM_REGS: &[(&str, u64, u8)] = &[("DATA20", 0x05, 1)];

const SMBUS_EEPROM_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write and read byte data",
    writes: &[
        (0x00, 0xff, 1),
        (0x03, 0x20, 1),
        (0x04, 0xa0, 1),
        (0x05, 0xaa, 1),
        (0x02, 0x49, 1),
        (0x00, 0xff, 1),
        (0x03, 0x20, 1),
        (0x04, 0xa1, 1),
        (0x02, 0x49, 1),
    ],
}];

pub const SMBUS_EEPROM: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "malta",
    arch_hint: "mips64el-ioport",
    qemu_extra_args: &[],
    mmio_base: 0x1100,
    registers: SMBUS_EEPROM_REGS,
    scenarios: SMBUS_EEPROM_SCENARIOS,
};

// -- tmp105 (Raspberry Pi I2C temperature sensor) --

const TMP105_REGS: &[(&str, u64, u8)] =
    &[("T_HIGH_MSB", 0x10, 4), ("T_HIGH_LSB", 0x10, 4)];

const TMP105_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write and read t_high",
    writes: &[
        (0x0c, 0x50, 4),
        (0x08, 3, 4),
        (0x00, 0x83b0, 4),
        (0x10, 0x03, 4),
        (0x10, 0xde, 4),
        (0x10, 0xad, 4),
        (0x04, 0x0302, 4),
        (0x0c, 0x50, 4),
        (0x08, 1, 4),
        (0x00, 0x83b0, 4),
        (0x10, 0x03, 4),
        (0x08, 2, 4),
        (0x00, 0x85b1, 4),
    ],
}];

pub const TMP105: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "raspi3b",
    arch_hint: "aarch64",
    qemu_extra_args: &["-device", "tmp105,address=0x50,bus=i2c-bus.0"],
    mmio_base: 0x3f20_5000,
    registers: TMP105_REGS,
    scenarios: TMP105_SCENARIOS,
};

// -- tmp421 (Raspberry Pi I2C temperature sensor) --

const TMP421_REGS: &[(&str, u64, u8)] = &[("CONFIG1", 0x10, 4)];

const TMP421_SCENARIOS: &[ScenarioDescriptor] = &[ScenarioDescriptor {
    name: "write and read config1",
    writes: &[
        (0x0c, 0x4c, 4),
        (0x08, 2, 4),
        (0x00, 0x83b0, 4),
        (0x10, 0x09, 4),
        (0x10, 0x44, 4),
        (0x04, 0x0302, 4),
        (0x0c, 0x4c, 4),
        (0x08, 1, 4),
        (0x00, 0x83b0, 4),
        (0x10, 0x09, 4),
        (0x08, 1, 4),
        (0x00, 0x85b1, 4),
    ],
}];

pub const TMP421: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "raspi3b",
    arch_hint: "aarch64",
    qemu_extra_args: &["-device", "tmp421,address=0x4c,bus=i2c-bus.0"],
    mmio_base: 0x3f20_5000,
    registers: TMP421_REGS,
    scenarios: TMP421_SCENARIOS,
};

// -- sbsa_gwdt (SBSA reference watchdog control frame) --

const SBSA_GWDT_REGS: &[(&str, u64, u8)] = &[
    ("WCS", 0x000, 4),
    ("WOR", 0x008, 4),
    ("WORU", 0x00c, 4),
    ("WCV", 0x010, 4),
    ("WCVU", 0x014, 4),
    ("W_IIDR", 0xfcc, 4),
];

const SBSA_GWDT_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write control regs",
        writes: &[
            (0x008, 0x1234_5678, 4),
            (0x00c, 0xaaaa_5555, 4),
            (0x010, 0xfeed_cafe, 4),
            (0x014, 0x8765_4321, 4),
        ],
    },
    ScenarioDescriptor {
        name: "enable offset updates compare regs",
        writes: &[(0x008, 62_500_000, 4), (0x000, 0x1, 4)],
    },
];

pub const SBSA_GWDT: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "sbsa-ref",
    arch_hint: "aarch64",
    qemu_extra_args: &["-S"],
    mmio_base: 0x5001_1000,
    registers: SBSA_GWDT_REGS,
    scenarios: SBSA_GWDT_SCENARIOS,
};

// -- pl050 (VersatilePB keyboard interface) --

const PL050_REGS: &[(&str, u64, u8)] = &[
    ("CR", 0x000, 4),
    ("CR_LO8", 0x000, 1),
    ("CR_LO16", 0x000, 2),
    ("STAT", 0x004, 4),
    ("DATA", 0x008, 4),
    ("CLKDIV", 0x00c, 4),
    ("CLKDIV_LO16", 0x00c, 2),
    ("IIR", 0x010, 4),
    ("ID0", 0xfe0, 4),
    ("ID_UNALIGNED1", 0xfe1, 4),
    ("ID_UNALIGNED2", 0xfe2, 4),
    ("ID_UNALIGNED3", 0xfe3, 4),
    ("ID1", 0xfe4, 4),
    ("ID2", 0xfe8, 4),
    ("ID3", 0xfec, 4),
    ("ID4", 0xff0, 4),
    ("ID5", 0xff4, 4),
    ("ID6", 0xff8, 4),
    ("ID7", 0xffc, 4),
];

const PL050_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write control regs",
        writes: &[(0x000, 0x18, 4), (0x00c, 0x1234, 4)],
    },
    ScenarioDescriptor {
        name: "write data resend",
        writes: &[(0x008, 0xab, 4)],
    },
    ScenarioDescriptor {
        name: "narrow access regs",
        writes: &[(0x000, 0x1234_5678, 4), (0x00c, 0x1234_5678, 2)],
    },
    ScenarioDescriptor {
        name: "unaligned wide access",
        writes: &[
            (0x000, 0x0000_0000, 4),
            (0x001, 0x0102_0304, 4),
            (0x00c, 0x0000_0000, 4),
            (0x00d, 0x0102_0304, 4),
        ],
    },
];

pub const PL050: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "versatilepb",
    arch_hint: "arm",
    qemu_extra_args: &["-S", "-audio", "none"],
    mmio_base: 0x1000_6000,
    registers: PL050_REGS,
    scenarios: PL050_SCENARIOS,
};

// -- pl080 (VersatilePB DMA controller) --

const PL080_REGS: &[(&str, u64, u8)] = &[
    ("INT_STATUS", 0x000, 4),
    ("INT_TC_STATUS", 0x004, 4),
    ("TC_RAW", 0x014, 4),
    ("ERR_RAW", 0x018, 4),
    ("ENABLED", 0x01c, 4),
    ("CONFIG", 0x030, 4),
    ("SYNC", 0x034, 4),
    ("CH2_SRC", 0x140, 4),
    ("CH2_DEST", 0x144, 4),
    ("CH2_LLI", 0x148, 4),
    ("CH2_CTRL", 0x14c, 4),
    ("CH2_CONF", 0x150, 4),
    ("ID0", 0xfe0, 4),
    ("ID_UNALIGNED1", 0xfe1, 4),
    ("ID_UNALIGNED2", 0xfe2, 4),
    ("ID_UNALIGNED3", 0xfe3, 4),
    ("ID1", 0xfe4, 4),
    ("ID2", 0xfe8, 4),
    ("ID3", 0xfec, 4),
    ("ID4", 0xff0, 4),
    ("ID5", 0xff4, 4),
    ("ID6", 0xff8, 4),
    ("ID7", 0xffc, 4),
];

const PL080_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "write channel regs",
        writes: &[
            (0x140, 0x1000_0000, 4),
            (0x144, 0x2000_0000, 4),
            (0x148, 0x3000_0000, 4),
            (0x14c, 0x8000_0010, 4),
            (0x150, 0x0000_0001, 4),
        ],
    },
    ScenarioDescriptor {
        name: "unaligned wide access",
        writes: &[
            (0x030, 0x0000_0000, 4),
            (0x034, 0x0000_0000, 4),
            (0x031, 0x0102_0304, 4),
        ],
    },
];

pub const PL080: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "versatilepb",
    arch_hint: "arm",
    qemu_extra_args: &["-S", "-audio", "none"],
    mmio_base: 0x1013_0000,
    registers: PL080_REGS,
    scenarios: PL080_SCENARIOS,
};

// -- sifive_pdma (SiFive U platform DMA) --

const SIFIVE_PDMA_REGS: &[(&str, u64, u8)] = &[
    ("CONTROL", 0x000, 4),
    ("NEXT_CONFIG", 0x004, 4),
    ("NEXT_BYTES", 0x008, 8),
    ("NEXT_BYTES_UNALIGNED", 0x009, 8),
    ("NEXT_DST", 0x010, 8),
    ("NEXT_SRC", 0x018, 8),
    ("EXEC_CONFIG", 0x104, 4),
    ("EXEC_BYTES", 0x108, 8),
    ("EXEC_DST", 0x110, 8),
    ("EXEC_SRC", 0x118, 8),
];

const SIFIVE_PDMA_SCENARIOS: &[ScenarioDescriptor] = &[
    ScenarioDescriptor {
        name: "claim channel 0",
        writes: &[(0x000, 0x0000_0001, 4)],
    },
    ScenarioDescriptor {
        name: "unaligned qword access",
        writes: &[(0x009, 0x0102_0304_0506_0708, 8)],
    },
];

pub const SIFIVE_PDMA: DeviceDescriptor = DeviceDescriptor {
    qemu_machine: "sifive_u",
    arch_hint: "riscv",
    qemu_extra_args: &["-S", "-bios", "none"],
    mmio_base: 0x0300_0000,
    registers: SIFIVE_PDMA_REGS,
    scenarios: SIFIVE_PDMA_SCENARIOS,
};

const ORACLE_DEVICE_NAMES: &[&str] = &[
    "sifive_e_prci",
    "plic",
    "riscv_aplic",
    "riscv_imsic",
    "cmgcr",
    "cpc",
    "aclint",
    "sifive_test",
    "unimp",
    "pvpanic",
    "pvpanic-mmio",
    "virt_ctrl",
    "loongarch_ipi",
    "loongarch_dintc",
    "liointc",
    "loongarch_pch_msi",
    "pch_pic",
    "eiointc",
    "uart16550",
    "sifive_u_prci",
    "pflash_cfi01",
    "pflash_cfi02",
    "m25p80",
    "pl022",
    "sifive_uart",
    "riscv_htif",
    "sifive_spi",
    "ssi_sd",
    "sifive_pwm",
    "sse_counter",
    "sse_timer",
    "sifive_gpio",
    "sifive_e_aon",
    "pl061",
    "sifive_u_otp",
    "fw_cfg",
    "pl181",
    "sdhci",
    "sd_card",
    "pl011",
    "pl031",
    "goldfish_rtc",
    "ls7a_rtc",
    "ds1338",
    "eeprom_at24c",
    "smbus_eeprom",
    "tmp105",
    "tmp421",
    "sbsa_gwdt",
    "pl050",
    "pl080",
    "sifive_pdma",
    "led",
    "gpio_key",
    "gpio_pwr",
];

pub fn all_oracle_device_names() -> &'static [&'static str] {
    ORACLE_DEVICE_NAMES
}

/// Map a device name string to its descriptor.
pub fn get_descriptor(name: &str) -> Option<&'static DeviceDescriptor> {
    match name {
        "sifive_e_prci" => Some(&SIFIVE_E_PRCI),
        "plic" => Some(&PLIC),
        "riscv_aplic" => Some(&RISCV_APLIC),
        "riscv_imsic" => Some(&RISCV_IMSIC),
        "cmgcr" => Some(&RISCV_CMGCR),
        "cpc" => Some(&RISCV_CPC),
        "aclint" => Some(&ACLINT),
        "sifive_test" => Some(&SIFIVE_TEST),
        "unimp" => Some(&UNIMP),
        "pvpanic" => Some(&PVPANIC),
        "pvpanic-mmio" => Some(&PVPANIC_MMIO),
        "virt_ctrl" => Some(&VIRT_CTRL),
        "loongarch_ipi" => Some(&LOONGARCH_IPI),
        "loongarch_dintc" => Some(&LOONGARCH_DINTC),
        "liointc" => Some(&LIOINTC),
        "loongarch_pch_msi" => Some(&LOONGARCH_PCH_MSI),
        "pch_pic" => Some(&PCH_PIC),
        "eiointc" => Some(&EIOINTC),
        "uart16550" => Some(&UART16550),
        "sifive_u_prci" => Some(&SIFIVE_U_PRCI),
        "pflash_cfi01" => Some(&PFLASH_CFI01),
        "pflash_cfi02" => Some(&PFLASH_CFI02),
        "m25p80" => Some(&M25P80),
        "pl022" => Some(&PL022),
        "sifive_uart" => Some(&SIFIVE_UART),
        "riscv_htif" => Some(&RISCV_HTIF),
        "sifive_spi" => Some(&SIFIVE_SPI),
        "ssi_sd" => Some(&SSI_SD),
        "sifive_pwm" => Some(&SIFIVE_PWM),
        "sse_counter" => Some(&SSE_COUNTER),
        "sse_timer" => Some(&SSE_TIMER),
        "sifive_gpio" => Some(&SIFIVE_GPIO),
        "sifive_e_aon" => Some(&SIFIVE_E_AON),
        "pl061" => Some(&PL061),
        "sifive_u_otp" => Some(&SIFIVE_U_OTP),
        "fw_cfg" => Some(&FW_CFG),
        "pl181" => Some(&PL181),
        "sdhci" => Some(&SDHCI),
        "sd_card" => Some(&SD_CARD),
        "pl011" => Some(&PL011),
        "pl031" => Some(&PL031),
        "goldfish_rtc" => Some(&GOLDFISH_RTC),
        "ls7a_rtc" => Some(&LS7A_RTC),
        "ds1338" => Some(&DS1338),
        "eeprom_at24c" => Some(&EEPROM_AT24C),
        "smbus_eeprom" => Some(&SMBUS_EEPROM),
        "tmp105" => Some(&TMP105),
        "tmp421" => Some(&TMP421),
        "sbsa_gwdt" => Some(&SBSA_GWDT),
        "pl050" => Some(&PL050),
        "pl080" => Some(&PL080),
        "sifive_pdma" => Some(&SIFIVE_PDMA),
        _ => None,
    }
}

/// Map a qtest-only device name string to its descriptor.
pub fn get_qtest_descriptor(
    name: &str,
) -> Option<&'static QtestDeviceDescriptor> {
    match name {
        "led" => Some(&LED_QTEST),
        "gpio_key" => Some(&GPIO_KEY_QTEST),
        "gpio_pwr" => Some(&GPIO_PWR_QTEST),
        _ => None,
    }
}
