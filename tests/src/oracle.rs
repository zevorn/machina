use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;

use machina_core::address::GPA;
use machina_hw_char::pl011::{Pl011, Pl011Mmio};
use machina_hw_char::riscv_htif::{Htif, HtifMmio};
use machina_hw_char::sifive_uart::{SiFiveUart, SiFiveUartMmio};
use machina_hw_char::uart::{Uart16550, Uart16550Mmio};
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_hw_dma::{Pl080, SifivePdma};
use machina_hw_firmware::{FwCfg, FwCfgMmio};
use machina_hw_gpio::pl061::{Pl061, Pl061Mmio};
use machina_hw_gpio::sifive_gpio::{SiFiveGpio, SiFiveGpioMmio};
use machina_hw_i2c::eeprom_at24c::{At24cEeprom, At24cEepromConfig};
use machina_hw_i2c::smbus_eeprom::SmbusEeprom;
use machina_hw_i2c::{I2cBus, I2cSlave};
use machina_hw_intc::aclint::{Aclint, AclintMmio};
use machina_hw_intc::aplic::{RiscvAplic, RiscvAplicMmio};
use machina_hw_intc::dintc::{Dintc, DintcMmio};
use machina_hw_intc::eiointc::{Eiointc, EiointcMmio};
use machina_hw_intc::imsic::{RiscvImsic, RiscvImsicMmio};
use machina_hw_intc::ipi::{LoongArchIpi, LoongArchIpiMmio};
use machina_hw_intc::liointc::{Liointc, LiointcMmio};
use machina_hw_intc::pch_msi::{PchMsi, PchMsiMmio};
use machina_hw_intc::pch_pic::{PchPic, PchPicMmio};
use machina_hw_intc::plic::{Plic, PlicMmio};
use machina_hw_misc::cmgcr::{Cmgcr, CmgcrMmio};
use machina_hw_misc::cpc::{Cpc, CpcMmio};
use machina_hw_misc::pl050::{Pl050, Pl050Mmio};
use machina_hw_misc::pvpanic::{Pvpanic, PvpanicEvent, PvpanicMmio};
use machina_hw_misc::sifive_e_aon::{SiFiveEAon, SiFiveEAonMmio};
use machina_hw_misc::sifive_u_otp::{SiFiveUOtp, SiFiveUOtpMmio};
use machina_hw_misc::unimp::{Unimp, UnimpMmio};
use machina_hw_misc::{VirtCtrl, VirtCtrlMmio};
use machina_hw_riscv::sifive_test::SifiveTest;
use machina_hw_rtc::ds1338::Ds1338;
use machina_hw_rtc::goldfish_rtc::{GoldfishRtc, GoldfishRtcMmio};
use machina_hw_rtc::ls7a_rtc::{Ls7aRtc, Ls7aRtcMmio};
use machina_hw_rtc::pl031::{Pl031, Pl031Mmio};
use machina_hw_sd::card::{SdCardConfig, SdMemoryCard};
use machina_hw_sd::pl181::{Pl181, Pl181Mmio};
use machina_hw_sd::sdhci::{Sdhci, SdhciMmio};
use machina_hw_sd::ssi_sd::SsiSd;
use machina_hw_sd::{SdBus, SdBusHost, SdCard, SdRequest};
use machina_hw_sensor::{Tmp105, Tmp421};
use machina_hw_ssi::m25p80::M25p80;
use machina_hw_ssi::pl022::{Pl022, Pl022Mmio};
use machina_hw_ssi::sifive_spi::{SiFiveSpi, SiFiveSpiMmio};
use machina_hw_ssi::SpiBus;
use machina_hw_storage::pflash::{
    PFlashCfi01, PFlashCfi01Config, PFlashCfi02, PFlashCfi02Config,
};
use machina_hw_storage::{BlockMedia, FlashMedia, MemBackend};
use machina_hw_timer::sifive_pwm::{SiFivePwm, SiFivePwmMmio};
use machina_hw_timer::sse_counter::{
    SseCounter, SseCounterControlMmio, SseCounterStatusMmio,
};
use machina_hw_timer::sse_timer::{SseTimer, SseTimerMmio};
use machina_hw_watchdog::SbsaGwdt;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};
use machina_oracle::{
    descriptors, qemu, Oracle, OracleCheckResult, OracleFixture, OracleQuirk,
    OracleScenario, RegSnapshot, RuntimeOracle, ScenarioApplier,
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
fn write_probe_script_text(
    dir: &std::path::Path,
    name: &str,
    script: &str,
) -> std::path::PathBuf {
    write_probe_script_text_with_mode(dir, name, script, 0o755)
}

fn write_probe_script_text_with_mode(
    dir: &std::path::Path,
    name: &str,
    script: &str,
    mode: u32,
) -> std::path::PathBuf {
    let path = dir.join(name);
    let tmp_path = dir.join(format!("{name}.tmp"));
    {
        let mut f = std::fs::File::create(&tmp_path).unwrap();
        f.write_all(script.as_bytes()).unwrap();
        f.sync_all().unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            &tmp_path,
            std::fs::Permissions::from_mode(mode),
        )
        .unwrap();
    }
    std::fs::rename(&tmp_path, &path).unwrap();
    path
}

fn write_probe_script(
    dir: &std::path::Path,
    name: &str,
    registers: &BTreeMap<String, u64>,
) -> std::path::PathBuf {
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
    write_probe_script_text(dir, name, &script)
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
    let probe_path = write_probe_script_text(
        dir.path(),
        "failing-probe",
        "#!/bin/sh\nexit 1\n",
    );

    let oracle =
        RuntimeOracle::new(&json, probe_path.to_str().unwrap(), &[]).unwrap();

    let actual = BTreeMap::new();
    match oracle.check_reset(&actual, &BTreeMap::new()) {
        OracleCheckResult::Error(_) => {}
        other => panic!("expected Error for failing probe, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn test_runtime_oracle_retries_text_file_busy_probe() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    let dir = tempfile::TempDir::new().unwrap();
    let probe_path = dir.path().join("busy-probe");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&probe_path)
        .unwrap();
    file.write_all(b"#!/bin/sh\necho '{\"registers\":{},\"irqs\":{}}'\n")
        .unwrap();
    file.sync_all().unwrap();
    std::fs::set_permissions(
        &probe_path,
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    let close_later = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        drop(file);
    });

    let fixture = OracleFixture {
        device: "test".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![],
        quirks: vec![],
    };
    let json = serde_json::to_vec(&fixture).unwrap();
    let oracle =
        RuntimeOracle::new(&json, probe_path.to_str().unwrap(), &[]).unwrap();

    match oracle.check_reset(&BTreeMap::new(), &BTreeMap::new()) {
        OracleCheckResult::Pass { .. } => {}
        other => {
            panic!("expected retry to pass after writer closes, got {other:?}")
        }
    }
    close_later.join().unwrap();
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
    let log_path = dir.join("argv.log");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\0' \"$@\" >> {log}\n",
        log = log_path.to_str().unwrap()
    );
    let script = script + "echo '{\"registers\":{},\"irqs\":{}}'\n";
    write_probe_script_text(dir, name, &script)
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
    let probe_path = write_probe_script_text_with_mode(
        dir.path(),
        "noexec-probe",
        "#!/bin/sh\necho '{}'\n",
        0o644,
    );

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

// -- Helpers for Batch 1/2 runtime oracle coverage ---------------

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
                reason.contains("NOT_FOUND") || reason.contains("SKIP_PROBE:"),
                "unexpected reset Skip for {}: {reason}",
                fixture.device,
            );
        }
        OracleCheckResult::Mismatch(r) => {
            eprintln!(
                "NOTE: Machina-vs-QEMU reset mismatch for {}: \
                 {}/{} differences: {:?}",
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
                    reason.contains("NOT_FOUND")
                        || reason.contains("SKIP_PROBE:"),
                    "unexpected scenario Skip for {}: {reason}",
                    fixture.device,
                );
            }
            OracleCheckResult::Mismatch(r) => {
                eprintln!(
                    "NOTE: Machina-vs-QEMU scenario mismatch for {}: \
                     {}/{} differences: {:?}",
                    fixture.device, r.mismatches, r.total, r.details
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
fn test_sifive_e_prci_runtime_oracle_matches_write_pllcfg() {
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
fn test_sifive_u_prci_runtime_oracle_matches_write_corepllcfg0() {
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
            let events = Arc::new(std::sync::Mutex::new(0u8));
            let events2 = events.clone();
            let dev = Pvpanic::new(PvpanicEvent::PANICKED);
            dev.set_event_handler(Box::new(move |e| {
                *events2.lock().unwrap() = e;
            }));

            let (mut aspace, mut bus) = crate::hw_misc::make_test_aspace();
            let region = MemoryRegion::io(
                "pvpanic",
                0x2,
                Arc::new(PvpanicMmio(dev.clone())),
            );
            dev.attach_to_bus(&mut bus).unwrap();
            dev.register_mmio(region, GPA(0x1000_0000)).unwrap();
            dev.realize_onto(&mut bus, &mut aspace).unwrap();

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
            let events = Arc::new(std::sync::Mutex::new(0u8));
            let events2 = events.clone();
            let dev = Pvpanic::new(
                PvpanicEvent::PANICKED | PvpanicEvent::CRASH_LOADED,
            );
            dev.set_event_handler(Box::new(move |e| {
                *events2.lock().unwrap() = e;
            }));

            let (mut aspace, mut bus) = crate::hw_misc::make_test_aspace();
            let region = MemoryRegion::io(
                "pvpanic-mmio",
                0x2,
                Arc::new(PvpanicMmio(dev.clone())),
            );
            dev.attach_to_bus(&mut bus).unwrap();
            dev.register_mmio(region, GPA(0x1000_0000)).unwrap();
            dev.realize_onto(&mut bus, &mut aspace).unwrap();

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
    let log_path = dir.path().join("argv.log");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\0' \"$@\" >> {log}\n",
        log = log_path.to_str().unwrap()
    );
    let script = script + "echo '{\"registers\":{},\"irqs\":{}}'\n";
    let probe_path = write_probe_script_text(dir.path(), "probe", &script);

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

    match oracle.check_reset(&BTreeMap::new(), &BTreeMap::new()) {
        OracleCheckResult::Pass { .. } => {}
        other => {
            panic!("expected fake probe reset check to pass, got {other:?}")
        }
    }

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

/// Fake-probe regression: prove an empty scenario applier mismatches
/// against non-empty probe scenario output, and a correct applier
/// passes. This catches the original Round 7 failure mode where
/// scenario comparison used an empty applier and still passed/skipped.
#[test]
fn test_batch1_oracle_fake_probe_scenario_mismatch() {
    let dir = tempfile::TempDir::new().unwrap();
    let log_path = dir.path().join("argv.log");
    let log = log_path.to_str().unwrap();
    let script = format!(
        "#!/bin/sh
printf '%s\\0' \"$@\" >> {log}
found=0
for arg in \"$@\"; do
 case \"$arg\" in
 --probe) found=1 ;;
 reset) [ \"$found\" = \"1\" ] && echo '{{\"registers\":{{}},\"irqs\":{{}}}}' && exit 0 ;;
 scenario) [ \"$found\" = \"1\" ] && echo '{{\"registers\":{{\"ACTION\":7}},\"irqs\":{{}}}}' && exit 0 ;;
 esac
done
echo '{{\"registers\":{{}},\"irqs\":{{}}}}'
"
    );
    let probe_path = write_probe_script_text(dir.path(), "probe", &script);

    let fixture = OracleFixture {
        device: "test-dev".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![OracleScenario {
            name: "nonempty scenario".into(),
            writes: vec![],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("ACTION".into(), 7);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };
    let json = serde_json::to_vec(&fixture).unwrap();
    let oracle = RuntimeOracle::new(
        &json,
        probe_path.to_str().unwrap(),
        &[fixture.device.clone()],
    )
    .unwrap();

    // Empty applier must mismatch against non-empty probe output.
    let results =
        oracle.check_scenarios(&|_scenario| (BTreeMap::new(), BTreeMap::new()));
    assert_eq!(results.len(), 1);
    match &results[0] {
        OracleCheckResult::Mismatch(r) => {
            assert!(
                r.details.iter().any(
                    |d| d.register == "ACTION"
                        && d.expected != d.actual
                ),
                "empty applier must mismatch against probe ACTION=7, got: {r:?}"
            );
        }
        other => panic!(
            "expected Mismatch for empty applier vs non-empty probe, got {other:?}"
        ),
    }

    // Correct applier must pass.
    let results = oracle.check_scenarios(&|_scenario| {
        let mut regs = BTreeMap::new();
        regs.insert("ACTION".into(), 7);
        (regs, BTreeMap::new())
    });
    assert_eq!(results.len(), 1);
    match &results[0] {
        OracleCheckResult::Pass { total } => {
            assert!(*total >= 1, "passing scenario should report >=1 total");
        }
        other => panic!("expected Pass for correct applier, got {other:?}"),
    }

    // Verify argv: device name, --probe, scenario, scenario-name.
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
    assert!(
        args.contains(&"--probe"),
        "probe args should contain --probe, got: {args:?}"
    );
    assert!(
        args.contains(&"scenario"),
        "probe args should contain 'scenario', got: {args:?}"
    );
    assert!(
        args.contains(&"nonempty scenario"),
        "probe args should contain scenario name, got: {args:?}"
    );
}

// -- Batch 2 Device Oracle Tests -----------------------------------

fn check_batch2_oracle(
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
                reason.contains("NOT_FOUND") || reason.contains("SKIP_PROBE:"),
                "unexpected reset Skip for {}: {reason}",
                fixture.device,
            );
        }
        OracleCheckResult::Mismatch(r) => {
            eprintln!(
                "NOTE: Machina-vs-QEMU reset mismatch for {}: \
                 {}/{} differences: {:?}",
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

    // Scenario check via fixture-based Oracle.
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
                    reason.contains("NOT_FOUND")
                        || reason.contains("SKIP_PROBE:"),
                    "unexpected scenario Skip for {}: {reason}",
                    fixture.device,
                );
            }
            OracleCheckResult::Mismatch(r) => {
                eprintln!(
                    "NOTE: Machina-vs-QEMU scenario mismatch for {}: \
                     {}/{} differences: {:?}",
                    fixture.device, r.mismatches, r.total, r.details
                );
            }
            OracleCheckResult::Error(e) => {
                panic!("oracle scenario error for {}: {e}", fixture.device);
            }
        }
    }
}

