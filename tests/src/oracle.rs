use std::collections::BTreeMap;
use std::io::Write;

use machina_oracle::{
    Oracle, OracleCheckResult, OracleFixture, OracleQuirk, OracleScenario,
    RegSnapshot, RuntimeOracle,
};

fn sample_fixture() -> OracleFixture {
    let mut reset_regs = BTreeMap::new();
    reset_regs.insert("RBR".into(), 0x00);
    reset_regs.insert("IER".into(), 0x00);
    reset_regs.insert("IIR".into(), 0x01);
    reset_regs.insert("LCR".into(), 0x00);
    reset_regs.insert("LSR".into(), 0x60);

    OracleFixture {
        device: "ns16550".into(),
        reset_regs,
        scenarios: vec![OracleScenario {
            name: "write LCR".into(),
            writes: vec![(3, 0x80, 1)],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("LCR".into(), 0x80);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    }
}

// -- Positive Tests (fixture-based Oracle) --

#[test]
fn test_oracle_load_fixture() {
    let json = serde_json::to_vec(&sample_fixture()).unwrap();
    let oracle = Oracle::load(&json).unwrap();
    assert_eq!(oracle.device(), "ns16550");
}

#[test]
fn test_oracle_check_reset_passes() {
    let fixture = sample_fixture();
    let json = serde_json::to_vec(&fixture).unwrap();
    let oracle = Oracle::load(&json).unwrap();

    let mut actual = BTreeMap::new();
    actual.insert("RBR".into(), 0x00);
    actual.insert("IER".into(), 0x00);
    actual.insert("IIR".into(), 0x01);
    actual.insert("LCR".into(), 0x00);
    actual.insert("LSR".into(), 0x60);

    let result = oracle.check_reset(&actual);
    assert_eq!(result.mismatches, 0);
    assert_eq!(result.total, 5);
}

#[test]
fn test_oracle_check_reset_detects_mismatch() {
    let fixture = sample_fixture();
    let json = serde_json::to_vec(&fixture).unwrap();
    let oracle = Oracle::load(&json).unwrap();

    let mut actual = BTreeMap::new();
    actual.insert("RBR".into(), 0x00);
    actual.insert("IER".into(), 0x00);
    actual.insert("IIR".into(), 0x01);
    actual.insert("LCR".into(), 0xFF); // wrong value
    actual.insert("LSR".into(), 0x60);

    let result = oracle.check_reset(&actual);
    assert_eq!(result.mismatches, 1);
    assert_eq!(result.details[0].register, "LCR");
    assert_eq!(result.details[0].expected, 0x00);
    assert_eq!(result.details[0].actual, 0xFF);
}

#[test]
fn test_oracle_check_reset_missing_reg() {
    let fixture = sample_fixture();
    let json = serde_json::to_vec(&fixture).unwrap();
    let oracle = Oracle::load(&json).unwrap();

    let actual = BTreeMap::new();
    let result = oracle.check_reset(&actual);
    assert!(result.mismatches > 0);
}

#[test]
fn test_oracle_check_reset_with_quirk() {
    let mut fixture = sample_fixture();
    fixture.quirks.push(OracleQuirk {
        target: "LSR".into(),
        reason: "Machina reports 0x00 after reset (known difference)".into(),
        approved: "2026-05-05".into(),
    });
    let json = serde_json::to_vec(&fixture).unwrap();
    let oracle = Oracle::load(&json).unwrap();

    let mut actual = BTreeMap::new();
    actual.insert("RBR".into(), 0x00);
    actual.insert("IER".into(), 0x00);
    actual.insert("IIR".into(), 0x01);
    actual.insert("LCR".into(), 0x00);
    actual.insert("LSR".into(), 0x00);

    let result = oracle.check_reset(&actual);
    assert_eq!(result.mismatches, 0);
}

#[test]
fn test_oracle_check_scenarios() {
    let fixture = sample_fixture();
    let json = serde_json::to_vec(&fixture).unwrap();
    let oracle = Oracle::load(&json).unwrap();

    let results = oracle.check_scenarios(&|_scenario| {
        let mut regs = BTreeMap::new();
        regs.insert("LCR".into(), 0x80);
        (regs, BTreeMap::new())
    });

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].mismatches, 0);
}

// -- Negative Tests (fixture-based) --

#[test]
fn test_oracle_load_invalid_json() {
    let result = Oracle::load(b"not json");
    assert!(result.is_err());
}

