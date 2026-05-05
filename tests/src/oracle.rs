use std::collections::BTreeMap;
use std::io::Write;

use machina_oracle::{
    Oracle, OracleCheckResult, OracleFixture, OracleQuirk, OracleScenario,
    RuntimeOracle,
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
    match oracle.check_reset(&actual) {
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

    match oracle.check_reset(&actual) {
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

    match oracle.check_reset(&actual) {
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
    // Write a script that exits non-zero
    let mut f = std::fs::File::create(&probe_path).unwrap();
    f.write_all(b"#!/bin/sh\nexit 1\n").unwrap();
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
    match oracle.check_reset(&actual) {
        OracleCheckResult::Skip(_) => {}
        other => panic!("expected Skip for failing probe, got {other:?}"),
    }
}