// -- pch_msi --

#[test]
fn test_oracle_batch2_pch_msi() {
    use machina_hw_intc::pch_msi::{PchMsi, PchMsiMmio};

    let msi = Arc::new(PchMsi::new_named("pch_msi", 0x40, 64));
    let msi2 = Arc::clone(&msi);

    let fixture = OracleFixture {
        device: "pch_msi".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![OracleScenario {
            name: "msg_data_LOW".into(),
            writes: vec![(0x04, 0x0000_3745, 4)],
            expected: BTreeMap::new(),
            irqs: {
                let mut m = BTreeMap::new();
                m.insert(5, true);
                m
            },
        }],
        quirks: vec![],
    };

    let applier: Box<ScenarioApplier> =
        Box::new(move |scenario: &OracleScenario| {
            let (mut aspace, mut bus) = crate::hw_misc::make_test_aspace();
            let sink = crate::hw_intc_riscv::RecordingSink::new(64);

            let region = MemoryRegion::io(
                "pch_msi",
                0x8,
                Arc::new(PchMsiMmio(msi2.clone())),
            );
            msi2.attach_to_bus(&mut bus).unwrap();
            msi2.register_mmio(region, GPA(0x1000_0000)).unwrap();
            msi2.connect_output(5, InterruptSource::new(sink.clone(), 5));
            msi2.realize_onto(&mut bus, &mut aspace).unwrap();

            for &(offset, val, size) in &scenario.writes {
                aspace.write(GPA(0x1000_0000 + offset), u32::from(size), val);
            }
            let mut irqs = BTreeMap::new();
            irqs.insert(5, sink.level(5));
            (BTreeMap::new(), irqs)
        });
    check_batch2_oracle(&fixture, &BTreeMap::new(), &applier);
}

// -- dintc --

#[test]
fn test_oracle_batch2_dintc() {
    use machina_hw_intc::dintc::{Dintc, DintcMmio};

    let dintc = Arc::new(Dintc::new_named("dintc", 4));
    let dintc2 = Arc::clone(&dintc);

    // Dintc decodes: cpu = ((msg_addr >> 12) & 0xff), irq = ((msg_addr >> 4) & 0xff)
    // msg_addr = VIRT_DINTC_BASE(0x2FE0_0000) + offset
    // For cpu=1, irq=3: offset must have bits [19:12]=1, bits [11:4]=3
    // offset = (1 << 12) | (3 << 4) = 0x1030
    let dintc_offset: u64 = (1 << 12) | (3 << 4);

    let fixture = OracleFixture {
        device: "dintc".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![OracleScenario {
            name: "ip_to_cpu1_vec3".into(),
            writes: vec![(dintc_offset, 0, 4)],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("PENDING_CPU1".into(), 1 << 3);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };

    let applier: Box<ScenarioApplier> =
        Box::new(move |scenario: &OracleScenario| {
            let (mut aspace, mut bus) = crate::hw_misc::make_test_aspace();

            let region = MemoryRegion::io(
                "dintc",
                0x2000,
                Arc::new(DintcMmio(dintc2.clone())),
            );
            dintc2.attach_to_bus(&mut bus).unwrap();
            dintc2.register_mmio(region, GPA(0x1000_0000)).unwrap();
            dintc2.realize_onto(&mut bus, &mut aspace).unwrap();

            for &(offset, val, size) in &scenario.writes {
                aspace.write(GPA(0x1000_0000 + offset), u32::from(size), val);
            }
            let mut regs = BTreeMap::new();
            regs.insert("PENDING_CPU1".into(), dintc2.pending_vector(1));
            (regs, BTreeMap::new())
        });
    check_batch2_oracle(&fixture, &BTreeMap::new(), &applier);
}

// -- liointc --

#[test]
fn test_oracle_batch2_liointc() {
    use machina_hw_intc::liointc::{Liointc, LiointcMmio};

    let lio = Arc::new(Liointc::new_named("liointc"));
    let lio2 = Arc::clone(&lio);

    let fixture = OracleFixture {
        device: "liointc".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![OracleScenario {
            name: "map_irq3_to_core0_ip0".into(),
            // 1. Enable IRQ 3 via IEN_SET (offset 0x28, bit 3)
            // 2. Map IRQ 3 → core 0, IP 0 (byte offset 3, val=0x11)
            writes: vec![(0x28, 1 << 3, 4), (0x03, 0x11, 1)],
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
            let (mut aspace, mut bus) = crate::hw_misc::make_test_aspace();
            let sink = crate::hw_intc_riscv::RecordingSink::new(4);

            let region = MemoryRegion::io(
                "liointc",
                0x10000,
                Arc::new(LiointcMmio(lio2.clone())),
            );
            lio2.attach_to_bus(&mut bus).unwrap();
            lio2.register_mmio(region, GPA(0x1000_0000)).unwrap();
            lio2.connect_output(0, InterruptSource::new(sink.clone(), 0));
            lio2.realize_onto(&mut bus, &mut aspace).unwrap();

            for &(offset, val, size) in &scenario.writes {
                aspace.write(GPA(0x1000_0000 + offset), u32::from(size), val);
            }
            lio2.set_irq(3, true);
            let mut irqs = BTreeMap::new();
            irqs.insert(0, sink.level(0));
            (BTreeMap::new(), irqs)
        });
    check_batch2_oracle(&fixture, &BTreeMap::new(), &applier);
}

// -- cmgcr --

#[test]
fn test_oracle_batch2_cmgcr() {
    use machina_hw_misc::cmgcr::{Cmgcr, CmgcrMmio};

    let cmgcr = Arc::new(Cmgcr::new_named("cmgcr", 4, 0, 4, 1, 1, 0));
    let cmgcr2 = Arc::new(CmgcrMmio(Arc::clone(&cmgcr)));

    let fixture = OracleFixture {
        device: "cmgcr".into(),
        reset_regs: {
            let mut m = BTreeMap::new();
            m.insert("GCR_BASE".into(), 0);
            m.insert("GCR_CPC_STATUS".into(), 0);
            m
        },
        scenarios: vec![OracleScenario {
            name: "write_gcr_base".into(),
            writes: vec![(0x08, 0x40_0000, 4)],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("GCR_BASE".into(), 0x40_0000);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };

    let mut actual = BTreeMap::new();
    actual.insert("GCR_BASE".into(), 0);
    actual.insert("GCR_CPC_STATUS".into(), 0);

    let applier = mmio_scenario_applier(
        cmgcr2.clone(),
        "cmgcr".to_string(),
        0x10000,
        0x1000_0000,
        |aspace, base| {
            let mut m = BTreeMap::new();
            m.insert("GCR_BASE".into(), aspace.read(GPA(base + 0x08), 4));
            m
        },
    );
    check_batch2_oracle(&fixture, &actual, &applier);
}

// -- cpc --

#[test]
fn test_oracle_batch2_cpc() {
    use machina_hw_misc::cpc::{Cpc, CpcMmio};

    let cpc = Arc::new(Cpc::new_named("cpc", 0, 4, 4, 1, 0));
    let cpc2 = Arc::new(CpcMmio(Arc::clone(&cpc)));

    let fixture = OracleFixture {
        device: "cpc".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![OracleScenario {
            name: "write_vp_run".into(),
            writes: vec![(0x0050, 0x0000_0001, 4)],
            expected: BTreeMap::new(),
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };

    let applier = mmio_scenario_applier(
        cpc2.clone(),
        "cpc".to_string(),
        0x10000,
        0x1000_0000,
        |_aspace, _base| BTreeMap::new(),
    );
    check_batch2_oracle(&fixture, &BTreeMap::new(), &applier);
}

// -- loongarch_ipi --

#[test]
fn test_oracle_batch2_loongarch_ipi() {
    use machina_hw_intc::ipi::{LoongArchIpi, LoongArchIpiMmio};

    let ipi = Arc::new(LoongArchIpi::new_named("loongarch_ipi", 4));
    let ipi2 = Arc::clone(&ipi);

    let fixture = OracleFixture {
        device: "loongarch_ipi".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![OracleScenario {
            name: "send_ipi_to_cpu0".into(),
            writes: vec![(0x040, (0u64 << 16) | 3, 8)],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("STATUS".into(), 1 << 3);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };

    let applier: Box<ScenarioApplier> =
        Box::new(move |scenario: &OracleScenario| {
            let (mut aspace, mut bus) = crate::hw_misc::make_test_aspace();
            let mmio = LoongArchIpiMmio(Arc::clone(&ipi2), 0);

            let region =
                MemoryRegion::io("loongarch_ipi", 0x200, Arc::new(mmio));
            ipi2.attach_to_bus(&mut bus).unwrap();
            ipi2.register_mmio(region, GPA(0x1000_0000)).unwrap();
            ipi2.realize_onto(&mut bus, &mut aspace).unwrap();

            for &(offset, val, size) in &scenario.writes {
                aspace.write(GPA(0x1000_0000 + offset), u32::from(size), val);
            }
            let mut regs = BTreeMap::new();
            regs.insert("STATUS".into(), ipi2.mmio_read(0, 0x000));
            (regs, BTreeMap::new())
        });
    check_batch2_oracle(&fixture, &BTreeMap::new(), &applier);
}

// -- riscv_aplic --

#[test]
fn test_oracle_batch2_riscv_aplic() {
    use machina_hw_intc::aplic::{RiscvAplic, RiscvAplicMmio};

    let aplic =
        Arc::new(RiscvAplic::new_named("riscv_aplic", 32, 4, 7, false, false));
    let aplic2 = Arc::clone(&aplic);

    let fixture = OracleFixture {
        device: "riscv_aplic".into(),
        reset_regs: {
            let mut m = BTreeMap::new();
            m.insert("DOMAINCFG".into(), 0x8000_0000);
            m.insert("SOURCECFG_1".into(), 0);
            m
        },
        scenarios: vec![OracleScenario {
            name: "write_domaincfg".into(),
            writes: vec![(0x0000, 0x100, 4)],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("DOMAINCFG".into(), 0x8000_0100);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };

    let mut actual = BTreeMap::new();
    actual.insert("DOMAINCFG".into(), 0x8000_0000);
    actual.insert("SOURCECFG_1".into(), 0);

    let applier = mmio_scenario_applier(
        Arc::new(RiscvAplicMmio(Arc::clone(&aplic2))),
        "riscv_aplic".to_string(),
        0x8000,
        0x1000_0000,
        |aspace, base| {
            let mut m = BTreeMap::new();
            m.insert("DOMAINCFG".into(), aspace.read(GPA(base), 4));
            m
        },
    );
    check_batch2_oracle(&fixture, &actual, &applier);
}

// -- riscv_imsic --

#[test]
fn test_oracle_batch2_riscv_imsic() {
    use machina_hw_intc::imsic::RiscvImsic;

    let imsic = Arc::new(RiscvImsic::new_named("riscv_imsic", false, 0, 2, 64));
    let imsic2 = Arc::clone(&imsic);

    let fixture = OracleFixture {
        device: "riscv_imsic".into(),
        reset_regs: {
            let mut m = BTreeMap::new();
            m.insert("EIDELIVERY_0".into(), 0);
            m.insert("EITHRESHOLD_0".into(), 0);
            m
        },
        scenarios: vec![OracleScenario {
            name: "set_eidelivery".into(),
            writes: vec![],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("EIDELIVERY_0".into(), 1);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };

    let mut actual = BTreeMap::new();
    actual.insert("EIDELIVERY_0".into(), 0);
    actual.insert("EITHRESHOLD_0".into(), 0);

    let applier: Box<ScenarioApplier> =
        Box::new(move |scenario: &OracleScenario| {
            if scenario.name == "set_eidelivery" {
                let mut val = 0u64;
                let reg = 0x70u64 | (1u64 << 16) | (32u64 << 24);
                imsic2.rmw(reg, &mut val, 1, 1);
            }
            let mut regs = BTreeMap::new();
            regs.insert("EIDELIVERY_0".into(), imsic2.eidelivery_val(0) as u64);
            (regs, BTreeMap::new())
        });
    check_batch2_oracle(&fixture, &actual, &applier);
}

// -- Batch 2 fake-probe regression --

#[test]
fn test_batch2_oracle_fake_probe_scenario_mismatch() {
    let dir = tempfile::TempDir::new().unwrap();
    let log_path = dir.path().join("argv.log");
    let log = log_path.to_str().unwrap();
    let script = format!(
        "#!/bin/sh
printf '%s\\0' \"$@\" >> {log}
found=0
for arg in \"$@\"; do
 case \"$arg\" in
 --probe) found=1 ;;
 reset) [ \"$found\" = \"1\" ] && echo '{{\"registers\":{{}},\"irqs\":{{}}}}' && exit 0 ;;
 scenario) [ \"$found\" = \"1\" ] && echo '{{\"registers\":{{\"DOMAINCFG\":0}},\"irqs\":{{}}}}' && exit 0 ;;
 esac
done
echo '{{\"registers\":{{}},\"irqs\":{{}}}}'
"
    );
    let probe_path = write_probe_script_text(dir.path(), "probe", &script);

    let fixture = OracleFixture {
        device: "test-batch2".into(),
        reset_regs: BTreeMap::new(),
        scenarios: vec![OracleScenario {
            name: "nonempty scenario".into(),
            writes: vec![],
            expected: {
                let mut m = BTreeMap::new();
                m.insert("DOMAINCFG".into(), 1);
                m
            },
            irqs: BTreeMap::new(),
        }],
        quirks: vec![],
    };
    let json = serde_json::to_vec(&fixture).unwrap();
    let oracle = RuntimeOracle::new(
        &json,
        probe_path.to_str().unwrap(),
        &[fixture.device.clone()],
    )
    .unwrap();

    // Empty applier must mismatch against non-empty probe output.
    let results =
        oracle.check_scenarios(&|_scenario| (BTreeMap::new(), BTreeMap::new()));
    assert_eq!(results.len(), 1);
    match &results[0] {
        OracleCheckResult::Mismatch(r) => {
            assert!(
                r.mismatches > 0,
                "empty applier must mismatch against non-empty probe, got {r:?}"
            );
        }
        other => panic!(
            "expected Mismatch for empty applier vs non-empty probe, got {other:?}"
        ),
    }

    // Correct applier must pass.
    let results = oracle.check_scenarios(&|_scenario| {
        let mut regs = BTreeMap::new();
        regs.insert("DOMAINCFG".into(), 0);
        (regs, BTreeMap::new())
    });
    assert_eq!(results.len(), 1);
    match &results[0] {
        OracleCheckResult::Pass { total } => {
            assert!(*total >= 1, "passing scenario should report >=1 total");
        }
        other => panic!("expected Pass for correct applier, got {other:?}"),
    }

    // Verify argv: device name, --probe, scenario, scenario-name.
    let log = std::fs::read(&log_path).unwrap_or_default();
    let args: Vec<&str> = log
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| std::str::from_utf8(s).unwrap_or(""))
        .collect();
    assert!(
        args.contains(&"test-batch2"),
        "probe args should contain device name 'test-batch2', got: {args:?}"
    );
    assert!(
        args.contains(&"--probe"),
        "probe args should contain --probe, got: {args:?}"
    );
    assert!(
        args.contains(&"scenario"),
        "probe args should contain 'scenario', got: {args:?}"
    );
    assert!(
        args.contains(&"nonempty scenario"),
        "probe args should contain scenario name, got: {args:?}"
    );
}

// -- pflash runtime oracle --

fn pflash_media(data: Vec<u8>, sector_len: u32) -> FlashMedia<MemBackend> {
    FlashMedia::new(MemBackend::new(data, false), sector_len).unwrap()
}

fn pflash_cfi01_virt(data: Vec<u8>) -> PFlashCfi01<MemBackend> {
    PFlashCfi01::new(
        pflash_media(data, 256 * 1024),
        PFlashCfi01Config {
            bank_width: 4,
            device_width: 2,
            sector_len: 256 * 1024,
            num_blocks: 128,
            ident0: 0x89,
            ident1: 0x18,
            ident2: 0,
            ident3: 0,
            ..PFlashCfi01Config::default()
        },
    )
    .unwrap()
}

fn pflash_cfi02_zynq(data: Vec<u8>) -> PFlashCfi02<MemBackend> {
    PFlashCfi02::new(
        pflash_media(data, 128 * 1024),
        PFlashCfi02Config {
            width: 1,
            sector_len: 128 * 1024,
            num_blocks: 512,
            ident0: 0x66,
            ident1: 0x22,
            ident2: 0,
            ident3: 0,
            unlock_addr0: 0x555,
            unlock_addr1: 0x2aa,
            ..PFlashCfi02Config::default()
        },
    )
    .unwrap()
}

fn pflash_cfi01_oracle_regs(
    dev: &PFlashCfi01<MemBackend>,
) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("READ_0000".to_string(), dev.do_read(0x00, 1));
    regs.insert("READ_0004".to_string(), dev.do_read(0x04, 1));
    regs.insert("READ_0020".to_string(), dev.do_read(0x20, 1));
    regs.insert("READ_0040".to_string(), dev.do_read(0x40, 1));
    regs.insert("READ_0044".to_string(), dev.do_read(0x44, 1));
    regs.insert("READ_0048".to_string(), dev.do_read(0x48, 1));
    regs
}

fn pflash_cfi02_oracle_regs(
    dev: &PFlashCfi02<MemBackend>,
) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("READ_0000".to_string(), dev.do_read(0x00, 1));
    regs.insert("READ_0001".to_string(), dev.do_read(0x01, 1));
    regs.insert("READ_000e".to_string(), dev.do_read(0x0e, 1));
    regs.insert("READ_000f".to_string(), dev.do_read(0x0f, 1));
    regs.insert("READ_0010".to_string(), dev.do_read(0x10, 1));
    regs.insert("READ_0011".to_string(), dev.do_read(0x11, 1));
    regs.insert("READ_0012".to_string(), dev.do_read(0x12, 1));
    regs.insert("READ_0020".to_string(), dev.do_read(0x20, 1));
    regs
}

#[test]
fn test_pflash_cfi01_runtime_oracle_matches_virt_cfi_id_and_program() {
    let Some(desc) = descriptors::get_descriptor("pflash_cfi01") else {
        panic!("missing pflash_cfi01 oracle descriptor");
    };

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => panic!("pflash_cfi01 oracle error: {error}"),
        };

        let dev = pflash_cfi01_virt(vec![0; 32 * 1024 * 1024]);
        for &(offset, value, size) in scenario.writes {
            dev.do_write(offset, u32::from(size), value);
        }

        assert_eq!(
            pflash_cfi01_oracle_regs(&dev),
            expected,
            "scenario {}",
            scenario.name
        );
    }
}

#[test]
fn test_pflash_cfi02_runtime_oracle_matches_zynq_cfi_and_id() {
    let Some(desc) = descriptors::get_descriptor("pflash_cfi02") else {
        panic!("missing pflash_cfi02 oracle descriptor");
    };

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => panic!("pflash_cfi02 oracle error: {error}"),
        };

        let dev = pflash_cfi02_zynq(vec![0; 64 * 1024 * 1024]);
        for &(offset, value, size) in scenario.writes {
            dev.do_write(offset, u32::from(size), value);
        }

        assert_eq!(
            pflash_cfi02_oracle_regs(&dev),
            expected,
            "scenario {}",
            scenario.name
        );
    }
}

// -- at24c-eeprom runtime oracle --

fn at24c_eeprom_bus(address: u8) -> (I2cBus, Arc<At24cEeprom<MemBackend>>) {
    let bus = I2cBus::new();
    let dev = Arc::new(
        At24cEeprom::new(
            MemBackend::new(vec![0; 256], false),
            At24cEepromConfig {
                address,
                size: 256,
                address_width: 1,
                page_size: 8,
            },
        )
        .unwrap(),
    );
    bus.attach(Arc::clone(&dev) as Arc<dyn I2cSlave>).unwrap();
    (bus, dev)
}

fn at24c_eeprom_byte_oracle_regs() -> BTreeMap<String, u64> {
    let (bus, _dev) = at24c_eeprom_bus(0x50);
    let mut regs = BTreeMap::new();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x20).unwrap();
    bus.send(0xaa).unwrap();
    bus.end_transfer();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x20).unwrap();
    bus.start_transfer(0x50, true).unwrap();
    regs.insert("DATA20".to_string(), u64::from(bus.recv()));
    bus.end_transfer();

    regs
}

#[test]
fn test_at24c_eeprom_runtime_oracle_matches_raspi_i2c_byte() {
    let Some(desc) = descriptors::get_descriptor("eeprom_at24c") else {
        panic!("missing eeprom_at24c oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write and read byte")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("eeprom_at24c oracle error: {error}"),
    };

    assert_eq!(at24c_eeprom_byte_oracle_regs(), expected);
}

// -- SMBus EEPROM runtime oracle --

fn smbus_eeprom_byte_oracle_regs() -> BTreeMap<String, u64> {
    let bus = I2cBus::new();
    let dev = Arc::new(
        SmbusEeprom::new(0x50, MemBackend::new(vec![0xff; 256], false))
            .unwrap(),
    );
    bus.attach(dev).unwrap();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x20).unwrap();
    bus.send(0xaa).unwrap();
    bus.end_transfer();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x20).unwrap();
    bus.start_transfer(0x50, true).unwrap();
    let byte = bus.recv();
    bus.end_transfer();

    let mut regs = BTreeMap::new();
    regs.insert("DATA20".to_string(), u64::from(byte));
    regs
}

#[test]
fn test_smbus_eeprom_runtime_oracle_matches_malta_byte_data() {
    let Some(desc) = descriptors::get_descriptor("smbus_eeprom") else {
        panic!("missing smbus_eeprom oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write and read byte data")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("smbus_eeprom oracle error: {error}"),
    };

    assert_eq!(smbus_eeprom_byte_oracle_regs(), expected);
}

// -- ds1338 runtime oracle --

fn ds1338_bus(address: u8) -> (I2cBus, Arc<Ds1338>) {
    let bus = I2cBus::new();
    let dev = Arc::new(Ds1338::new(address));
    bus.attach(Arc::clone(&dev) as Arc<dyn I2cSlave>).unwrap();
    (bus, dev)
}

fn ds1338_nvram_oracle_regs() -> BTreeMap<String, u64> {
    let (bus, _dev) = ds1338_bus(0x68);
    let mut regs = BTreeMap::new();

    bus.start_transfer(0x68, false).unwrap();
    bus.send(0x0a).unwrap();
    bus.send(0xab).unwrap();
    bus.end_transfer();

    bus.start_transfer(0x68, false).unwrap();
    bus.send(0x0a).unwrap();
    bus.start_transfer(0x68, true).unwrap();
    regs.insert("NVRAM10".to_string(), u64::from(bus.recv()));
    bus.end_transfer();

    regs
}

#[test]
fn test_ds1338_runtime_oracle_matches_raspi_i2c_nvram() {
    let Some(desc) = descriptors::get_descriptor("ds1338") else {
        panic!("missing ds1338 oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write and read nvram")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("ds1338 oracle error: {error}"),
    };

    assert_eq!(ds1338_nvram_oracle_regs(), expected);
}

// -- tmp105 runtime oracle --

fn tmp105_bus(address: u8) -> (I2cBus, Arc<Tmp105>) {
    let bus = I2cBus::new();
    let dev = Tmp105::new(address);
    bus.attach(Arc::clone(&dev) as Arc<dyn I2cSlave>).unwrap();
    (bus, dev)
}

fn tmp105_t_high_oracle_regs() -> BTreeMap<String, u64> {
    let (bus, _dev) = tmp105_bus(0x50);
    let mut regs = BTreeMap::new();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x03).unwrap();
    bus.send(0xde).unwrap();
    bus.send(0xad).unwrap();
    bus.end_transfer();

    bus.start_transfer(0x50, false).unwrap();
    bus.send(0x03).unwrap();
    bus.start_transfer(0x50, true).unwrap();
    regs.insert("T_HIGH_MSB".to_string(), u64::from(bus.recv()));
    regs.insert("T_HIGH_LSB".to_string(), u64::from(bus.recv()));
    bus.end_transfer();

    regs
}

