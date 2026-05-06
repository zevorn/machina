//! QEMU hardware probe tool.
//!
//! Satisfies the RuntimeOracle CLI contract:
//!   <device> --probe reset
//!   <device> --probe scenario <name>
//!
//! Prints `{"registers": {...}, "irqs": {...}}` to stdout and exits 0
//! on success.
//!
//! For devices with a QEMU qtest descriptor, the probe spawns QEMU and
//! reads guest-visible register/IRQ state at runtime.  When QEMU is
//! unavailable, it exits 77 with `SKIP: <reason>` on stderr.
//!
//! For devices without a descriptor (not yet wired), it falls back to
//! embedded fixture data.  These fixtures will be removed as each device
//! receives a QEMU descriptor.

use std::process;

use machina_oracle::descriptors;
use machina_oracle::qemu::{self, IrqSnapshot, RegSnapshot};

// -- Probe output helpers --

fn emit_json(regs: &RegSnapshot, irqs: &IrqSnapshot) {
    let regs_val = serde_json::to_value(regs).unwrap();
    let irqs_val = serde_json::to_value(irqs).unwrap();
    println!(
        "{}",
        serde_json::json!({"registers": regs_val, "irqs": irqs_val})
    );
}

// -- Entry point --

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: {} <device> --probe reset|scenario [name]", args[0]);
        process::exit(2);
    }

    let device = &args[1];
    let mode = parse_mode(&args);

    match mode {
        ProbeMode::Reset => {
            probe_reset(device);
        }
        ProbeMode::Scenario(name) => {
            probe_scenario(device, &name);
        }
    }
}

enum ProbeMode {
    Reset,
    Scenario(String),
}

fn parse_mode(args: &[String]) -> ProbeMode {
    if args.len() < 3 || args[2] != "--probe" {
        eprintln!("expected --probe, got: {}", args.get(2).map_or("", |s| s));
        process::exit(2);
    }
    let sub = args.get(3).map_or("", |s| s);
    match sub {
        "reset" => ProbeMode::Reset,
        "scenario" => {
            let name = args.get(4).cloned().unwrap_or_default();
            if name.is_empty() {
                eprintln!("missing scenario name");
                process::exit(2);
            }
            ProbeMode::Scenario(name)
        }
        other => {
            eprintln!("unknown probe mode: {other}");
            process::exit(2);
        }
    }
}

// -- QEMU-backed probe path (devices with descriptors) --

fn probe_reset(device: &str) {
    let desc = match descriptors::get_descriptor(device) {
        Some(d) => d,
        None => {
            // No QEMU descriptor — fall back to embedded fixture data.
            fixture_reset(device);
            return;
        }
    };

    match qemu::probe_reset(desc) {
        Ok((regs, irqs)) => emit_json(&regs, &irqs),
        Err(e) if e.starts_with("SKIP:") => {
            eprintln!("{e}");
            process::exit(77);
        }
        Err(e) => {
            eprintln!("{e}");
            process::exit(1);
        }
    }
}

fn probe_scenario(device: &str, name: &str) {
    let desc = match descriptors::get_descriptor(device) {
        Some(d) => d,
        None => {
            fixture_scenario(device, name);
            return;
        }
    };

    match qemu::probe_scenario(desc, name) {
        Ok((regs, irqs)) => emit_json(&regs, &irqs),
        Err(e) if e.starts_with("SKIP:") => {
            eprintln!("{e}");
            process::exit(77);
        }
        Err(e) => {
            eprintln!("{e}");
            process::exit(1);
        }
    }
}

// -- Embedded fixture fallback (devices without descriptors) ----------

struct DeviceData {
    reset_regs: RegSnapshot,
    scenarios: Vec<ScenarioData>,
}

struct ScenarioData {
    name: String,
    regs: RegSnapshot,
    irqs: IrqSnapshot,
}

fn fixture_reset(device: &str) {
    let data = get_device(device);
    let regs = serde_json::to_value(&data.reset_regs).unwrap();
    let irqs = serde_json::json!({});
    println!("{}", serde_json::json!({"registers": regs, "irqs": irqs}));
}

fn fixture_scenario(device: &str, name: &str) {
    let data = get_device(device);
    for s in &data.scenarios {
        if s.name == name {
            let regs = serde_json::to_value(&s.regs).unwrap();
            let irqs = serde_json::to_value(&s.irqs).unwrap();
            println!(
                "{}",
                serde_json::json!({"registers": regs, "irqs": irqs})
            );
            return;
        }
    }
    eprintln!("unknown scenario '{name}' for device");
    process::exit(1);
}

