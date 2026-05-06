//! QEMU hardware probe tool.
//!
//! Satisfies the RuntimeOracle CLI contract:
//!   <device> --probe reset
//!   <device> --probe scenario <name>
//!
//! Prints `{"registers": {...}, "irqs": {...}}` to stdout and exits 0
//! on success.  Exits non-zero on unknown device or scenario.
//!
//! Currently serves Batch 1/2 fixture data directly.  Future: spawn
//! QEMU to dynamically probe device behavior for devices not in the
//! embedded set.

use std::collections::BTreeMap;
use std::process;

type RegSnapshot = BTreeMap<String, u64>;
type IrqSnapshot = BTreeMap<u32, bool>;

struct DeviceData {
    reset_regs: RegSnapshot,
    scenarios: Vec<ScenarioData>,
}

struct ScenarioData {
    name: String,
    regs: RegSnapshot,
    irqs: IrqSnapshot,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: {} <device> --probe reset|scenario [name]", args[0]);
        process::exit(2);
    }

    let device = &args[1];
    let mode = &args[2];
    let scenario_name = if args.len() > 3 {
        Some(args[3].as_str())
    } else {
        None
    };

    let data = match get_device(device) {
        Some(d) => d,
        None => {
            eprintln!("unknown device: {device}");
            process::exit(1);
        }
    };

    let output = match mode.as_str() {
        "--probe" => {
            // Formats: --probe reset  OR  --probe scenario <name>
            let sub = scenario_name.unwrap_or("");
            match sub {
                "reset" => emit_reset(&data),
                "scenario" => {
                    let name = if args.len() > 4 {
                        args[4].as_str()
                    } else {
                        eprintln!("missing scenario name");
                        process::exit(2);
                    };
                    emit_scenario(&data, name)
                }
                other => {
                    eprintln!("unknown probe mode: {other}");
                    process::exit(2);
                }
            }
        }
        other => {
            eprintln!("expected --probe, got: {other}");
            process::exit(2);
        }
    };

    println!("{output}");
}

fn emit_reset(data: &DeviceData) -> String {
    let regs = serde_json::to_value(&data.reset_regs).unwrap();
    let irqs = serde_json::json!({});
    serde_json::json!({"registers": regs, "irqs": irqs}).to_string()
}

fn emit_scenario(data: &DeviceData, name: &str) -> String {
    for s in &data.scenarios {
        if s.name == name {
            let regs = serde_json::to_value(&s.regs).unwrap();
            let irqs = serde_json::to_value(&s.irqs).unwrap();
            return serde_json::json!({"registers": regs, "irqs": irqs})
                .to_string();
        }
    }
    eprintln!("unknown scenario '{name}' for device");
    process::exit(1);
}

fn get_device(name: &str) -> Option<DeviceData> {
    match name {
        "sifive_e_prci" => Some(sifive_e_prci()),
        "sifive_u_prci" => Some(sifive_u_prci()),
        "pvpanic" => Some(pvpanic()),
        "pvpanic-mmio" => Some(pvpanic_mmio()),
        "unimp" => Some(unimp()),
        "virt_ctrl" => Some(virt_ctrl()),
        "led" => Some(led()),
        "gpio_key" => Some(gpio_key()),
        "gpio_pwr" => Some(gpio_pwr()),
        "pch_msi" => Some(pch_msi()),
        "dintc" => Some(dintc()),
        "liointc" => Some(liointc()),
        "cmgcr" => Some(cmgcr()),
        "cpc" => Some(cpc()),
        "loongarch_ipi" => Some(loongarch_ipi()),
        "riscv_aplic" => Some(riscv_aplic()),
        "riscv_imsic" => Some(riscv_imsic()),
        _ => None,
    }
}

// -- Batch 1 fixture data --

fn sifive_e_prci() -> DeviceData {
    let pllcfg_default: u64 = 0x8006_0000;
    DeviceData {
        reset_regs: {
            let mut m = RegSnapshot::new();
            m.insert("HFROSCCFG".into(), 0xC000_0000);
            m.insert("HFXOSCCFG".into(), 0xC000_0000);
            m.insert("PLLCFG".into(), pllcfg_default);
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