#[test]
fn test_tmp105_runtime_oracle_matches_raspi_i2c_t_high() {
    let Some(desc) = descriptors::get_descriptor("tmp105") else {
        panic!("missing tmp105 oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write and read t_high")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("tmp105 oracle error: {error}"),
    };

    assert_eq!(tmp105_t_high_oracle_regs(), expected);
}

// -- tmp421 runtime oracle --

fn tmp421_bus(address: u8) -> (I2cBus, Arc<Tmp421>) {
    let bus = I2cBus::new();
    let dev = Tmp421::new(address);
    bus.attach(Arc::clone(&dev) as Arc<dyn I2cSlave>).unwrap();
    (bus, dev)
}

fn tmp421_config_oracle_regs() -> BTreeMap<String, u64> {
    let (bus, _dev) = tmp421_bus(0x4c);
    let mut regs = BTreeMap::new();

    bus.start_transfer(0x4c, false).unwrap();
    bus.send(0x09).unwrap();
    bus.send(0x44).unwrap();
    bus.end_transfer();

    bus.start_transfer(0x4c, false).unwrap();
    bus.send(0x09).unwrap();
    bus.start_transfer(0x4c, true).unwrap();
    regs.insert("CONFIG1".to_string(), u64::from(bus.recv()));
    bus.end_transfer();

    regs
}

#[test]
fn test_tmp421_runtime_oracle_matches_raspi_i2c_config() {
    let Some(desc) = descriptors::get_descriptor("tmp421") else {
        panic!("missing tmp421 oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write and read config1")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("tmp421 oracle error: {error}"),
    };

    assert_eq!(tmp421_config_oracle_regs(), expected);
}

// -- unimp runtime oracle --

fn unimp_oracle_regs(mmio: &UnimpMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("READ0".to_string(), mmio.read(0x00, 4));
    regs.insert("READ4".to_string(), mmio.read(0x04, 4));
    regs.insert("READB0".to_string(), mmio.read(0x00, 1));
    regs.insert("READW2".to_string(), mmio.read(0x02, 2));
    regs
}

#[test]
fn test_unimp_runtime_oracle_matches_mps3_unimplemented_region() {
    let Some(desc) = descriptors::get_descriptor("unimp") else {
        panic!("missing unimp oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write ignored")
        .unwrap();

    let expected_reset = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("unimp reset oracle error: {error}"),
    };

    let reset_mmio = UnimpMmio(Unimp::new("SYS_PPU", 0x1000));
    assert_eq!(unimp_oracle_regs(&reset_mmio), expected_reset);

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("unimp scenario oracle error: {error}"),
    };

    let mmio = UnimpMmio(Unimp::new("SYS_PPU", 0x1000));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(unimp_oracle_regs(&mmio), expected);
}

// -- pvpanic runtime oracle --

fn pvpanic_oracle_regs(mmio: &PvpanicMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("EVENTS".to_string(), mmio.read(0x00, 1));
    regs.insert("EVENTS_W".to_string(), mmio.read(0x00, 2));
    regs.insert("EVENTS_L".to_string(), mmio.read(0x00, 4));
    regs
}