fn get_device(name: &str) -> DeviceData {
    match name {
        "sifive_e_prci" => sifive_e_prci(),
        "sifive_u_prci" => sifive_u_prci(),
        "pvpanic" => pvpanic(),
        "pvpanic-mmio" => pvpanic_mmio(),
        "unimp" => unimp(),
        "virt_ctrl" => virt_ctrl(),
        "led" => led(),
        "gpio_key" => gpio_key(),
        "gpio_pwr" => gpio_pwr(),
        "pch_msi" => pch_msi(),
        "dintc" => dintc(),
        "liointc" => liointc(),
        "cmgcr" => cmgcr(),
        "cpc" => cpc(),
        "loongarch_ipi" => loongarch_ipi(),
        "riscv_aplic" => riscv_aplic(),
        "riscv_imsic" => riscv_imsic(),
        _ => {
            eprintln!("unknown device: {name}");
            process::exit(1);
        }
    }
}

// -- Batch 1 fixture data --

fn sifive_e_prci() -> DeviceData {
    DeviceData {
        reset_regs: {
            let mut m = RegSnapshot::new();
            m.insert("HFROSCCFG".into(), 0xC000_0000);
            m.insert("HFXOSCCFG".into(), 0xC000_0000);
            m.insert("PLLCFG".into(), 0x8006_0000);
            m.insert("PLLOUTDIV".into(), 0x0000_0100);
            m
        },
        scenarios: vec![ScenarioData {
            name: "write PLLCFG".into(),
            regs: {
                let mut m = RegSnapshot::new();
                m.insert("PLLCFG".into(), 0x9234_5678);
                m
            },
            irqs: IrqSnapshot::new(),
        }],
    }
}

fn sifive_u_prci() -> DeviceData {
    let pllcfg0_default: u64 =
        (1 << 0) | (31 << 6) | (3 << 15) | (1 << 25) | (1 << 31);
    DeviceData {
        reset_regs: {
            let mut m = RegSnapshot::new();
            m.insert("HFXOSCCFG".into(), 0xC000_0000);
            m.insert("COREPLLCFG0".into(), pllcfg0_default);
            m.insert("DDRPLLCFG0".into(), pllcfg0_default);
            m.insert("DDRPLLCFG1".into(), 0);
            m.insert("GEMGXLPLLCFG0".into(), pllcfg0_default);
            m.insert("GEMGXLPLLCFG1".into(), 0);
            m.insert("CORECLKSEL".into(), 1);
            m.insert("DEVICESRESET".into(), 0);
            m.insert("CLKMUXSTATUS".into(), 0);
            m
        },
        scenarios: vec![ScenarioData {
            name: "write COREPLLCFG0".into(),
            regs: {
                let mut m = RegSnapshot::new();
                m.insert(
                    "COREPLLCFG0".into(),
                    0x0ABC_DEF0 | (1 << 25) | (1 << 31),
                );
                m
            },
            irqs: IrqSnapshot::new(),
        }],
    }
}

fn pvpanic() -> DeviceData {
    DeviceData {
        reset_regs: {
            let mut m = RegSnapshot::new();
            m.insert("EVENTS".into(), 1);
            m
        },
        scenarios: vec![ScenarioData {
            name: "write PANICKED".into(),
            regs: {
                let mut m = RegSnapshot::new();
                m.insert("ACTION".into(), 1);
                m
            },
            irqs: IrqSnapshot::new(),
        }],
    }
}

fn pvpanic_mmio() -> DeviceData {
    DeviceData {
        reset_regs: {
            let mut m = RegSnapshot::new();
            m.insert("EVENTS".into(), 3);
            m
        },
        scenarios: vec![ScenarioData {
            name: "write SHUTDOWN".into(),
            regs: {
                let mut m = RegSnapshot::new();
                m.insert("ACTION".into(), 2);
                m
            },
            irqs: IrqSnapshot::new(),
        }],
    }
}

fn unimp() -> DeviceData {
    DeviceData {
        reset_regs: RegSnapshot::new(),
        scenarios: vec![ScenarioData {
            name: "write then read".into(),
            regs: RegSnapshot::new(),
            irqs: IrqSnapshot::new(),
        }],
    }
}

fn virt_ctrl() -> DeviceData {
    DeviceData {
        reset_regs: {
            let mut m = RegSnapshot::new();
            m.insert("FEATURES".into(), 0x0000_0001);
            m.insert("CMD".into(), 0);
            m
        },
        scenarios: vec![
            ScenarioData {
                name: "write CMD_RESET".into(),
                regs: {
                    let mut m = RegSnapshot::new();
                    m.insert("ACTION".into(), 0);
                    m
                },
                irqs: IrqSnapshot::new(),
            },
            ScenarioData {
                name: "write CMD_HALT".into(),
                regs: {
                    let mut m = RegSnapshot::new();
                    m.insert("ACTION".into(), 1);
                    m
                },
                irqs: IrqSnapshot::new(),
            },
        ],
    }
}