#[test]
fn test_oracle_load_empty_json() {
    let json =
        br#"{"device":"test","reset_regs":{},"scenarios":[],"quirks":[]}"#;
    let oracle = Oracle::load(json).unwrap();
    assert_eq!(oracle.device(), "test");

    let result = oracle.check_reset(&BTreeMap::new());
    assert_eq!(result.total, 0);
    assert_eq!(result.mismatches, 0);
}

// -- RuntimeOracle tests --

/// Write a fake probe script that outputs a JSON response.
fn write_probe_script(
    dir: &std::path::Path,
    name: &str,
    registers: &BTreeMap<String, u64>,
) -> std::path::PathBuf {
    let path = dir.join(name);
    let regs_json = serde_json::to_string(&serde_json::json!({
        "registers": registers,
        "irqs": {}
    }))
    .unwrap();
    let script = format!("#!/bin/sh\ncase \"$1\" in\n  --probe)\n");
    let script = script
        + &format!("    case \"$2\" in\n      reset) echo '{regs_json}' ;;\n");
    let script = script + "      *) echo '{\"registers\":{},\"irqs\":{}}' ;;\n";
    let script = script + "    esac\n    ;;\n";
    let script = script + "esac\n";

    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(script.as_bytes()).unwrap();
        f.flush().unwrap();
        // f drops here, closing the file
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    path
}

#[test]
fn test_runtime_oracle_check_reset_skip_missing_probe() {
    let fixture = sample_fixture();
    let json = serde_json::to_vec(&fixture).unwrap();
    let oracle =
        RuntimeOracle::new(&json, "/nonexistent/probe/command", &[]).unwrap();

    let actual = BTreeMap::new();
    match oracle.check_reset(&actual, &BTreeMap::new()) {
        OracleCheckResult::Skip(reason) => {
            assert!(reason.contains("cannot start probe"));
        }
        other => panic!("expected Skip, got {other:?}"),
    }
}

#[test]
fn test_runtime_oracle_check_reset_with_fake_probe() {
    let fixture = sample_fixture();
    let json = serde_json::to_vec(&fixture).unwrap();

    let dir = tempfile::TempDir::new().unwrap();
    let probe_path =
        write_probe_script(dir.path(), "fake-qemu", &fixture.reset_regs);

    let oracle =
        RuntimeOracle::new(&json, probe_path.to_str().unwrap(), &[]).unwrap();

    let mut actual = BTreeMap::new();
    actual.insert("RBR".into(), 0x00);
    actual.insert("IER".into(), 0x00);
    actual.insert("IIR".into(), 0x01);
    actual.insert("LCR".into(), 0x00);
    actual.insert("LSR".into(), 0x60);

    match oracle.check_reset(&actual, &BTreeMap::new()) {
        OracleCheckResult::Pass { total } => {
            assert_eq!(total, 5);
        }
        other => panic!("expected Pass, got {other:?}"),
    }
}

#[test]
fn test_runtime_oracle_check_reset_detects_mismatch() {
    let fixture = sample_fixture();
    let json = serde_json::to_vec(&fixture).unwrap();

    let dir = tempfile::TempDir::new().unwrap();
    let probe_path =
        write_probe_script(dir.path(), "fake-qemu", &fixture.reset_regs);

    let oracle =
        RuntimeOracle::new(&json, probe_path.to_str().unwrap(), &[]).unwrap();

    // LCR is 0x00 in probe, but we report 0xFF
    let mut actual = BTreeMap::new();
    actual.insert("RBR".into(), 0x00);
    actual.insert("IER".into(), 0x00);
    actual.insert("IIR".into(), 0x01);
    actual.insert("LCR".into(), 0xFF);
    actual.insert("LSR".into(), 0x60);

    match oracle.check_reset(&actual, &BTreeMap::new()) {
        OracleCheckResult::Mismatch(result) => {
            assert!(result.mismatches > 0);
        }
        other => panic!("expected Mismatch, got {other:?}"),
    }
}

#[test]
fn test_runtime_oracle_skip_when_probe_fails() {
    let fixture = sample_fixture();
    let json = serde_json::to_vec(&fixture).unwrap();

    let dir = tempfile::TempDir::new().unwrap();
    let probe_path = dir.path().join("failing-probe");
    // Write a script that exits non-zero; close before chmod+run.
    {
        let mut f = std::fs::File::create(&probe_path).unwrap();
        f.write_all(b"#!/bin/sh\nexit 1\n").unwrap();
        f.sync_all().unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            &probe_path,
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }

    let oracle =
        RuntimeOracle::new(&json, probe_path.to_str().unwrap(), &[]).unwrap();

    let actual = BTreeMap::new();
    match oracle.check_reset(&actual, &BTreeMap::new()) {
        OracleCheckResult::Error(_) => {}
        other => panic!("expected Error for failing probe, got {other:?}"),
    }
}