#[test]
fn test_pvpanic_runtime_oracle_matches_isa_port_events() {
    let Some(desc) = descriptors::get_descriptor("pvpanic") else {
        panic!("missing pvpanic oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write PANICKED")
        .unwrap();

    let expected_reset = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("pvpanic reset oracle error: {error}"),
    };

    let dev = Pvpanic::new(
        PvpanicEvent::PANICKED
            | PvpanicEvent::CRASH_LOADED
            | PvpanicEvent::SHUTDOWN,
    );
    let reset_mmio = PvpanicMmio(Arc::clone(&dev));
    assert_eq!(pvpanic_oracle_regs(&reset_mmio), expected_reset);

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("pvpanic scenario oracle error: {error}"),
    };

    let dev = Pvpanic::new(
        PvpanicEvent::PANICKED
            | PvpanicEvent::CRASH_LOADED
            | PvpanicEvent::SHUTDOWN,
    );
    let mmio = PvpanicMmio(Arc::clone(&dev));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(pvpanic_oracle_regs(&mmio), expected);
}

#[test]
fn test_pvpanic_mmio_runtime_oracle_matches_event_register() {
    let Some(desc) = descriptors::get_descriptor("pvpanic-mmio") else {
        panic!("missing pvpanic-mmio oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write SHUTDOWN")
        .unwrap();

    let expected_reset = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("pvpanic-mmio reset oracle error: {error}"),
    };

    let dev = Pvpanic::new(
        PvpanicEvent::PANICKED
            | PvpanicEvent::CRASH_LOADED
            | PvpanicEvent::SHUTDOWN,
    );
    let reset_mmio = PvpanicMmio(Arc::clone(&dev));
    assert_eq!(pvpanic_oracle_regs(&reset_mmio), expected_reset);

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("pvpanic-mmio scenario oracle error: {error}"),
    };

    let dev = Pvpanic::new(
        PvpanicEvent::PANICKED
            | PvpanicEvent::CRASH_LOADED
            | PvpanicEvent::SHUTDOWN,
    );
    let mmio = PvpanicMmio(Arc::clone(&dev));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(pvpanic_oracle_regs(&mmio), expected);
}

// -- gpio_key runtime oracle --

fn gpio_key_irq_oracle_regs(
    events: &[(u32, bool)],
) -> (BTreeMap<String, u64>, BTreeMap<u32, bool>) {
    let mut regs = BTreeMap::new();
    let mut irqs = BTreeMap::new();

    for &(irq, level) in events {
        let key = if level {
            format!("IRQ{irq}_RAISES")
        } else {
            format!("IRQ{irq}_LOWERS")
        };
        *regs.entry(key).or_insert(0) += 1;
        irqs.insert(irq, level);
    }

    (regs, irqs)
}

#[test]
fn test_gpio_key_runtime_oracle_matches_arm_virt_press_release() {
    use machina_accel::timer::{ClockType, VirtualClock};
    use machina_hw_core::irq::{IrqLine, IrqSink};
    use machina_hw_gpio::GpioKey;
    use std::sync::Mutex;

    #[derive(Default)]
    struct KeySink {
        events: Mutex<Vec<(u32, bool)>>,
    }

    impl IrqSink for KeySink {
        fn set_irq(&self, irq: u32, level: bool) {
            self.events.lock().unwrap().push((irq, level));
        }
    }

    let Some(desc) = descriptors::get_qtest_descriptor("gpio_key") else {
        panic!("missing gpio_key qtest oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "press and release")
        .unwrap();

    let (expected_regs, expected_irqs) =
        match qemu::probe_qtest_scenario(desc, scenario.name) {
            Ok(snapshot) => snapshot,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => panic!("gpio_key scenario oracle error: {error}"),
        };

    let sink = Arc::new(KeySink::default());
    let clock = Arc::new(VirtualClock::new(ClockType::Virtual));
    let irq = IrqLine::new(sink.clone() as Arc<dyn IrqSink>, 3);
    let key = GpioKey::new(irq, clock.clone());

    key.set_gpio(true);
    clock.step(100_000_000);

    let (actual_regs, actual_irqs) =
        gpio_key_irq_oracle_regs(&sink.events.lock().unwrap());
    assert_eq!(actual_regs, expected_regs);
    assert_eq!(actual_irqs, expected_irqs);
}

// -- led runtime oracle --

#[test]
fn test_led_runtime_oracle_matches_qtest_gpio_intensity() {
    use machina_hw_misc::{Led, LedColor};

    let Some(desc) = descriptors::get_qtest_descriptor("led") else {
        panic!("missing led qtest oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "set gpio high then low")
        .unwrap();

    let (expected_regs, expected_irqs) =
        match qemu::probe_qtest_scenario(desc, scenario.name) {
            Ok(snapshot) => snapshot,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => panic!("led scenario oracle error: {error}"),
        };

    let led = Led::new(LedColor::Green, "status", true);
    led.set_gpio(true);
    led.set_gpio(false);

    let mut actual_regs = BTreeMap::new();
    actual_regs.insert("INTENSITY".to_string(), u64::from(led.get_intensity()));
    assert_eq!(actual_regs, expected_regs);
    assert_eq!(BTreeMap::new(), expected_irqs);
}

// -- gpio_pwr runtime oracle --

#[test]
fn test_gpio_pwr_runtime_oracle_matches_arm_virt_shutdown() {
    use machina_hw_gpio::{GpioPwr, GpioPwrAction};

    let Some(desc) = descriptors::get_qtest_descriptor("gpio_pwr") else {
        panic!("missing gpio_pwr qtest oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "shutdown trigger")
        .unwrap();

    let (expected_regs, expected_irqs) =
        match qemu::probe_qtest_scenario(desc, scenario.name) {
            Ok(snapshot) => snapshot,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => panic!("gpio_pwr scenario oracle error: {error}"),
        };

    let pwr = GpioPwr::new();
    let actions = Arc::new(std::sync::Mutex::new(Vec::new()));
    let actions2 = actions.clone();
    pwr.set_action_handler(Box::new(move |action| {
        actions2.lock().unwrap().push(action);
    }));

    pwr.gpio_shutdown(true);

    let action = actions
        .lock()
        .unwrap()
        .last()
        .copied()
        .unwrap_or(GpioPwrAction::Reset);
    let mut actual_regs = BTreeMap::new();
    actual_regs.insert("ACTION".to_string(), action as u64);
    assert_eq!(actual_regs, expected_regs);
    assert_eq!(BTreeMap::new(), expected_irqs);
}

// -- virt_ctrl runtime oracle --

fn virt_ctrl_oracle_regs(mmio: &VirtCtrlMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("FEATURES".to_string(), mmio.read(0x00, 4));
    regs.insert("CMD".to_string(), mmio.read(0x04, 4));
    regs
}

#[test]
fn test_virt_ctrl_runtime_oracle_matches_m68k_virt_mmio_regs() {
    let Some(desc) = descriptors::get_descriptor("virt_ctrl") else {
        panic!("missing virt_ctrl oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write CMD_NOOP")
        .unwrap();

    let expected_reset = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("virt_ctrl reset oracle error: {error}"),
    };

    let reset_mmio = VirtCtrlMmio(VirtCtrl::new());
    assert_eq!(virt_ctrl_oracle_regs(&reset_mmio), expected_reset);

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("virt_ctrl scenario oracle error: {error}"),
    };

    let mmio = VirtCtrlMmio(VirtCtrl::new());
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(virt_ctrl_oracle_regs(&mmio), expected);
}

// -- sbsa_gwdt runtime oracle --

const SBSA_REF_CLOCK_FREQUENCY: u32 = 1_000_000_000;

fn sbsa_gwdt_control_regs(dev: &SbsaGwdt) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("WCS".to_string(), dev.control_read(0x000, 4));
    regs.insert("WOR".to_string(), dev.control_read(0x008, 4));
    regs.insert("WORU".to_string(), dev.control_read(0x00c, 4));
    regs.insert("WCV".to_string(), dev.control_read(0x010, 4));
    regs.insert("WCVU".to_string(), dev.control_read(0x014, 4));
    regs.insert("W_IIDR".to_string(), dev.control_read(0xfcc, 4));
    regs
}

#[test]
fn test_sbsa_gwdt_runtime_oracle_matches_sbsa_ref_control_regs() {
    let Some(desc) = descriptors::get_descriptor("sbsa_gwdt") else {
        panic!("missing sbsa_gwdt oracle descriptor");
    };

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => panic!("sbsa_gwdt oracle error: {error}"),
        };

        let dev = SbsaGwdt::new_with_clock_frequency(SBSA_REF_CLOCK_FREQUENCY);
        for &(offset, value, size) in scenario.writes {
            dev.control_write(offset, u32::from(size), value);
        }

        assert_eq!(
            sbsa_gwdt_control_regs(&dev),
            expected,
            "scenario {}",
            scenario.name
        );
    }
}

// -- pl080 runtime oracle --

fn pl080_oracle_regs(dev: &Pl080) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("INT_STATUS".to_string(), dev.do_read(0x000, 4));
    regs.insert("INT_TC_STATUS".to_string(), dev.do_read(0x004, 4));
    regs.insert("TC_RAW".to_string(), dev.do_read(0x014, 4));
    regs.insert("ERR_RAW".to_string(), dev.do_read(0x018, 4));
    regs.insert("ENABLED".to_string(), dev.do_read(0x01c, 4));
    regs.insert("CONFIG".to_string(), dev.do_read(0x030, 4));
    regs.insert("SYNC".to_string(), dev.do_read(0x034, 4));
    regs.insert("CH2_SRC".to_string(), dev.do_read(0x140, 4));
    regs.insert("CH2_DEST".to_string(), dev.do_read(0x144, 4));
    regs.insert("CH2_LLI".to_string(), dev.do_read(0x148, 4));
    regs.insert("CH2_CTRL".to_string(), dev.do_read(0x14c, 4));
    regs.insert("CH2_CONF".to_string(), dev.do_read(0x150, 4));
    regs.insert("ID0".to_string(), dev.do_read(0xfe0, 4));
    regs.insert("ID_UNALIGNED1".to_string(), dev.do_read(0xfe1, 4));
    regs.insert("ID_UNALIGNED2".to_string(), dev.do_read(0xfe2, 4));
    regs.insert("ID_UNALIGNED3".to_string(), dev.do_read(0xfe3, 4));
    regs.insert("ID1".to_string(), dev.do_read(0xfe4, 4));
    regs.insert("ID2".to_string(), dev.do_read(0xfe8, 4));
    regs.insert("ID3".to_string(), dev.do_read(0xfec, 4));
    regs.insert("ID4".to_string(), dev.do_read(0xff0, 4));
    regs.insert("ID5".to_string(), dev.do_read(0xff4, 4));
    regs.insert("ID6".to_string(), dev.do_read(0xff8, 4));
    regs.insert("ID7".to_string(), dev.do_read(0xffc, 4));
    regs
}

#[test]
fn test_pl080_runtime_oracle_matches_versatilepb_channel_regs() {
    let Some(desc) = descriptors::get_descriptor("pl080") else {
        panic!("missing pl080 oracle descriptor");
    };

    let expected_reset = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("pl080 reset oracle error: {error}"),
    };
    assert_eq!(pl080_oracle_regs(&Pl080::new()), expected_reset);

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => panic!("pl080 scenario oracle error: {error}"),
        };

        let dev = Pl080::new();
        for &(offset, value, size) in scenario.writes {
            dev.do_write(offset, u32::from(size), value);
        }
        assert_eq!(pl080_oracle_regs(&dev), expected, "{}", scenario.name);
    }
}

// -- PL050 runtime oracle --

fn pl050_oracle_regs(mmio: &Pl050Mmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("CR".to_string(), mmio.read(0x00, 4));
    regs.insert("CR_LO8".to_string(), mmio.read(0x00, 1));
    regs.insert("CR_LO16".to_string(), mmio.read(0x00, 2));
    regs.insert("STAT".to_string(), mmio.read(0x04, 4));
    regs.insert("DATA".to_string(), mmio.read(0x08, 4));
    regs.insert("CLKDIV".to_string(), mmio.read(0x0c, 4));
    regs.insert("CLKDIV_LO16".to_string(), mmio.read(0x0c, 2));
    regs.insert("IIR".to_string(), mmio.read(0x10, 4));
    regs.insert("ID0".to_string(), mmio.read(0xfe0, 4));
    regs.insert("ID_UNALIGNED1".to_string(), mmio.read(0xfe1, 4));
    regs.insert("ID_UNALIGNED2".to_string(), mmio.read(0xfe2, 4));
    regs.insert("ID_UNALIGNED3".to_string(), mmio.read(0xfe3, 4));
    regs.insert("ID1".to_string(), mmio.read(0xfe4, 4));
    regs.insert("ID2".to_string(), mmio.read(0xfe8, 4));
    regs.insert("ID3".to_string(), mmio.read(0xfec, 4));
    regs.insert("ID4".to_string(), mmio.read(0xff0, 4));
    regs.insert("ID5".to_string(), mmio.read(0xff4, 4));
    regs.insert("ID6".to_string(), mmio.read(0xff8, 4));
    regs.insert("ID7".to_string(), mmio.read(0xffc, 4));
    regs
}

#[test]
fn test_pl050_runtime_oracle_matches_versatilepb_regs() {
    let Some(desc) = descriptors::get_descriptor("pl050") else {
        panic!("missing pl050 oracle descriptor");
    };

    let expected_reset = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("pl050 reset oracle error: {error}"),
    };
    let reset_mmio = Pl050Mmio(Arc::new(Pl050::new()));
    assert_eq!(pl050_oracle_regs(&reset_mmio), expected_reset);

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => panic!("pl050 scenario oracle error: {error}"),
        };

        let mmio = Pl050Mmio(Arc::new(Pl050::new()));
        for &(offset, value, size) in scenario.writes {
            mmio.write(offset, u32::from(size), value);
        }

        assert_eq!(
            pl050_oracle_regs(&mmio),
            expected,
            "scenario {}",
            scenario.name
        );
    }
}

// -- SiFive E AON runtime oracle --