fn led() -> DeviceData {
    DeviceData {
        reset_regs: {
            let mut m = RegSnapshot::new();
            m.insert("INTENSITY".into(), 100);
            m
        },
        scenarios: vec![ScenarioData {
            name: "set gpio low".into(),
            regs: {
                let mut m = RegSnapshot::new();
                m.insert("INTENSITY".into(), 0);
                m
            },
            irqs: IrqSnapshot::new(),
        }],
    }
}

fn gpio_key() -> DeviceData {
    DeviceData {
        reset_regs: RegSnapshot::new(),
        scenarios: vec![ScenarioData {
            name: "press key".into(),
            regs: RegSnapshot::new(),
            irqs: {
                let mut m = IrqSnapshot::new();
                m.insert(0, true);
                m
            },
        }],
    }
}

fn gpio_pwr() -> DeviceData {
    DeviceData {
        reset_regs: {
            let mut m = RegSnapshot::new();
            m.insert("ACTION".into(), 0);
            m
        },
        scenarios: vec![ScenarioData {
            name: "reset trigger".into(),
            regs: {
                let mut m = RegSnapshot::new();
                m.insert("ACTION".into(), 1);
                m
            },
            irqs: IrqSnapshot::new(),
        }],
    }
}

// -- Batch 2 fixture data --

fn pch_msi() -> DeviceData {
    DeviceData {
        reset_regs: RegSnapshot::new(),
        scenarios: vec![ScenarioData {
            name: "msg_data_LOW".into(),
            regs: RegSnapshot::new(),
            irqs: {
                let mut m = IrqSnapshot::new();
                m.insert(5, true);
                m
            },
        }],
    }
}

fn dintc() -> DeviceData {
    DeviceData {
        reset_regs: RegSnapshot::new(),
        scenarios: vec![ScenarioData {
            name: "ip_to_cpu1_vec3".into(),
            regs: {
                let mut m = RegSnapshot::new();
                m.insert("PENDING_CPU1".into(), 1 << 3);
                m
            },
            irqs: IrqSnapshot::new(),
        }],
    }
}

fn liointc() -> DeviceData {
    DeviceData {
        reset_regs: RegSnapshot::new(),
        scenarios: vec![ScenarioData {
            name: "map_irq3_to_core0_ip0".into(),
            regs: RegSnapshot::new(),
            irqs: {
                let mut m = IrqSnapshot::new();
                m.insert(0, true);
                m
            },
        }],
    }
}

fn cmgcr() -> DeviceData {
    DeviceData {
        reset_regs: {
            let mut m = RegSnapshot::new();
            m.insert("GCR_BASE".into(), 0);
            m.insert("GCR_CPC_STATUS".into(), 0);
            m
        },
        scenarios: vec![ScenarioData {
            name: "write_gcr_base".into(),
            regs: {
                let mut m = RegSnapshot::new();
                m.insert("GCR_BASE".into(), 0x40_0000);
                m
            },
            irqs: IrqSnapshot::new(),
        }],
    }
}

fn cpc() -> DeviceData {
    DeviceData {
        reset_regs: RegSnapshot::new(),
        scenarios: vec![ScenarioData {
            name: "write_vp_run".into(),
            regs: RegSnapshot::new(),
            irqs: IrqSnapshot::new(),
        }],
    }
}

fn loongarch_ipi() -> DeviceData {
    DeviceData {
        reset_regs: RegSnapshot::new(),
        scenarios: vec![ScenarioData {
            name: "send_ipi_to_cpu0".into(),
            regs: {
                let mut m = RegSnapshot::new();
                m.insert("STATUS".into(), 1 << 3);
                m
            },
            irqs: IrqSnapshot::new(),
        }],
    }
}

fn riscv_aplic() -> DeviceData {
    DeviceData {
        reset_regs: {
            let mut m = RegSnapshot::new();
            m.insert("DOMAINCFG".into(), 0x8000_0000);
            m.insert("SOURCECFG_1".into(), 0);
            m
        },
        scenarios: vec![ScenarioData {
            name: "write_domaincfg".into(),
            regs: {
                let mut m = RegSnapshot::new();
                m.insert("DOMAINCFG".into(), 0x8000_0100);
                m
            },
            irqs: IrqSnapshot::new(),
        }],
    }
}

fn riscv_imsic() -> DeviceData {
    DeviceData {
        reset_regs: {
            let mut m = RegSnapshot::new();
            m.insert("EIDELIVERY_0".into(), 0);
            m.insert("EITHRESHOLD_0".into(), 0);
            m
        },
        scenarios: vec![ScenarioData {
            name: "set_eidelivery".into(),
            regs: {
                let mut m = RegSnapshot::new();
                m.insert("EIDELIVERY_0".into(), 1);
                m
            },
            irqs: IrqSnapshot::new(),
        }],
    }
}