// -- RuntimeOracle: AC-6 contract tests --

#[test]
fn test_oracle_missing_false_irq_is_mismatch() {
    // When an expected IRQ (even false/deasserted) is missing from the
    // actual IRQ map, it must be flagged as a mismatch — the same class
    // of false-pass we fixed for registers in Round 2.
    let fixture = OracleFixture {
        device: "test".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![OracleScenario {
            name: "irq test".into(),
            writes: vec![],
            expected: BTreeMap::new(),
            irqs: {
                let mut m = BTreeMap::new();
                m.insert(0, false);
                m.insert(1, true);
                m
            },
        }],
        quirks: vec![],
    };
    let json = serde_json::to_vec(&fixture).unwrap();
    let oracle = Oracle::load(&json).unwrap();

    // Empty actual IRQ map — both expected IRQs are missing.
    let results =
        oracle.check_scenarios(&|_scenario| (BTreeMap::new(), BTreeMap::new()));
    assert_eq!(results.len(), 1);
    // IRQ 0 (false) missing → mismatch; IRQ 1 (true) missing → mismatch
    assert_eq!(results[0].mismatches, 2);
    // Verify the false IRQ appears in the mismatch details
    assert!(results[0]
        .details
        .iter()
        .any(|d| d.register == "IRQ_0" && d.expected == 0));
}

#[test]
fn test_oracle_irqs_match_when_present() {
    let fixture = OracleFixture {
        device: "test".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![OracleScenario {
            name: "irq match".into(),
            writes: vec![],
            expected: BTreeMap::new(),
            irqs: {
                let mut m = BTreeMap::new();
                m.insert(0, false);
                m.insert(1, true);
                m
            },
        }],
        quirks: vec![],
    };
    let json = serde_json::to_vec(&fixture).unwrap();
    let oracle = Oracle::load(&json).unwrap();

    // Actual IRQs match expected.
    let results = oracle.check_scenarios(&|_scenario| {
        let mut irqs = BTreeMap::new();
        irqs.insert(0, false);
        irqs.insert(1, true);
        (BTreeMap::new(), irqs)
    });
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].mismatches, 0);
    assert_eq!(results[0].total, 2);
}

/// Write a probe script that records argv into a shared log.
fn write_argv_logging_probe(
    dir: &std::path::Path,
    name: &str,
) -> std::path::PathBuf {
    let path = dir.join(name);
    let log_path = dir.join("argv.log");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\0' \"$@\" >> {log}\n",
        log = log_path.to_str().unwrap()
    );
    let script = script + "echo '{\"registers\":{},\"irqs\":{}}'\n";
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(script.as_bytes()).unwrap();
        f.sync_all().unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    path
}

#[test]
fn test_runtime_oracle_scenario_argv_separate() {
    // The probe must receive --probe, scenario, <name> as separate argv.
    let fixture = OracleFixture {
        device: "test".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![OracleScenario {
            name: "write LCR".into(),
            writes: vec![],
            expected: BTreeMap::new(),
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };
    let json = serde_json::to_vec(&fixture).unwrap();

    let dir = tempfile::TempDir::new().unwrap();
    let probe_path = write_argv_logging_probe(dir.path(), "probe");
    let log_path = dir.path().join("argv.log");

    let oracle =
        RuntimeOracle::new(&json, probe_path.to_str().unwrap(), &[]).unwrap();

    let _ =
        oracle.check_scenarios(&|_scenario| (BTreeMap::new(), BTreeMap::new()));

    // Read the logged argv. It should contain three NUL-terminated
    // strings: --probe, scenario, write LCR.
    let log = std::fs::read(&log_path).unwrap_or_default();
    let args: Vec<&str> = log
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| std::str::from_utf8(s).unwrap_or(""))
        .collect();
    assert_eq!(args, vec!["--probe", "scenario", "write LCR"]);
}

#[test]
fn test_runtime_oracle_perm_denied_returns_error() {
    let fixture = sample_fixture();
    let json = serde_json::to_vec(&fixture).unwrap();

    let dir = tempfile::TempDir::new().unwrap();
    let probe_path = dir.path().join("noexec-probe");
    {
        let mut f = std::fs::File::create(&probe_path).unwrap();
        f.write_all(b"#!/bin/sh\necho '{}'\n").unwrap();
        f.sync_all().unwrap();
    }
    // No execute permission — permission denied on spawn.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            &probe_path,
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
    }

    let oracle =
        RuntimeOracle::new(&json, probe_path.to_str().unwrap(), &[]).unwrap();

    let actual = BTreeMap::new();
    match oracle.check_reset(&actual, &BTreeMap::new()) {
        OracleCheckResult::Error(msg) => {
            // Should NOT be Skip; must be Error for permission denied.
            assert!(
                msg.contains("Permission denied")
                    || msg.contains("cannot start probe"),
                "expected Permission denied or cannot start, got: {msg}"
            );
        }
        other => {
            panic!("expected Error for permission-denied probe, got {other:?}")
        }
    }
}