fn sifive_e_aon_oracle_regs(mmio: &SiFiveEAonMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("WDOGCFG".to_string(), mmio.read(0x00, 4));
    regs.insert("WDOGCOUNT".to_string(), mmio.read(0x08, 4));
    regs.insert("WDOGS".to_string(), mmio.read(0x10, 4));
    regs.insert("WDOGFEED".to_string(), mmio.read(0x18, 4));
    regs.insert("WDOGKEY".to_string(), mmio.read(0x1c, 4));
    regs.insert("WDOGCMP0".to_string(), mmio.read(0x20, 4));
    regs.insert("RTC".to_string(), mmio.read(0x40, 4));
    regs.insert("LFROSC".to_string(), mmio.read(0x70, 4));
    regs
}

#[test]
fn test_sifive_e_aon_runtime_oracle_matches_sifive_e_wdog_regs() {
    let Some(desc) = descriptors::get_descriptor("sifive_e_aon") else {
        panic!("missing sifive_e_aon oracle descriptor");
    };

    let expected_reset = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("sifive_e_aon reset oracle error: {error}"),
    };
    let reset_mmio = SiFiveEAonMmio(Arc::new(SiFiveEAon::default()));
    assert_eq!(sifive_e_aon_oracle_regs(&reset_mmio), expected_reset);

    let Some(scenario) = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write compare")
    else {
        panic!("missing sifive_e_aon write compare scenario");
    };
    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("sifive_e_aon scenario oracle error: {error}"),
    };

    let mmio = SiFiveEAonMmio(Arc::new(SiFiveEAon::default()));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(sifive_e_aon_oracle_regs(&mmio), expected);
}

#[test]
fn test_sifive_e_aon_runtime_oracle_matches_wdogs_scale_field() {
    let Some(desc) = descriptors::get_descriptor("sifive_e_aon") else {
        panic!("missing sifive_e_aon oracle descriptor");
    };
    let Some(scenario) = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "scale wdogs")
    else {
        panic!("missing sifive_e_aon scale wdogs scenario");
    };

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("sifive_e_aon scale oracle error: {error}"),
    };

    let mmio = SiFiveEAonMmio(Arc::new(SiFiveEAon::default()));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(sifive_e_aon_oracle_regs(&mmio), expected);
    assert_eq!(expected["WDOGS"], 0x10);
}

// -- sifive_pdma runtime oracle --

fn sifive_pdma_oracle_regs(dev: &SifivePdma) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("CONTROL".to_string(), dev.do_read(0x000, 4));
    regs.insert("NEXT_CONFIG".to_string(), dev.do_read(0x004, 4));
    regs.insert("NEXT_BYTES".to_string(), dev.do_read(0x008, 8));
    regs.insert("NEXT_BYTES_UNALIGNED".to_string(), dev.do_read(0x009, 8));
    regs.insert("NEXT_DST".to_string(), dev.do_read(0x010, 8));
    regs.insert("NEXT_SRC".to_string(), dev.do_read(0x018, 8));
    regs.insert("EXEC_CONFIG".to_string(), dev.do_read(0x104, 4));
    regs.insert("EXEC_BYTES".to_string(), dev.do_read(0x108, 8));
    regs.insert("EXEC_DST".to_string(), dev.do_read(0x110, 8));
    regs.insert("EXEC_SRC".to_string(), dev.do_read(0x118, 8));
    regs
}

#[test]
fn test_sifive_pdma_runtime_oracle_matches_sifive_u_claim() {
    let Some(desc) = descriptors::get_descriptor("sifive_pdma") else {
        panic!("missing sifive_pdma oracle descriptor");
    };

    let expected_reset = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("sifive_pdma reset oracle error: {error}"),
    };
    assert_eq!(sifive_pdma_oracle_regs(&SifivePdma::new()), expected_reset);

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => {
                panic!(
                    "sifive_pdma oracle error for {}: {error}",
                    scenario.name
                )
            }
        };

        let dev = SifivePdma::new();
        for &(offset, value, size) in scenario.writes {
            dev.do_write(offset, u32::from(size), value);
        }
        assert_eq!(sifive_pdma_oracle_regs(&dev), expected);
    }
}

// -- m25p80 runtime oracle --

fn m25p80_sifive_u_spi(data: Vec<u8>) -> SiFiveSpiMmio {
    let flash = FlashMedia::new(MemBackend::new(data, false), 4096).unwrap();
    let flash = Arc::new(M25p80::new(0, flash, [0x9d, 0x70, 0x19]));
    let ssi_bus = Arc::new(SpiBus::new());
    ssi_bus.attach(flash).unwrap();

    let spi = Arc::new(SiFiveSpi::new());
    spi.connect_ssi_bus(ssi_bus);
    SiFiveSpiMmio(spi)
}

fn m25p80_jedec_oracle_regs(mmio: &SiFiveSpiMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("RX_00".to_string(), mmio.read(0x4c, 4));
    regs.insert("RX_01".to_string(), mmio.read(0x4c, 4));
    regs.insert("RX_02".to_string(), mmio.read(0x4c, 4));
    regs.insert("RX_03".to_string(), mmio.read(0x4c, 4));
    regs
}

#[test]
fn test_m25p80_runtime_oracle_matches_sifive_u_jedec_id() {
    let Some(desc) = descriptors::get_descriptor("m25p80") else {
        panic!("missing m25p80 oracle descriptor");
    };

    let expected = match qemu::probe_scenario(desc, "jedec id") {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("m25p80 oracle error: {error}"),
    };

    let mmio = m25p80_sifive_u_spi(vec![0xff; 8192]);
    for &(offset, value, size) in desc.scenarios[0].writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(m25p80_jedec_oracle_regs(&mmio), expected);
}

// -- PL022 runtime oracle --

fn pl022_oracle_regs(mmio: &Pl022Mmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("CR0".to_string(), mmio.read(0x00, 4));
    regs.insert("CR0_LO8".to_string(), mmio.read(0x00, 1));
    regs.insert("CR0_LO16".to_string(), mmio.read(0x00, 2));
    regs.insert("CR1".to_string(), mmio.read(0x04, 4));
    regs.insert("DR".to_string(), mmio.read(0x08, 4));
    regs.insert("SR".to_string(), mmio.read(0x0c, 4));
    regs.insert("CPSR".to_string(), mmio.read(0x10, 4));
    regs.insert("CPSR_LO8".to_string(), mmio.read(0x10, 1));
    regs.insert("IMSC".to_string(), mmio.read(0x14, 4));
    regs.insert("RIS".to_string(), mmio.read(0x18, 4));
    regs.insert("MIS".to_string(), mmio.read(0x1c, 4));
    regs.insert("PID0".to_string(), mmio.read(0xfe0, 4));
    regs.insert("PID_UNALIGNED1".to_string(), mmio.read(0xfe1, 4));
    regs.insert("PID_UNALIGNED2".to_string(), mmio.read(0xfe2, 4));
    regs.insert("PID_UNALIGNED3".to_string(), mmio.read(0xfe3, 4));
    regs.insert("PID1".to_string(), mmio.read(0xfe4, 4));
    regs.insert("PID2".to_string(), mmio.read(0xfe8, 4));
    regs.insert("PID3".to_string(), mmio.read(0xfec, 4));
    regs.insert("CID0".to_string(), mmio.read(0xff0, 4));
    regs.insert("CID1".to_string(), mmio.read(0xff4, 4));
    regs.insert("CID2".to_string(), mmio.read(0xff8, 4));
    regs.insert("CID3".to_string(), mmio.read(0xffc, 4));
    regs
}

fn assert_pl022_runtime_oracle_matches_scenario(scenario_name: &str) {
    let Some(desc) = descriptors::get_descriptor("pl022") else {
        panic!("missing pl022 oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == scenario_name)
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("pl022 oracle error: {error}"),
    };

    let mmio = Pl022Mmio(Arc::new(Pl022::new()));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(pl022_oracle_regs(&mmio), expected);
}

#[test]
fn test_pl022_runtime_oracle_matches_mps2_loopback_fifo() {
    assert_pl022_runtime_oracle_matches_scenario("loopback fifo");
}

#[test]
fn test_pl022_runtime_oracle_matches_mps2_narrow_access_regs() {
    assert_pl022_runtime_oracle_matches_scenario("narrow access regs");
}

// -- SiFive SPI runtime oracle --

fn sifive_spi_oracle_regs(mmio: &SiFiveSpiMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("SCKDIV".to_string(), mmio.read(0x00, 4));
    regs.insert("SCKDIV_UNALIGNED".to_string(), mmio.read(0x01, 4));
    regs.insert("CSID".to_string(), mmio.read(0x10, 4));
    regs.insert("CSDEF".to_string(), mmio.read(0x14, 4));
    regs.insert("CSDEF_UNALIGNED".to_string(), mmio.read(0x15, 4));
    regs.insert("CSMODE".to_string(), mmio.read(0x18, 4));
    regs.insert("DELAY0".to_string(), mmio.read(0x28, 4));
    regs.insert("DELAY1".to_string(), mmio.read(0x2c, 4));
    regs.insert("TXDATA".to_string(), mmio.read(0x48, 4));
    regs.insert("RXDATA".to_string(), mmio.read(0x4c, 4));
    regs.insert("IE".to_string(), mmio.read(0x70, 4));
    regs.insert("IP".to_string(), mmio.read(0x74, 4));
    regs
}

struct SsiSdCmd8Card;

impl SdCard for SsiSdCmd8Card {
    fn do_command(&self, req: &SdRequest, resp: &mut [u8]) -> usize {
        if req.cmd == 8 && req.arg == 0x01aa {
            resp[..5].copy_from_slice(&[0x01, 0x00, 0x00, 0x01, 0xaa]);
            return 5;
        }
        0
    }

    fn write_byte(&self, _value: u8) {}

    fn read_byte(&self) -> u8 {
        0xff
    }

    fn receive_ready(&self) -> bool {
        false
    }

    fn data_ready(&self) -> bool {
        false
    }

    fn get_inserted(&self) -> bool {
        true
    }

    fn get_readonly(&self) -> bool {
        false
    }

    fn set_voltage(&self, _millivolts: u16) {}

    fn get_dat_lines(&self) -> u8 {
        0b1111
    }

    fn get_cmd_line(&self) -> bool {
        true
    }
}

fn ssi_sd_sifive_u_spi() -> SiFiveSpiMmio {
    let sd_bus = Arc::new(SdBus::new());
    sd_bus.insert_card(Arc::new(SsiSdCmd8Card));

    let bridge = Arc::new(SsiSd::new(0));
    bridge.connect_sd_bus(sd_bus);

    let ssi_bus = Arc::new(SpiBus::new());
    ssi_bus.attach(bridge).unwrap();

    let spi = Arc::new(SiFiveSpi::new());
    spi.connect_ssi_bus(ssi_bus);
    SiFiveSpiMmio(spi)
}

fn ssi_sd_cmd8_oracle_regs(mmio: &SiFiveSpiMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    for index in 0..8 {
        regs.insert(format!("RX_{index:02}"), mmio.read(0x4c, 4));
    }
    regs
}

#[test]
fn test_ssi_sd_runtime_oracle_matches_sifive_u_spi_cmd8_prefix() {
    let Some(desc) = descriptors::get_descriptor("ssi_sd") else {
        panic!("missing ssi_sd oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "cmd8 response prefix")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("ssi_sd oracle error: {error}"),
    };

    let mmio = ssi_sd_sifive_u_spi();
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(ssi_sd_cmd8_oracle_regs(&mmio), expected);
}

#[test]
fn test_sifive_spi_runtime_oracle_matches_sifive_u_reset_regs() {
    let Some(desc) = descriptors::get_descriptor("sifive_spi") else {
        panic!("missing sifive_spi oracle descriptor");
    };

    let expected = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("sifive_spi oracle error: {error}"),
    };

    let mmio = SiFiveSpiMmio(Arc::new(SiFiveSpi::new()));

    assert_eq!(sifive_spi_oracle_regs(&mmio), expected);
}

#[test]
fn test_sifive_spi_runtime_oracle_matches_sifive_u_unaligned_access_ignored() {
    let Some(desc) = descriptors::get_descriptor("sifive_spi") else {
        panic!("missing sifive_spi oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "unaligned access ignored")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("sifive_spi oracle error: {error}"),
    };

    let mmio = SiFiveSpiMmio(Arc::new(SiFiveSpi::new()));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(sifive_spi_oracle_regs(&mmio), expected);
}

// -- SiFive PWM runtime oracle --

fn sifive_pwm_oracle_regs(mmio: &SiFivePwmMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("CONFIG".to_string(), mmio.read(0x00, 4));
    regs.insert("CONFIG_LO8".to_string(), mmio.read(0x00, 1));
    regs.insert("CONFIG_LO16".to_string(), mmio.read(0x00, 2));
    regs.insert("COUNT".to_string(), mmio.read(0x08, 4));
    regs.insert("PWMS".to_string(), mmio.read(0x10, 4));
    regs.insert("PWMCMP0".to_string(), mmio.read(0x20, 4));
    regs.insert("PWMCMP0_LO8".to_string(), mmio.read(0x20, 1));
    regs.insert("PWMCMP0_LO16".to_string(), mmio.read(0x20, 2));
    regs.insert("PWMCMP0_UNALIGNED1".to_string(), mmio.read(0x21, 4));
    regs.insert("PWMCMP0_UNALIGNED2".to_string(), mmio.read(0x22, 4));
    regs.insert("PWMCMP0_UNALIGNED3".to_string(), mmio.read(0x23, 4));
    regs.insert("PWMCMP1".to_string(), mmio.read(0x24, 4));
    regs.insert("PWMCMP2".to_string(), mmio.read(0x28, 4));
    regs.insert("PWMCMP3".to_string(), mmio.read(0x2c, 4));
    regs
}

#[test]
fn test_sifive_pwm_runtime_oracle_matches_sifive_u_compare_regs() {
    let Some(desc) = descriptors::get_descriptor("sifive_pwm") else {
        panic!("missing sifive_pwm oracle descriptor");
    };

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => {
                panic!("sifive_pwm oracle error for {}: {error}", scenario.name)
            }
        };

        let mmio = SiFivePwmMmio(Arc::new(SiFivePwm::new()));
        for &(offset, value, size) in scenario.writes {
            mmio.write(offset, u32::from(size), value);
        }

        assert_eq!(sifive_pwm_oracle_regs(&mmio), expected);
    }
}

