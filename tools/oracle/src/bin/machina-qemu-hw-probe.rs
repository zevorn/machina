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
            if let Some(desc) = descriptors::get_qtest_descriptor(device) {
                match qemu::probe_qtest_reset(desc) {
                    Ok((regs, irqs)) => {
                        emit_json(&regs, &irqs);
                        return;
                    }
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
            eprintln!("unknown device or missing runtime descriptor: {device}");
            process::exit(1);
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
            if let Some(desc) = descriptors::get_qtest_descriptor(device) {
                match qemu::probe_qtest_scenario(desc, name) {
                    Ok((regs, irqs)) => {
                        emit_json(&regs, &irqs);
                        return;
                    }
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
            eprintln!("unknown device or missing runtime descriptor: {device}");
            process::exit(1);
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