// -- Batch 1 Device Oracle Tests -----------------------------------

/// Resolve the probe command from MACHINA_QEMU_HW_PROBE env var.
fn probe_command() -> String {
    std::env::var("MACHINA_QEMU_HW_PROBE")
        .unwrap_or_else(|_| "/usr/local/bin/machina-qemu-hw-probe".into())
}

/// Build a RuntimeOracle from a fixture, check reset and all scenarios
/// against the probe. Assert Pass when the probe is available; accept
/// Skip only on NOT_FOUND (probe binary genuinely missing). Panics
/// on Mismatch, Error, or non-NOT_FOUND Skip.
fn check_batch1_oracle(fixture: &OracleFixture, actual: &RegSnapshot) {
    let json = serde_json::to_vec(fixture).unwrap();
    let probe = probe_command();
    let oracle = RuntimeOracle::new(&json, &probe, &[]).unwrap();

    // Reset check.
    match oracle.check_reset(actual, &BTreeMap::new()) {
        OracleCheckResult::Pass { .. } => {}
        OracleCheckResult::Skip(reason) => {
            assert!(
                reason.contains("NOT_FOUND"),
                "unexpected Skip for {}: {reason}",
                fixture.device,
            );
        }
        OracleCheckResult::Mismatch(r) => {
            panic!(
                "oracle mismatch for {}: {}/{} mismatched: {:?}",
                fixture.device, r.mismatches, r.total, r.details
            );
        }
        OracleCheckResult::Error(e) => {
            panic!("oracle error for {}: {e}", fixture.device);
        }
    }

    // Scenario checks.
    if fixture.scenarios.is_empty() {
        return;
    }
    for result in
        oracle.check_scenarios(&|_scenario| (BTreeMap::new(), BTreeMap::new()))
    {
        match result {
            OracleCheckResult::Pass { .. } => {}
            OracleCheckResult::Skip(reason) => {
                assert!(
                    reason.contains("NOT_FOUND"),
                    "unexpected scenario Skip for {}: {reason}",
                    fixture.device,
                );
            }
            OracleCheckResult::Mismatch(r) => {
                panic!(
                    "oracle scenario mismatch for {}: {:?}",
                    fixture.device, r.details
                );
            }
            OracleCheckResult::Error(e) => {
                panic!("oracle scenario error for {}: {e}", fixture.device);
            }
        }
    }
}

// -- sifive_e_prci --