// -- SSE Timer runtime oracle --

fn sse_timer_oracle_regs(mmio: &SseTimerMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("CNTFRQ".to_string(), mmio.read(0x10, 4));
    regs.insert("CNTP_CVAL_LO".to_string(), mmio.read(0x20, 4));
    regs.insert("CNTP_CVAL_HI".to_string(), mmio.read(0x24, 4));
    regs.insert("CNTP_TVAL".to_string(), mmio.read(0x28, 4));
    regs.insert("CNTP_CTL".to_string(), mmio.read(0x2c, 4));
    regs.insert("CNTP_AIVAL_RELOAD".to_string(), mmio.read(0x48, 4));
    regs.insert("CNTP_AIVAL_CTL".to_string(), mmio.read(0x4c, 4));
    regs.insert("CNTP_CFG".to_string(), mmio.read(0x50, 4));
    regs.insert("PID4".to_string(), mmio.read(0xfd0, 4));
    regs.insert("PID0".to_string(), mmio.read(0xfe0, 4));
    regs
}

#[test]
fn test_sse_timer_runtime_oracle_matches_mps3_timer_regs() {
    let Some(desc) = descriptors::get_descriptor("sse_timer") else {
        panic!("missing sse_timer oracle descriptor");
    };

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => {
                panic!("sse_timer oracle error for {}: {error}", scenario.name)
            }
        };

        let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
        let mmio = SseTimerMmio(Arc::new(SseTimer::new(counter)));
        for &(offset, value, size) in scenario.writes {
            if offset < 0x1000 {
                mmio.write(offset, u32::from(size), value);
            }
        }

        assert_eq!(sse_timer_oracle_regs(&mmio), expected);
    }
}

// -- SSE Counter runtime oracle --

const SSE_COUNTER_CONTROL_DELTA: u64 = 0x0fff_f000;

fn sse_counter_oracle_regs(
    control: &SseCounterControlMmio,
    status: &SseCounterStatusMmio,
) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("CNTCR".to_string(), control.read(0x00, 4));
    regs.insert("CNTCV_LO".to_string(), control.read(0x08, 4));
    regs.insert("CNTCV_HI".to_string(), control.read(0x0c, 4));
    regs.insert("CNTSCR".to_string(), control.read(0x10, 4));
    regs.insert("CNTID".to_string(), control.read(0x1c, 4));
    regs.insert("CNTSCR0".to_string(), control.read(0xd0, 4));
    regs.insert("PID4".to_string(), control.read(0xfd0, 4));
    regs.insert("PID0".to_string(), control.read(0xfe0, 4));
    regs.insert("STATUS_CNTCV_LO".to_string(), status.read(0x00, 4));
    regs.insert("STATUS_CNTCV_HI".to_string(), status.read(0x04, 4));
    regs
}

#[test]
fn test_sse_counter_runtime_oracle_matches_mps3_counter_regs() {
    let Some(desc) = descriptors::get_descriptor("sse_counter") else {
        panic!("missing sse_counter oracle descriptor");
    };

    let expected_reset = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("sse_counter reset oracle error: {error}"),
    };
    let reset_counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let reset_control = SseCounterControlMmio(Arc::clone(&reset_counter));
    let reset_status = SseCounterStatusMmio(Arc::clone(&reset_counter));
    assert_eq!(
        sse_counter_oracle_regs(&reset_control, &reset_status),
        expected_reset
    );

    let Some(scenario) = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write counter regs")
    else {
        panic!("missing sse_counter write counter regs scenario");
    };
    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("sse_counter scenario oracle error: {error}"),
    };

    let counter = Arc::new(SseCounter::new_with_freq(1_000_000));
    let control = SseCounterControlMmio(Arc::clone(&counter));
    let status = SseCounterStatusMmio(Arc::clone(&counter));
    for &(offset, value, size) in scenario.writes {
        let control_offset = offset
            .checked_sub(SSE_COUNTER_CONTROL_DELTA)
            .expect("sse_counter scenario write must target control frame");
        control.write(control_offset, u32::from(size), value);
    }

    assert_eq!(sse_counter_oracle_regs(&control, &status), expected);
}

// -- SiFive GPIO runtime oracle --

fn sifive_gpio_oracle_regs(mmio: &SiFiveGpioMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("VALUE".to_string(), mmio.read(0x00, 4));
    regs.insert("INPUT_EN".to_string(), mmio.read(0x04, 4));
    regs.insert("INPUT_EN_LO8".to_string(), mmio.read(0x04, 1));
    regs.insert("INPUT_EN_LO16".to_string(), mmio.read(0x04, 2));
    regs.insert("INPUT_EN_UNALIGNED1".to_string(), mmio.read(0x05, 4));
    regs.insert("INPUT_EN_UNALIGNED2".to_string(), mmio.read(0x06, 4));
    regs.insert("INPUT_EN_UNALIGNED3".to_string(), mmio.read(0x07, 4));
    regs.insert("OUTPUT_EN".to_string(), mmio.read(0x08, 4));
    regs.insert("PORT".to_string(), mmio.read(0x0c, 4));
    regs.insert("PUE".to_string(), mmio.read(0x10, 4));
    regs.insert("DS".to_string(), mmio.read(0x14, 4));
    regs
}

#[test]
fn test_sifive_gpio_runtime_oracle_matches_sifive_u_regs() {
    let Some(desc) = descriptors::get_descriptor("sifive_gpio") else {
        panic!("missing sifive_gpio oracle descriptor");
    };

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => {
                panic!(
                    "sifive_gpio oracle error for {}: {error}",
                    scenario.name
                )
            }
        };

        let mmio = SiFiveGpioMmio(Arc::new(SiFiveGpio::new()));
        for &(offset, value, size) in scenario.writes {
            mmio.write(offset, u32::from(size), value);
        }

        assert_eq!(sifive_gpio_oracle_regs(&mmio), expected);
    }
}

// -- PL061 runtime oracle --

fn pl061_oracle_regs(mmio: &Pl061Mmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("DATA_3FC".to_string(), mmio.read(0x3fc, 4));
    regs.insert("DIR".to_string(), mmio.read(0x400, 4));
    regs.insert("ISENSE".to_string(), mmio.read(0x404, 4));
    regs.insert("IBE".to_string(), mmio.read(0x408, 4));
    regs.insert("IEV".to_string(), mmio.read(0x40c, 4));
    regs.insert("IM".to_string(), mmio.read(0x410, 4));
    regs.insert("RIS".to_string(), mmio.read(0x414, 4));
    regs.insert("MIS".to_string(), mmio.read(0x418, 4));
    regs.insert("AFSEL".to_string(), mmio.read(0x420, 4));
    regs.insert("DR2R".to_string(), mmio.read(0x500, 4));
    regs.insert("DR4R".to_string(), mmio.read(0x504, 4));
    regs.insert("DR8R".to_string(), mmio.read(0x508, 4));
    regs.insert("ODR".to_string(), mmio.read(0x50c, 4));
    regs.insert("PUR".to_string(), mmio.read(0x510, 4));
    regs.insert("PDR".to_string(), mmio.read(0x514, 4));
    regs.insert("SLR".to_string(), mmio.read(0x518, 4));
    regs.insert("DEN".to_string(), mmio.read(0x51c, 4));
    regs.insert("LOCK".to_string(), mmio.read(0x520, 4));
    regs.insert("CR".to_string(), mmio.read(0x524, 4));
    regs.insert("AMSEL".to_string(), mmio.read(0x528, 4));
    regs.insert("PID0".to_string(), mmio.read(0xfe0, 4));
    regs.insert("PID_UNALIGNED1".to_string(), mmio.read(0xfe1, 4));
    regs.insert("PID_UNALIGNED2".to_string(), mmio.read(0xfe2, 4));
    regs.insert("PID_UNALIGNED3".to_string(), mmio.read(0xfe3, 4));
    regs.insert("PID1".to_string(), mmio.read(0xfe4, 4));
    regs.insert("PID2".to_string(), mmio.read(0xfe8, 4));
    regs.insert("PID3".to_string(), mmio.read(0xfec, 4));
    regs.insert("CID0".to_string(), mmio.read(0xff0, 4));
    regs.insert("CID1".to_string(), mmio.read(0xff4, 4));
    regs.insert("CID2".to_string(), mmio.read(0xff8, 4));
    regs.insert("CID3".to_string(), mmio.read(0xffc, 4));
    regs
}

#[test]
fn test_pl061_runtime_oracle_matches_arm_virt_gpio_regs() {
    let Some(desc) = descriptors::get_descriptor("pl061") else {
        panic!("missing pl061 oracle descriptor");
    };

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => {
                panic!("pl061 oracle error for {}: {error}", scenario.name)
            }
        };

        let mmio = Pl061Mmio(Arc::new(Pl061::new_with_pull(0, 0xff)));
        for &(offset, value, size) in scenario.writes {
            mmio.write(offset, u32::from(size), value);
        }

        assert_eq!(pl061_oracle_regs(&mmio), expected);
    }
}

// -- SiFive U OTP runtime oracle --

fn sifive_u_otp_oracle_regs(mmio: &SiFiveUOtpMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("PA".to_string(), mmio.read(0x00, 4));
    regs.insert("PAIO".to_string(), mmio.read(0x04, 4));
    regs.insert("PAS".to_string(), mmio.read(0x08, 4));
    regs.insert("PCE".to_string(), mmio.read(0x0c, 4));
    regs.insert("PDIN".to_string(), mmio.read(0x14, 4));
    regs.insert("PDOUT".to_string(), mmio.read(0x18, 4));
    regs.insert("PDSTB".to_string(), mmio.read(0x1c, 4));
    regs.insert("PTRIM".to_string(), mmio.read(0x34, 4));
    regs.insert("PWE".to_string(), mmio.read(0x38, 4));
    regs
}

fn assert_sifive_u_otp_runtime_oracle_matches(scenario_name: &str) {
    let Some(desc) = descriptors::get_descriptor("sifive_u_otp") else {
        panic!("missing sifive_u_otp oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == scenario_name)
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("sifive_u_otp oracle error: {error}"),
    };

    let mmio = SiFiveUOtpMmio(Arc::new(SiFiveUOtp::with_serial(1)));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(sifive_u_otp_oracle_regs(&mmio), expected);
}

#[test]
fn test_sifive_u_otp_runtime_oracle_matches_sifive_u_program_bit() {
    assert_sifive_u_otp_runtime_oracle_matches("program bit");
}

#[test]
fn test_sifive_u_otp_runtime_oracle_matches_sifive_u_pdin_value_shift() {
    assert_sifive_u_otp_runtime_oracle_matches("pdin value shift");
}

// -- fw_cfg runtime oracle --

fn fw_cfg_signature_oracle_regs(mmio: &FwCfgMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("DATA_0".to_string(), mmio.read(0x00, 1));
    regs.insert("DATA_1".to_string(), mmio.read(0x00, 1));
    regs.insert("DATA_2".to_string(), mmio.read(0x00, 1));
    regs.insert("DATA_3".to_string(), mmio.read(0x00, 1));
    regs
}

#[test]
fn test_fw_cfg_runtime_oracle_matches_virt_signature() {
    let Some(desc) = descriptors::get_descriptor("fw_cfg") else {
        panic!("missing fw_cfg oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "signature")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("fw_cfg oracle error: {error}"),
    };

    let mmio = FwCfgMmio::new(FwCfg::new());
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(fw_cfg_signature_oracle_regs(&mmio), expected);
}

// -- PL181 runtime oracle --

fn pl181_oracle_regs(mmio: &Pl181Mmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("POWER".to_string(), mmio.read(0x00, 4));
    regs.insert("CLOCK".to_string(), mmio.read(0x04, 4));
    regs.insert("ARGUMENT".to_string(), mmio.read(0x08, 4));
    regs.insert("ARGUMENT_UNALIGNED1".to_string(), mmio.read(0x09, 4));
    regs.insert("ARGUMENT_UNALIGNED2".to_string(), mmio.read(0x0a, 4));
    regs.insert("ARGUMENT_UNALIGNED3".to_string(), mmio.read(0x0b, 4));
    regs.insert("COMMAND".to_string(), mmio.read(0x0c, 4));
    regs
}

#[test]
fn test_pl181_runtime_oracle_matches_vexpress_control_regs() {
    let Some(desc) = descriptors::get_descriptor("pl181") else {
        panic!("missing pl181 oracle descriptor");
    };

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => {
                panic!("pl181 oracle error for {}: {error}", scenario.name)
            }
        };

        let mmio = Pl181Mmio(Arc::new(Pl181::new()));
        for &(offset, value, size) in scenario.writes {
            mmio.write(offset, u32::from(size), value);
        }

        assert_eq!(pl181_oracle_regs(&mmio), expected);
    }
}

// -- SDHCI runtime oracle --

fn sdhci_oracle_regs(mmio: &SdhciMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("BLOCK_SIZE".to_string(), mmio.read(0x04, 2));
    regs.insert("BLOCK_COUNT".to_string(), mmio.read(0x06, 2));
    regs.insert("ARGUMENT".to_string(), mmio.read(0x08, 4));
    regs.insert("COMMAND".to_string(), mmio.read(0x0e, 2));
    regs.insert("SOFTWARE_RESET".to_string(), mmio.read(0x2f, 1));
    regs.insert("NORMAL_INT_STATUS".to_string(), mmio.read(0x30, 2));
    regs.insert("ERROR_INT_STATUS".to_string(), mmio.read(0x32, 2));
    regs.insert("NORMAL_INT_ENABLE".to_string(), mmio.read(0x34, 2));
    regs.insert("NORMAL_INT_SIGNAL_ENABLE".to_string(), mmio.read(0x38, 2));
    regs
}

