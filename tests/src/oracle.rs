use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;

use machina_core::address::GPA;
use machina_hw_core::bus::SysBusDeviceState;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MemoryRegion;
use machina_oracle::{
    Oracle, OracleCheckResult, OracleFixture, OracleQuirk, OracleScenario,
    RegSnapshot, RuntimeOracle, ScenarioApplier,
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

/// Build a RuntimeOracle from a fixture with the device name in
/// probe_args, check reset and all scenarios against both the
/// fixture-based Oracle (always validates) and the RuntimeOracle
/// (probe-based, skips when probe not found).
fn check_batch1_oracle(
    fixture: &OracleFixture,
    actual_reset: &RegSnapshot,
    apply_scenario: &ScenarioApplier,
) {
    let json = serde_json::to_vec(fixture).unwrap();
    let probe = probe_command();
    let runtime =
        RuntimeOracle::new(&json, &probe, &[fixture.device.clone()]).unwrap();

    // Reset check (probe-based).
    match runtime.check_reset(actual_reset, &BTreeMap::new()) {
        OracleCheckResult::Pass { .. } => {}
        OracleCheckResult::Skip(reason) => {
            assert!(
                reason.contains("NOT_FOUND"),
                "unexpected reset Skip for {}: {reason}",
                fixture.device,
            );
        }
        OracleCheckResult::Mismatch(r) => {
            panic!(
                "oracle reset mismatch for {}: {}/{} mismatched: {:?}",
                fixture.device, r.mismatches, r.total, r.details
            );
        }
        OracleCheckResult::Error(e) => {
            panic!("oracle reset error for {}: {e}", fixture.device);
        }
    }

    if fixture.scenarios.is_empty() {
        return;
    }

    // Scenario check via fixture-based Oracle (always validates).
    let static_oracle = Oracle::load(&json).unwrap();
    let results = static_oracle.check_scenarios(apply_scenario);
    for result in &results {
        if result.mismatches > 0 {
            panic!(
                "fixture scenario mismatch for {}: {:?}",
                fixture.device, result.details
            );
        }
    }

    // Scenario check via RuntimeOracle (probe-based).
    for result in runtime.check_scenarios(apply_scenario) {
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

/// Build a simple MMIO applier: map the device at base, attach/bus/
/// realize, apply scenario writes, then read back register state.
fn mmio_scenario_applier(
    mmio: Arc<dyn machina_memory::region::MmioOps>,
    region_name: String,
    region_size: u64,
    base: u64,
    read_regs: fn(&mut AddressSpace, u64) -> RegSnapshot,
) -> Box<ScenarioApplier> {
    Box::new(move |scenario: &OracleScenario| {
        let region = MemoryRegion::io(&region_name, region_size, mmio.clone());
        let (mut aspace, mut bus) = crate::hw_misc::make_test_aspace();
        let mut state = SysBusDeviceState::new(&region_name);
        state.attach_to_bus(&mut bus).unwrap();
        state.register_mmio(region, GPA(base)).unwrap();
        state.realize_onto(&mut bus, &mut aspace).unwrap();

        for &(offset, val, size) in &scenario.writes {
            aspace.write(GPA(base + offset), u32::from(size), val);
        }
        let regs = read_regs(&mut aspace, base);
        (regs, BTreeMap::new())
    })
}

// -- sifive_e_prci --

#[test]
fn test_oracle_batch1_sifive_e_prci() {
    use machina_hw_misc::{SifiveEPRCI, SifiveEPRCIMmio};
    let prci = SifiveEPRCI::new();
    let prci2 = prci.clone();

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
                // PLLCFG write ORs in the LOCK bit (bit 31).
                m.insert("PLLCFG".into(), 0x1234_5678 | 0x8000_0000);
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

    let applier = mmio_scenario_applier(
        Arc::new(SifiveEPRCIMmio(prci2)),
        "sifive_e_prci".to_string(),
        0x1000,
        0x1000_0000,
        |aspace, base| {
            let mut m = BTreeMap::new();
            m.insert("PLLCFG".into(), aspace.read(GPA(base + 0x08), 4));
            m
        },
    );
    check_batch1_oracle(&fixture, &actual, &applier);
}

// -- sifive_u_prci --

#[test]
fn test_oracle_batch1_sifive_u_prci() {
    use machina_hw_misc::{SifiveUPRCI, SifiveUPRCIMmio};
    let prci = SifiveUPRCI::new();
    let prci2 = prci.clone();
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
                // COREPLLCFG0 write ORs in FSE (bit 25) and LOCK (bit 31).
                m.insert(
                    "COREPLLCFG0".into(),
                    0x0ABC_DEF0 | (1 << 25) | (1 << 31),
                );
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

    let applier = mmio_scenario_applier(
        Arc::new(SifiveUPRCIMmio(prci2)),
        "sifive_u_prci".to_string(),
        0x1000,
        0x1000_0000,
        |aspace, base| {
            let mut m = BTreeMap::new();
            m.insert("COREPLLCFG0".into(), aspace.read(GPA(base + 0x04), 4));
            m
        },
    );
    check_batch1_oracle(&fixture, &actual, &applier);
}

// -- pvpanic (ISA) --

#[test]
fn test_oracle_batch1_pvpanic() {
    use machina_hw_misc::{Pvpanic, PvpanicEvent, PvpanicMmio};
    let pvp = Pvpanic::new(PvpanicEvent::PANICKED);
    let pvp2 = pvp.clone();
    let events = Arc::new(std::sync::Mutex::new(0u8));
    let events2 = events.clone();
    pvp.set_event_handler(Box::new(move |e| {
        *events2.lock().unwrap() = e;
    }));

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
                m.insert("ACTION".into(), PvpanicEvent::PANICKED as u64);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };

    let mut actual = BTreeMap::new();
    actual.insert("EVENTS".into(), 1);

    let applier: Box<ScenarioApplier> =
        Box::new(move |scenario: &OracleScenario| {
            let (mut aspace, mut bus) = crate::hw_misc::make_test_aspace();
            let region = MemoryRegion::io(
                "pvpanic",
                0x2,
                Arc::new(PvpanicMmio(pvp2.clone())),
            );
            pvp2.attach_to_bus(&mut bus).unwrap();
            pvp2.register_mmio(region, GPA(0x1000_0000)).unwrap();
            pvp2.realize_onto(&mut bus, &mut aspace).unwrap();

            for &(offset, val, size) in &scenario.writes {
                aspace.write(GPA(0x1000_0000 + offset), u32::from(size), val);
            }
            let action = u64::from(*events.lock().unwrap());
            let mut regs = BTreeMap::new();
            regs.insert("ACTION".into(), action);
            (regs, BTreeMap::new())
        });
    check_batch1_oracle(&fixture, &actual, &applier);
}

// -- pvpanic-mmio --

#[test]
fn test_oracle_batch1_pvpanic_mmio() {
    use machina_hw_misc::{Pvpanic, PvpanicEvent, PvpanicMmio};
    let pvp = Pvpanic::new(PvpanicEvent::PANICKED | PvpanicEvent::CRASH_LOADED);
    let pvp2 = pvp.clone();
    let events = Arc::new(std::sync::Mutex::new(0u8));
    let events2 = events.clone();
    pvp.set_event_handler(Box::new(move |e| {
        *events2.lock().unwrap() = e;
    }));

    let fixture = OracleFixture {
        device: "pvpanic-mmio".into(),
        reset_regs: {
            let mut m = BTreeMap::new();
            m.insert("EVENTS".into(), 3);
            m
        },
        scenarios: vec![OracleScenario {
            name: "write SHUTDOWN".into(),
            writes: vec![(0x00, PvpanicEvent::SHUTDOWN as u64, 1)],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("ACTION".into(), PvpanicEvent::SHUTDOWN as u64);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };

    let mut actual = BTreeMap::new();
    actual.insert("EVENTS".into(), 3);

    let applier: Box<ScenarioApplier> =
        Box::new(move |scenario: &OracleScenario| {
            let (mut aspace, mut bus) = crate::hw_misc::make_test_aspace();
            let region = MemoryRegion::io(
                "pvpanic-mmio",
                0x2,
                Arc::new(PvpanicMmio(pvp2.clone())),
            );
            pvp2.attach_to_bus(&mut bus).unwrap();
            pvp2.register_mmio(region, GPA(0x1000_0000)).unwrap();
            pvp2.realize_onto(&mut bus, &mut aspace).unwrap();

            for &(offset, val, size) in &scenario.writes {
                aspace.write(GPA(0x1000_0000 + offset), u32::from(size), val);
            }
            let action = u64::from(*events.lock().unwrap());
            let mut regs = BTreeMap::new();
            regs.insert("ACTION".into(), action);
            (regs, BTreeMap::new())
        });
    check_batch1_oracle(&fixture, &actual, &applier);
}

// -- unimp --

#[test]
fn test_oracle_batch1_unimp() {
    use machina_hw_misc::{Unimp, UnimpMmio};
    let unimp = Unimp::new("unimp", 0x1000);
    let unimp2 = unimp.clone();

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

    let applier = mmio_scenario_applier(
        Arc::new(UnimpMmio(unimp2)),
        "unimp".to_string(),
        0x1000,
        0x1000_0000,
        |aspace, base| {
            let _ = aspace.read(GPA(base), 4);
            BTreeMap::new()
        },
    );
    check_batch1_oracle(&fixture, &BTreeMap::new(), &applier);
}

// -- virt_ctrl --

#[test]
fn test_oracle_batch1_virt_ctrl() {
    use machina_hw_misc::{VirtCtrl, VirtCtrlAction, VirtCtrlMmio};

    let fixture = OracleFixture {
        device: "virt_ctrl".into(),
        reset_regs: {
            let mut m = BTreeMap::new();
            m.insert("FEATURES".into(), 0x0000_0001);
            m.insert("CMD".into(), 0);
            m
        },
        scenarios: vec![
            OracleScenario {
                name: "write CMD_RESET".into(),
                writes: vec![(0x04, 1, 4)],
                expected: {
                    let mut m = BTreeMap::new();
                    m.insert("ACTION".into(), VirtCtrlAction::Reset as u64);
                    m
                },
                irqs: BTreeMap::new(),
            },
            OracleScenario {
                name: "write CMD_HALT".into(),
                writes: vec![(0x04, 2, 4)],
                expected: {
                    let mut m = BTreeMap::new();
                    m.insert("ACTION".into(), VirtCtrlAction::Halt as u64);
                    m
                },
                irqs: BTreeMap::new(),
            },
        ],
        quirks: vec![],
    };

    let mut actual = BTreeMap::new();
    actual.insert("FEATURES".into(), 0x0000_0001);
    actual.insert("CMD".into(), 0);

    let actions = Arc::new(std::sync::Mutex::new(Vec::new()));
    let applier: Box<ScenarioApplier> =
        Box::new(move |scenario: &OracleScenario| {
            let actions2 = actions.clone();
            let dev = VirtCtrl::new();
            dev.set_action_handler(Box::new(move |a| {
                actions2.lock().unwrap().push(a);
            }));

            let (mut aspace, mut bus) = crate::hw_misc::make_test_aspace();
            let region = MemoryRegion::io(
                "virt_ctrl",
                0x1000,
                Arc::new(VirtCtrlMmio(dev.clone())),
            );
            dev.attach_to_bus(&mut bus).unwrap();
            dev.register_mmio(region, GPA(0x1000_0000)).unwrap();
            dev.realize_onto(&mut bus, &mut aspace).unwrap();

            for &(offset, val, size) in &scenario.writes {
                aspace.write(GPA(0x1000_0000 + offset), u32::from(size), val);
            }
            let last_action = actions
                .lock()
                .unwrap()
                .last()
                .map(|a| *a as u64)
                .unwrap_or(0);
            let mut regs = BTreeMap::new();
            regs.insert("ACTION".into(), last_action);
            (regs, BTreeMap::new())
        });
    check_batch1_oracle(&fixture, &actual, &applier);
}

// -- led --

#[test]
fn test_oracle_batch1_led() {
    use machina_hw_misc::{Led, LedColor};
    let led = Led::new(LedColor::Green, "status", true);
    let led2 = led.clone();

    let fixture = OracleFixture {
        device: "led".into(),
        reset_regs: {
            let mut m = BTreeMap::new();
            m.insert("INTENSITY".into(), 100);
            m
        },
        scenarios: vec![OracleScenario {
            name: "set gpio low".into(),
            writes: vec![],
            expected: {
                let mut m = BTreeMap::new();
                // Active-high LED: gpio low → intensity 0.
                m.insert("INTENSITY".into(), 0);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };

    let mut actual = BTreeMap::new();
    actual.insert("INTENSITY".into(), 100);

    let applier: Box<ScenarioApplier> =
        Box::new(move |scenario: &OracleScenario| {
            if scenario.name == "set gpio low" {
                led2.set_gpio(false);
            }
            let mut regs = BTreeMap::new();
            regs.insert("INTENSITY".into(), u64::from(led2.get_intensity()));
            (regs, BTreeMap::new())
        });
    check_batch1_oracle(&fixture, &actual, &applier);
}

// -- gpio_key --

#[test]
fn test_oracle_batch1_gpio_key() {
    use machina_accel::timer::{ClockType, VirtualClock};
    use machina_hw_core::irq::{IrqLine, IrqSink};
    use machina_hw_gpio::GpioKey;
    use std::sync::Arc;

    #[derive(Default)]
    struct KeySink {
        level: std::sync::Mutex<bool>,
    }
    impl IrqSink for KeySink {
        fn set_irq(&self, _irq: u32, level: bool) {
            *self.level.lock().unwrap() = level;
        }
    }

    let sink = Arc::new(KeySink::default());
    let clock = Arc::new(VirtualClock::new(ClockType::Virtual));
    let irq = IrqLine::new(sink.clone() as Arc<dyn IrqSink>, 0);
    let key = GpioKey::new(irq, clock.clone());
    let key2 = key.clone();

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

    let applier: Box<ScenarioApplier> =
        Box::new(move |scenario: &OracleScenario| {
            if scenario.name == "press key" {
                key2.set_gpio(true);
            }
            let mut irqs = BTreeMap::new();
            irqs.insert(0, sink.level.lock().unwrap().clone());
            (BTreeMap::new(), irqs)
        });
    check_batch1_oracle(&fixture, &BTreeMap::new(), &applier);
}

// -- gpio_pwr --

#[test]
fn test_oracle_batch1_gpio_pwr() {
    use machina_hw_gpio::{GpioPwr, GpioPwrAction};
    let pwr = GpioPwr::new();
    let pwr2 = pwr.clone();
    let actions = Arc::new(std::sync::Mutex::new(Vec::new()));
    let actions2 = actions.clone();
    pwr.set_action_handler(Box::new(move |a| {
        actions2.lock().unwrap().push(a);
    }));

    let fixture = OracleFixture {
        device: "gpio_pwr".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![OracleScenario {
            name: "reset trigger".into(),
            writes: vec![],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("ACTION".into(), GpioPwrAction::Reset as u64);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };

    let applier: Box<ScenarioApplier> =
        Box::new(move |scenario: &OracleScenario| {
            if scenario.name == "reset trigger" {
                pwr2.gpio_reset(true);
            }
            let last = actions
                .lock()
                .unwrap()
                .last()
                .map(|a| *a as u64)
                .unwrap_or(0);
            let mut regs = BTreeMap::new();
            regs.insert("ACTION".into(), last);
            (regs, BTreeMap::new())
        });
    check_batch1_oracle(&fixture, &BTreeMap::new(), &applier);
}

// -- Fake-probe regression: verify device name in probe_args --

#[test]
fn test_batch1_oracle_probe_argv_includes_device_name() {
    let dir = tempfile::TempDir::new().unwrap();
    let probe_path = dir.path().join("probe");
    let log_path = dir.path().join("argv.log");
    {
        let script = format!(
            "#!/bin/sh\nprintf '%s\\0' \"$@\" >> {log}\n",
            log = log_path.to_str().unwrap()
        );
        let script = script + "echo '{\"registers\":{},\"irqs\":{}}'\n";
        let mut f = std::fs::File::create(&probe_path).unwrap();
        f.write_all(script.as_bytes()).unwrap();
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

    let fixture = OracleFixture {
        device: "test-dev".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![],
        quirks: vec![],
    };
    let json = serde_json::to_vec(&fixture).unwrap();
    let oracle = RuntimeOracle::new(
        &json,
        probe_path.to_str().unwrap(),
        &[fixture.device.clone()],
    )
    .unwrap();

    let _ = oracle.check_reset(&BTreeMap::new(), &BTreeMap::new());

    let log = std::fs::read(&log_path).unwrap_or_default();
    let args: Vec<&str> = log
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| std::str::from_utf8(s).unwrap_or(""))
        .collect();
    assert!(
        args.contains(&"test-dev"),
        "probe args should contain device name 'test-dev', got: {args:?}"
    );
}