#[test]
fn test_oracle_batch1_sifive_e_prci() {
    let fixture = OracleFixture {
        device: "sifive_e_prci".into(),
        reset_regs: {
            let mut m = BTreeMap::new();
            m.insert("HFROSCCFG".into(), 0xC000_0000);
            m.insert("HFXOSCCFG".into(), 0xC000_0000);
            m.insert("PLLCFG".into(), 0x8006_0000);
            m.insert("PLLOUTDIV".into(), 0x0000_0100);
            m
        },
        scenarios: vec![OracleScenario {
            name: "write PLLCFG".into(),
            writes: vec![(0x08, 0x1234_5678, 4)],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("PLLCFG".into(), 0x1234_5678);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };
    let mut actual = BTreeMap::new();
    actual.insert("HFROSCCFG".into(), 0xC000_0000);
    actual.insert("HFXOSCCFG".into(), 0xC000_0000);
    actual.insert("PLLCFG".into(), 0x8006_0000);
    actual.insert("PLLOUTDIV".into(), 0x0000_0100);
    check_batch1_oracle(&fixture, &actual);
}

// -- sifive_u_prci --

#[test]
fn test_oracle_batch1_sifive_u_prci() {
    let pllcfg0_default: u64 =
        (1 << 0) | (31 << 6) | (3 << 15) | (1 << 25) | (1 << 31);
    let fixture = OracleFixture {
        device: "sifive_u_prci".into(),
        reset_regs: {
            let mut m = BTreeMap::new();
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
        scenarios: vec![OracleScenario {
            name: "write COREPLLCFG0".into(),
            writes: vec![(0x04, 0x0ABC_DEF0, 4)],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("COREPLLCFG0".into(), 0x0ABC_DEF0);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };
    let mut actual = BTreeMap::new();
    actual.insert("HFXOSCCFG".into(), 0xC000_0000);
    actual.insert("COREPLLCFG0".into(), pllcfg0_default);
    actual.insert("DDRPLLCFG0".into(), pllcfg0_default);
    actual.insert("DDRPLLCFG1".into(), 0);
    actual.insert("GEMGXLPLLCFG0".into(), pllcfg0_default);
    actual.insert("GEMGXLPLLCFG1".into(), 0);
    actual.insert("CORECLKSEL".into(), 1);
    actual.insert("DEVICESRESET".into(), 0);
    actual.insert("CLKMUXSTATUS".into(), 0);
    check_batch1_oracle(&fixture, &actual);
}

// -- pvpanic (ISA) --

#[test]
fn test_oracle_batch1_pvpanic() {
    let fixture = OracleFixture {
        device: "pvpanic".into(),
        reset_regs: {
            let mut m = BTreeMap::new();
            m.insert("EVENTS".into(), 1);
            m
        },
        scenarios: vec![OracleScenario {
            name: "write PANICKED".into(),
            writes: vec![(0x00, 1, 1)],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("EVENTS".into(), 1);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };
    let mut actual = BTreeMap::new();
    actual.insert("EVENTS".into(), 1);
    check_batch1_oracle(&fixture, &actual);
}

// -- pvpanic-mmio --

#[test]
fn test_oracle_batch1_pvpanic_mmio() {
    // pvpanic-mmio defaults to PANICKED | CRASH_LOADED.
    let fixture = OracleFixture {
        device: "pvpanic-mmio".into(),
        reset_regs: {
            let mut m = BTreeMap::new();
            m.insert("EVENTS".into(), 3);
            m
        },
        scenarios: vec![OracleScenario {
            name: "write SHUTDOWN".into(),
            writes: vec![(0x00, 4, 1)],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("EVENTS".into(), 3);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };
    let mut actual = BTreeMap::new();
    actual.insert("EVENTS".into(), 3);
    check_batch1_oracle(&fixture, &actual);
}

// -- unimp --

#[test]
fn test_oracle_batch1_unimp() {
    let fixture = OracleFixture {
        device: "unimp".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![OracleScenario {
            name: "write then read".into(),
            writes: vec![(0x00, 0xDEAD_BEEF, 4)],
            expected: BTreeMap::new(),
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };
    check_batch1_oracle(&fixture, &BTreeMap::new());
}

// -- virt_ctrl --

#[test]
fn test_oracle_batch1_virt_ctrl() {
    let fixture = OracleFixture {
        device: "virt_ctrl".into(),
        reset_regs: {
            let mut m = BTreeMap::new();
            m.insert("FEATURES".into(), 0x0000_0001);
            m.insert("CMD".into(), 0);
            m
        },
        scenarios: vec![OracleScenario {
            name: "write CMD_RESET".into(),
            writes: vec![(0x04, 0x00, 4)],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("CMD".into(), 0);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };
    let mut actual = BTreeMap::new();
    actual.insert("FEATURES".into(), 0x0000_0001);
    actual.insert("CMD".into(), 0);
    check_batch1_oracle(&fixture, &actual);
}

// -- led --

#[test]
fn test_oracle_batch1_led() {
    let fixture = OracleFixture {
        device: "led".into(),
        reset_regs: {
            let mut m = BTreeMap::new();
            m.insert("INTENSITY".into(), 100);
            m
        },
        scenarios: vec![OracleScenario {
            name: "set gpio high".into(),
            writes: vec![],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("INTENSITY".into(), 255);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };
    let mut actual = BTreeMap::new();
    actual.insert("INTENSITY".into(), 100);
    check_batch1_oracle(&fixture, &actual);
}

// -- gpio_key --

#[test]
fn test_oracle_batch1_gpio_key() {
    let fixture = OracleFixture {
        device: "gpio_key".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![OracleScenario {
            name: "press key".into(),
            writes: vec![],
            expected: BTreeMap::new(),
            irqs: {
                let mut m = BTreeMap::new();
                m.insert(0, true);
                m
            },
        }],
        quirks: vec![],
    };
    check_batch1_oracle(&fixture, &BTreeMap::new());
}

// -- gpio_pwr --

#[test]
fn test_oracle_batch1_gpio_pwr() {
    let fixture = OracleFixture {
        device: "gpio_pwr".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![OracleScenario {
            name: "reset trigger".into(),
            writes: vec![],
            expected: BTreeMap::new(),
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };
    check_batch1_oracle(&fixture, &BTreeMap::new());
}