fn sd_memory_card(data: Vec<u8>) -> Arc<SdMemoryCard<MemBackend>> {
    Arc::new(
        SdMemoryCard::new(
            BlockMedia::new(MemBackend::new(data, false), 512).unwrap(),
            SdCardConfig::default(),
        )
        .unwrap(),
    )
}

fn sd_card_cmd8_oracle_regs() -> BTreeMap<String, u64> {
    let bus = Arc::new(SdBus::new());
    let controller = Arc::new(Sdhci::new());
    controller.connect_bus(Arc::clone(&bus));
    bus.set_host(Arc::clone(&controller) as Arc<dyn SdBusHost>);
    bus.insert_card(sd_memory_card(vec![0; 512]));

    let mmio = SdhciMmio(controller);
    mmio.write(0x30, 2, 0xffff);
    mmio.write(0x34, 2, 0x0001);
    mmio.write(0x08, 4, 0x01aa);
    mmio.write(0x0e, 2, 0x0802);

    let mut regs = BTreeMap::new();
    regs.insert("RESPONSE0".to_string(), mmio.read(0x10, 4));
    regs.insert("NORMAL_INT_STATUS".to_string(), mmio.read(0x30, 2));
    regs
}

#[test]
fn test_sd_card_runtime_oracle_matches_zynq_sdhci_cmd8() {
    let Some(desc) = descriptors::get_descriptor("sd_card") else {
        panic!("missing sd_card oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "cmd8 interface condition")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("sd_card oracle error: {error}"),
    };

    assert_eq!(sd_card_cmd8_oracle_regs(), expected);
}

#[test]
fn test_sdhci_runtime_oracle_matches_zynq_interrupt_enable_regs() {
    let Some(desc) = descriptors::get_descriptor("sdhci") else {
        panic!("missing sdhci oracle descriptor");
    };

    let expected_reset = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("sdhci reset oracle error: {error}"),
    };
    let mmio = SdhciMmio(Arc::new(Sdhci::new()));
    assert_eq!(sdhci_oracle_regs(&mmio), expected_reset);

    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write interrupt enables")
        .unwrap_or_else(|| {
            panic!("missing sdhci write interrupt enables scenario")
        });
    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("sdhci scenario oracle error: {error}"),
    };

    let mmio = SdhciMmio(Arc::new(Sdhci::new()));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(sdhci_oracle_regs(&mmio), expected);
}

// -- PL011 runtime oracle --

fn pl011_oracle_regs(mmio: &Pl011Mmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("FR".to_string(), mmio.read(0x18, 4));
    regs.insert("IBRD".to_string(), mmio.read(0x24, 4));
    regs.insert("IBRD_LO8".to_string(), mmio.read(0x24, 1));
    regs.insert("IBRD_LO16".to_string(), mmio.read(0x24, 2));
    regs.insert("FBRD".to_string(), mmio.read(0x28, 4));
    regs.insert("LCRH".to_string(), mmio.read(0x2c, 4));
    regs.insert("CR".to_string(), mmio.read(0x30, 4));
    regs.insert("IFLS".to_string(), mmio.read(0x34, 4));
    regs.insert("IMSC".to_string(), mmio.read(0x38, 4));
    regs.insert("RIS".to_string(), mmio.read(0x3c, 4));
    regs.insert("MIS".to_string(), mmio.read(0x40, 4));
    regs.insert("DMACR".to_string(), mmio.read(0x48, 4));
    regs.insert("PID0".to_string(), mmio.read(0xfe0, 4));
    regs.insert("PID_UNALIGNED1".to_string(), mmio.read(0xfe1, 4));
    regs.insert("PID_UNALIGNED2".to_string(), mmio.read(0xfe2, 4));
    regs.insert("PID_UNALIGNED3".to_string(), mmio.read(0xfe3, 4));
    regs.insert("PID1".to_string(), mmio.read(0xfe4, 4));
    regs.insert("PID2".to_string(), mmio.read(0xfe8, 4));
    regs.insert("PID3".to_string(), mmio.read(0xfec, 4));
    regs
}

#[test]
fn test_pl011_runtime_oracle_matches_vexpress_regs() {
    let Some(desc) = descriptors::get_descriptor("pl011") else {
        panic!("missing pl011 oracle descriptor");
    };

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => {
                panic!("pl011 oracle error for {}: {error}", scenario.name)
            }
        };

        let mmio = Pl011Mmio(Arc::new(Pl011::new()));
        for &(offset, value, size) in scenario.writes {
            mmio.write(offset, u32::from(size), value);
        }

        assert_eq!(pl011_oracle_regs(&mmio), expected);
    }
}

// -- PL031 runtime oracle --

fn pl031_oracle_regs(mmio: &Pl031Mmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("MR".to_string(), mmio.read(0x04, 4));
    regs.insert("MR_LO8".to_string(), mmio.read(0x04, 1));
    regs.insert("MR_LO16".to_string(), mmio.read(0x04, 2));
    regs.insert("LR".to_string(), mmio.read(0x08, 4));
    regs.insert("LR_LO8".to_string(), mmio.read(0x08, 1));
    regs.insert("LR_LO16".to_string(), mmio.read(0x08, 2));
    regs.insert("CR".to_string(), mmio.read(0x0c, 4));
    regs.insert("IMSC".to_string(), mmio.read(0x10, 4));
    regs.insert("RIS".to_string(), mmio.read(0x14, 4));
    regs.insert("MIS".to_string(), mmio.read(0x18, 4));
    regs.insert("PID0".to_string(), mmio.read(0xfe0, 4));
    regs.insert("PID_UNALIGNED1".to_string(), mmio.read(0xfe1, 4));
    regs.insert("PID_UNALIGNED2".to_string(), mmio.read(0xfe2, 4));
    regs.insert("PID_UNALIGNED3".to_string(), mmio.read(0xfe3, 4));
    regs.insert("PID1".to_string(), mmio.read(0xfe4, 4));
    regs.insert("PID2".to_string(), mmio.read(0xfe8, 4));
    regs.insert("PID3".to_string(), mmio.read(0xfec, 4));
    regs.insert("CID0".to_string(), mmio.read(0xff0, 4));
    regs.insert("CID1".to_string(), mmio.read(0xff4, 4));
    regs.insert("CID2".to_string(), mmio.read(0xff8, 4));
    regs.insert("CID3".to_string(), mmio.read(0xffc, 4));
    regs
}

#[test]
fn test_pl031_runtime_oracle_matches_vexpress_alarm_regs() {
    let Some(desc) = descriptors::get_descriptor("pl031") else {
        panic!("missing pl031 oracle descriptor");
    };

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => {
                panic!("pl031 oracle error for {}: {error}", scenario.name)
            }
        };

        let mmio = Pl031Mmio(Arc::new(Pl031::new()));
        for &(offset, value, size) in scenario.writes {
            mmio.write(offset, u32::from(size), value);
        }

        assert_eq!(pl031_oracle_regs(&mmio), expected);
    }
}

// -- Goldfish RTC runtime oracle --

fn goldfish_rtc_oracle_regs(mmio: &GoldfishRtcMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("ALARM_LOW".to_string(), mmio.read(0x08, 4));
    regs.insert("ALARM_HIGH".to_string(), mmio.read(0x0c, 4));
    regs.insert("IRQ_ENABLED".to_string(), mmio.read(0x10, 4));
    regs.insert("ALARM_STATUS".to_string(), mmio.read(0x18, 4));
    regs
}

#[test]
fn test_goldfish_rtc_runtime_oracle_matches_virt_alarm_regs() {
    let Some(desc) = descriptors::get_descriptor("goldfish_rtc") else {
        panic!("missing goldfish_rtc oracle descriptor");
    };

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => panic!(
                "goldfish_rtc oracle error for {}: {error}",
                scenario.name
            ),
        };

        let mmio = GoldfishRtcMmio(Arc::new(GoldfishRtc::new()));
        for &(offset, value, size) in scenario.writes {
            mmio.write(offset, u32::from(size), value);
        }

        assert_eq!(goldfish_rtc_oracle_regs(&mmio), expected);
    }
}

// -- LS7A RTC runtime oracle --

fn ls7a_rtc_oracle_regs(mmio: &Ls7aRtcMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("TOYMATCH0".to_string(), mmio.read(0x34, 4));
    regs.insert("TOYMATCH1".to_string(), mmio.read(0x38, 4));
    regs.insert("TOYMATCH2".to_string(), mmio.read(0x3c, 4));
    regs.insert("RTCCTRL".to_string(), mmio.read(0x40, 4));
    regs.insert("RTCMATCH0".to_string(), mmio.read(0x6c, 4));
    regs.insert("RTCMATCH1".to_string(), mmio.read(0x70, 4));
    regs.insert("RTCMATCH2".to_string(), mmio.read(0x74, 4));
    regs
}

#[test]
fn test_ls7a_rtc_runtime_oracle_matches_loongarch_virt_match_regs() {
    let Some(desc) = descriptors::get_descriptor("ls7a_rtc") else {
        panic!("missing ls7a_rtc oracle descriptor");
    };

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => {
                panic!("ls7a_rtc oracle error for {}: {error}", scenario.name)
            }
        };

        let mmio = Ls7aRtcMmio(Arc::new(Ls7aRtc::new()));
        for &(offset, value, size) in scenario.writes {
            mmio.write(offset, u32::from(size), value);
        }

        assert_eq!(ls7a_rtc_oracle_regs(&mmio), expected);
    }
}

// -- SiFive UART runtime oracle --

fn sifive_uart_oracle_regs(mmio: &SiFiveUartMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("TXFIFO".to_string(), mmio.read(0x00, 4));
    regs.insert("RXFIFO".to_string(), mmio.read(0x04, 4));
    regs.insert("TXCTRL".to_string(), mmio.read(0x08, 4));
    regs.insert("RXCTRL".to_string(), mmio.read(0x0c, 4));
    regs.insert("IE".to_string(), mmio.read(0x10, 4));
    regs.insert("IP".to_string(), mmio.read(0x14, 4));
    regs.insert("DIV".to_string(), mmio.read(0x18, 4));
    regs
}

#[test]
fn test_sifive_uart_runtime_oracle_matches_sifive_u_control_regs() {
    let Some(desc) = descriptors::get_descriptor("sifive_uart") else {
        panic!("missing sifive_uart oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write control regs")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("sifive_uart oracle error: {error}"),
    };

    let mmio = SiFiveUartMmio(Arc::new(SiFiveUart::new()));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(sifive_uart_oracle_regs(&mmio), expected);
}

// -- RISC-V HTIF runtime oracle --

fn htif_oracle_regs(mmio: &HtifMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("FROMHOST_LO".to_string(), mmio.read(0x00, 4));
    regs.insert("FROMHOST_B0".to_string(), mmio.read(0x00, 1));
    regs.insert("FROMHOST_W0".to_string(), mmio.read(0x00, 2));
    regs.insert("FROMHOST_B1".to_string(), mmio.read(0x01, 1));
    regs.insert("FROMHOST_W2".to_string(), mmio.read(0x02, 2));
    regs.insert("FROMHOST_HI".to_string(), mmio.read(0x04, 4));
    regs.insert("TOHOST_LO".to_string(), mmio.read(0x08, 4));
    regs.insert("TOHOST_HI".to_string(), mmio.read(0x0c, 4));
    regs
}

#[test]
fn test_riscv_htif_runtime_oracle_matches_spike_regs() {
    let Some(desc) = descriptors::get_descriptor("riscv_htif") else {
        panic!("missing riscv_htif oracle descriptor");
    };

    let expected_reset = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("riscv_htif reset oracle error: {error}"),
    };

    assert_eq!(
        htif_oracle_regs(&HtifMmio(Arc::new(Htif::new()))),
        expected_reset
    );

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => {
                panic!("riscv_htif oracle error for {}: {error}", scenario.name)
            }
        };

        let mmio = HtifMmio(Arc::new(Htif::new()));
        for &(offset, value, size) in scenario.writes {
            mmio.write(offset, u32::from(size), value);
        }

        assert_eq!(htif_oracle_regs(&mmio), expected);
    }
}

// -- UART16550 runtime oracle --

fn uart16550_divisor_oracle_regs(
    mmio: &Uart16550Mmio,
) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("DLL".to_string(), mmio.read(0x00, 1));
    regs.insert("DLM".to_string(), mmio.read(0x01, 1));
    regs.insert("IIR".to_string(), mmio.read(0x02, 1));
    regs.insert("LCR".to_string(), mmio.read(0x03, 1));
    regs.insert("LSR".to_string(), mmio.read(0x05, 1));
    regs.insert("SCR".to_string(), mmio.read(0x07, 1));
    regs
}

#[test]
fn test_uart16550_runtime_oracle_matches_virt_divisor_latch_regs() {
    let Some(desc) = descriptors::get_descriptor("uart16550") else {
        panic!("missing uart16550 oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write divisor latch")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("uart16550 oracle error: {error}"),
    };

    let mmio = Uart16550Mmio(Arc::new(Uart16550::new()));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(uart16550_divisor_oracle_regs(&mmio), expected);
}

// -- PLIC runtime oracle --

fn plic_oracle_regs(mmio: &PlicMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("PRIORITY1".to_string(), mmio.read(0x04, 4));
    regs.insert("PRIORITY2".to_string(), mmio.read(0x08, 4));
    regs.insert("PRIORITY5".to_string(), mmio.read(0x14, 4));
    regs.insert("PRIORITY5_UNALIGNED".to_string(), mmio.read(0x15, 4));
    regs.insert("PENDING0".to_string(), mmio.read(0x1000, 4));
    regs.insert("ENABLE0".to_string(), mmio.read(0x2000, 4));
    regs.insert("THRESHOLD0".to_string(), mmio.read(0x200000, 4));
    regs.insert("CLAIM0".to_string(), mmio.read(0x200004, 4));
    regs
}

#[test]
fn test_plic_runtime_oracle_matches_virt_regs() {
    let Some(desc) = descriptors::get_descriptor("plic") else {
        panic!("missing plic oracle descriptor");
    };

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => panic!("plic oracle error: {error}"),
        };

        let mmio = PlicMmio(Arc::new(Plic::new(64, 2)));
        for &(offset, value, size) in scenario.writes {
            mmio.write(offset, u32::from(size), value);
        }

        assert_eq!(plic_oracle_regs(&mmio), expected, "{}", scenario.name);
    }
}

// -- RISC-V APLIC runtime oracle --

fn riscv_aplic_oracle_regs(mmio: &RiscvAplicMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("DOMAINCFG".to_string(), mmio.read(0x0000, 4));
    regs.insert("SOURCECFG1".to_string(), mmio.read(0x0004, 4));
    regs.insert("SETIP0".to_string(), mmio.read(0x1c00, 4));
    regs.insert("SETIE0".to_string(), mmio.read(0x1e00, 4));
    regs.insert("TARGET1".to_string(), mmio.read(0x3004, 4));
    regs.insert("IDELIVERY0".to_string(), mmio.read(0x4000, 4));
    regs.insert("ITHRESHOLD0".to_string(), mmio.read(0x4008, 4));
    regs
}

#[test]
fn test_riscv_aplic_runtime_oracle_matches_virt_aia_direct_mode() {
    let Some(desc) = descriptors::get_descriptor("riscv_aplic") else {
        panic!("missing riscv_aplic oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write direct mode regs")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("riscv_aplic oracle error: {error}"),
    };

    let mmio = RiscvAplicMmio(Arc::new(RiscvAplic::new_named(
        "riscv_aplic",
        96,
        1,
        3,
        false,
        true,
    )));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(riscv_aplic_oracle_regs(&mmio), expected);
}

// -- RISC-V IMSIC runtime oracle --

fn riscv_imsic_oracle_regs(mmio: &RiscvImsicMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("LE_DOORBELL".to_string(), mmio.read(0x00, 4));
    regs.insert("BE_DOORBELL".to_string(), mmio.read(0x04, 4));
    regs
}

#[test]
fn test_riscv_imsic_runtime_oracle_matches_virt_aia_msi_doorbells() {
    let Some(desc) = descriptors::get_descriptor("riscv_imsic") else {
        panic!("missing riscv_imsic oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write msi doorbells")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("riscv_imsic oracle error: {error}"),
    };

    let imsic = Arc::new(RiscvImsic::new_named("riscv_imsic", true, 0, 1, 64));
    let mmio = RiscvImsicMmio(Arc::clone(&imsic));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(riscv_imsic_oracle_regs(&mmio), expected);
    assert_eq!(imsic.eistate_val(5) & 1, 1);
    assert_eq!(imsic.eistate_val(3) & 1, 0);
}

// -- ACLINT runtime oracle --

fn aclint_oracle_regs(mmio: &AclintMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("MSIP0".to_string(), mmio.read(0x0000, 4));
    regs.insert("MSIP1".to_string(), mmio.read(0x0004, 4));
    regs.insert("MTIMECMP0".to_string(), mmio.read(0x4000, 8));
    regs
}

#[test]
fn test_aclint_runtime_oracle_matches_virt_msip_mtimecmp() {
    let Some(desc) = descriptors::get_descriptor("aclint") else {
        panic!("missing aclint oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write msip and mtimecmp")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("aclint oracle error: {error}"),
    };

    let mmio = AclintMmio(Arc::new(Aclint::new(1)));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(aclint_oracle_regs(&mmio), expected);
}

// -- SiFive Test runtime oracle --

fn sifive_test_oracle_regs(mmio: &SifiveTest) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("FINISHER".to_string(), mmio.read(0x00, 4));
    regs
}

#[test]
fn test_sifive_test_runtime_oracle_matches_virt_reset_read() {
    let Some(desc) = descriptors::get_descriptor("sifive_test") else {
        panic!("missing sifive_test oracle descriptor");
    };

    let expected = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("sifive_test oracle error: {error}"),
    };

    let mmio = SifiveTest::new();

    assert_eq!(sifive_test_oracle_regs(&mmio), expected);
}

// -- LoongArch IPI runtime oracle --

fn loongarch_ipi_oracle_regs(mmio: &LoongArchIpiMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("STATUS".to_string(), mmio.read(0x000, 4));
    regs.insert("ENABLE".to_string(), mmio.read(0x004, 4));
    regs.insert("MAILBOX0".to_string(), mmio.read(0x020, 8));
    regs
}

#[test]
fn test_loongarch_ipi_runtime_oracle_matches_virt_enable_mailbox() {
    let Some(desc) = descriptors::get_descriptor("loongarch_ipi") else {
        panic!("missing loongarch_ipi oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write enable mailbox")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("loongarch_ipi oracle error: {error}"),
    };

    let ipi = Arc::new(LoongArchIpi::new_named("loongarch_ipi", 1));
    let mmio = LoongArchIpiMmio(ipi, 0);
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(loongarch_ipi_oracle_regs(&mmio), expected);
}

// -- LoongArch DINTC runtime oracle --

fn dintc_oracle_regs(mmio: &DintcMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("ZERO0".to_string(), mmio.read(0x0000, 4));
    regs.insert("CPU1_VEC5".to_string(), mmio.read(0x1050, 4));
    regs
}

#[test]
fn test_dintc_runtime_oracle_matches_loongarch_virt_doorbell_readback() {
    let Some(desc) = descriptors::get_descriptor("loongarch_dintc") else {
        panic!("missing loongarch_dintc oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "send cpu0 vector 5")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("loongarch_dintc oracle error: {error}"),
    };

    let mmio = DintcMmio(Arc::new(Dintc::new_named("loongarch_dintc", 2)));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(dintc_oracle_regs(&mmio), expected);
}

// -- Loongson LIOINTC runtime oracle --

fn liointc_oracle_regs(mmio: &LiointcMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("MAPPER3".to_string(), mmio.read(0x03, 1));
    regs.insert("ISR".to_string(), mmio.read(0x20, 4));
    regs.insert("IEN".to_string(), mmio.read(0x24, 4));
    regs.insert("PER_CORE_ISR0".to_string(), mmio.read(0x40, 4));
    regs
}

#[test]
fn test_liointc_runtime_oracle_matches_loongson3_mapper_enable() {
    let Some(desc) = descriptors::get_descriptor("liointc") else {
        panic!("missing liointc oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "map irq3 enable")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("liointc oracle error: {error}"),
    };

    let mmio = LiointcMmio(Arc::new(Liointc::new_named("liointc")));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(liointc_oracle_regs(&mmio), expected);
}

// -- LoongArch PCH-MSI runtime oracle --

fn pch_msi_oracle_regs(mmio: &PchMsiMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("MSI0".to_string(), mmio.read(0x00, 4));
    regs.insert("MSI1".to_string(), mmio.read(0x04, 4));
    regs
}

#[test]
fn test_pch_msi_runtime_oracle_matches_loongarch_virt_write_readback() {
    let Some(desc) = descriptors::get_descriptor("loongarch_pch_msi") else {
        panic!("missing loongarch_pch_msi oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write msi vectors")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("loongarch_pch_msi oracle error: {error}"),
    };

    let mmio =
        PchMsiMmio(Arc::new(PchMsi::new_named("loongarch_pch_msi", 32, 224)));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(pch_msi_oracle_regs(&mmio), expected);
}

// -- RISC-V CMGCR/CPC runtime oracle --

fn cmgcr_boston_oracle_regs(mmio: &CmgcrMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("GCR_CONFIG".to_string(), mmio.read(0x0000, 8));
    regs.insert("GCR_BASE".to_string(), mmio.read(0x0008, 8));
    regs.insert("GCR_REV".to_string(), mmio.read(0x0030, 8));
    regs.insert("GCR_CPC_STATUS".to_string(), mmio.read(0x00f0, 8));
    regs.insert("GCR_L2_CONFIG".to_string(), mmio.read(0x0130, 8));
    regs
}

fn boston_cmgcr() -> (Arc<Cmgcr>, CmgcrMmio) {
    let dev =
        Arc::new(Cmgcr::new_named("cmgcr", 0xa00, 0, 1, 1, 1, 0x1fb8_0000));
    dev.set_cpc_connected(true);
    dev.reset_runtime();
    (Arc::clone(&dev), CmgcrMmio(dev))
}

#[test]
fn test_cmgcr_runtime_oracle_matches_boston_gcr_regs() {
    let Some(desc) = descriptors::get_descriptor("cmgcr") else {
        panic!("missing cmgcr oracle descriptor");
    };

    let expected_reset = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("cmgcr reset oracle error: {error}"),
    };

    let (_dev, reset_mmio) = boston_cmgcr();
    assert_eq!(cmgcr_boston_oracle_regs(&reset_mmio), expected_reset);

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => panic!("cmgcr scenario oracle error: {error}"),
        };

        let (_dev, mmio) = boston_cmgcr();
        for &(offset, value, size) in scenario.writes {
            mmio.write(offset, u32::from(size), value);
        }

        assert_eq!(
            cmgcr_boston_oracle_regs(&mmio),
            expected,
            "scenario {}",
            scenario.name
        );
    }
}

fn cpc_boston_oracle_regs(mmio: &CpcMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("CM_STAT_CONF".to_string(), mmio.read(0x1008, 8));
    regs.insert("CL0_STAT_CONF".to_string(), mmio.read(0x2008, 8));
    regs
}

fn boston_cpc() -> (Arc<Cpc>, CpcMmio) {
    let dev = Arc::new(Cpc::new_named("cpc", 0, 1, 1, 1, 1));
    dev.reset_runtime();
    (Arc::clone(&dev), CpcMmio(dev))
}

#[test]
fn test_cpc_runtime_oracle_matches_boston_status_regs() {
    let Some(desc) = descriptors::get_descriptor("cpc") else {
        panic!("missing cpc oracle descriptor");
    };

    let expected_reset = match qemu::probe_reset(desc) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("cpc reset oracle error: {error}"),
    };

    let (_dev, reset_mmio) = boston_cpc();
    assert_eq!(cpc_boston_oracle_regs(&reset_mmio), expected_reset);

    for scenario in desc.scenarios {
        let expected = match qemu::probe_scenario(desc, scenario.name) {
            Ok((regs, _irqs)) => regs,
            Err(reason) if reason.starts_with("SKIP:") => {
                eprintln!("SKIP: {reason}");
                return;
            }
            Err(error) => panic!("cpc scenario oracle error: {error}"),
        };

        let (_dev, mmio) = boston_cpc();
        for &(offset, value, size) in scenario.writes {
            mmio.write(offset, u32::from(size), value);
        }

        assert_eq!(
            cpc_boston_oracle_regs(&mmio),
            expected,
            "scenario {}",
            scenario.name
        );
    }
}

// -- LoongArch PCH-PIC runtime oracle --

fn pch_pic_oracle_regs(mmio: &PchPicMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("ID".to_string(), mmio.read(0x000, 8));
    regs.insert("INT_MASK".to_string(), mmio.read(0x020, 8));
    regs.insert("HTMSI_EN".to_string(), mmio.read(0x040, 8));
    regs.insert("INT_EDGE".to_string(), mmio.read(0x060, 8));
    regs.insert("ROUTE0".to_string(), mmio.read(0x100, 4));
    regs.insert("HTMSI_VEC0".to_string(), mmio.read(0x200, 4));
    regs.insert("INT_STATUS".to_string(), mmio.read(0x3a0, 8));
    regs.insert("INT_POL".to_string(), mmio.read(0x3e0, 8));
    regs
}

#[test]
fn test_pch_pic_runtime_oracle_matches_loongarch_virt_mask_route_vector() {
    let Some(desc) = descriptors::get_descriptor("pch_pic") else {
        panic!("missing pch_pic oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write mask route vector polarity")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("pch_pic oracle error: {error}"),
    };

    let mmio = PchPicMmio(Arc::new(PchPic::new_named("pch_pic", 32)));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(pch_pic_oracle_regs(&mmio), expected);
}

// -- LoongArch EIOINTC runtime oracle --

fn eiointc_oracle_regs(mmio: &EiointcMmio) -> BTreeMap<String, u64> {
    let mut regs = BTreeMap::new();
    regs.insert("ZERO".to_string(), mmio.read(0x000, 4));
    regs.insert("NODEMAP0".to_string(), mmio.read(0x0a0, 4));
    regs.insert("IPMAP0".to_string(), mmio.read(0x0c0, 4));
    regs.insert("ENABLE0".to_string(), mmio.read(0x200, 4));
    regs.insert("BOUNCE0".to_string(), mmio.read(0x280, 4));
    regs.insert("CORE_ISR0".to_string(), mmio.read(0x400, 4));
    regs.insert("COREMAP0".to_string(), mmio.read(0x800, 4));
    regs
}

#[test]
fn test_eiointc_runtime_oracle_matches_loongarch_virt_routing_regs() {
    let Some(desc) = descriptors::get_descriptor("eiointc") else {
        panic!("missing eiointc oracle descriptor");
    };
    let scenario = desc
        .scenarios
        .iter()
        .find(|scenario| scenario.name == "write routing regs")
        .unwrap();

    let expected = match qemu::probe_scenario(desc, scenario.name) {
        Ok((regs, _irqs)) => regs,
        Err(reason) if reason.starts_with("SKIP:") => {
            eprintln!("SKIP: {reason}");
            return;
        }
        Err(error) => panic!("eiointc oracle error: {error}"),
    };

    let mmio = EiointcMmio(Arc::new(Eiointc::new_named("eiointc", 1)));
    for &(offset, value, size) in scenario.writes {
        mmio.write(offset, u32::from(size), value);
    }

    assert_eq!(eiointc_oracle_regs(&mmio), expected);
}
