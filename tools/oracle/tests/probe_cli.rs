//! Integration tests for the `machina-qemu-hw-probe` CLI contract.
//!
//! Uses `CARGO_BIN_EXE` so the test always locates the real binary
//! regardless of target directory — no artifact-walking needed.

use std::process::Command;

fn probe_binary() -> &'static str {
    env!("CARGO_BIN_EXE_machina-qemu-hw-probe")
}

/// Run the probe and return (status, stdout, stderr).
fn run_probe(args: &[&str]) -> (std::process::ExitStatus, String, String) {
    let output = Command::new(probe_binary())
        .args(args)
        .output()
        .expect("failed to execute probe binary");
    (
        output.status,
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

// -- CLI contract (fixture-backed, no QEMU needed) --

#[test]
fn test_probe_reset_json_format() {
    // Use a fixture-backed device so no QEMU is needed.
    let (status, stdout, _) = run_probe(&["unimp", "--probe", "reset"]);
    assert!(status.success(), "probe should exit 0 on reset");
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    assert!(parsed["registers"].is_object());
    assert!(parsed["irqs"].is_object());
}

#[test]
fn test_probe_scenario_json_format() {
    let (status, stdout, _) =
        run_probe(&["unimp", "--probe", "scenario", "write then read"]);
    assert!(status.success(), "probe should exit 0 on scenario");
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    assert!(parsed["registers"].is_object());
    assert!(parsed["irqs"].is_object());
}

#[test]
fn test_probe_unknown_device_exits_nonzero() {
    let (status, _, stderr) =
        run_probe(&["nonexistent_device", "--probe", "reset"]);
    assert!(!status.success());
    assert!(
        stderr.contains("unknown device") || stderr.contains("unknown"),
        "stderr should mention unknown device: {stderr}"
    );
}

#[test]
fn test_probe_unknown_scenario_exits_nonzero() {
    let (status, _, stderr) =
        run_probe(&["unimp", "--probe", "scenario", "nonexistent_scenario"]);
    assert!(!status.success());
    assert!(
        stderr.contains("unknown scenario") || stderr.contains("unknown"),
        "stderr should mention unknown scenario: {stderr}"
    );
}

#[test]
fn test_probe_invalid_args_exits_nonzero() {
    let (status, _, _) = run_probe(&["unimp", "--bad-flag"]);
    assert!(!status.success());
}

#[test]
fn test_probe_missing_args_exits_nonzero() {
    let (status, _, _) = run_probe(&["unimp"]);
    assert!(!status.success());
}

// -- QEMU-backed probe (skip when QEMU is unavailable) --

#[test]
fn test_probe_qemu_reset_sifive_e_prci() {
    let (status, stdout, stderr) =
        run_probe(&["sifive_e_prci", "--probe", "reset"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["HFROSCCFG"].as_u64().unwrap(), 0xC000_0000);
    assert_eq!(regs["HFXOSCCFG"].as_u64().unwrap(), 0xC000_0000);
    assert_eq!(regs["PLLCFG"].as_u64().unwrap(), 0x8006_0000);
    assert_eq!(regs["PLLOUTDIV"].as_u64().unwrap(), 0x100);
}

#[test]
fn test_probe_qemu_reset_sifive_u_prci() {
    let (status, stdout, stderr) =
        run_probe(&["sifive_u_prci", "--probe", "reset"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert!(regs["HFXOSCCFG"].as_u64().is_some());
    assert!(regs["COREPLLCFG0"].as_u64().is_some());
}

#[test]
fn test_probe_qemu_scenario_write_pllcfg() {
    let (status, stdout, stderr) =
        run_probe(&["sifive_e_prci", "--probe", "scenario", "write PLLCFG"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    // Typed readl returns the guest-visible 32-bit value.
    assert_eq!(parsed["registers"]["PLLCFG"].as_u64().unwrap(), 0x9234_5678);
}

#[test]
fn test_probe_skip_when_qemu_missing() {
    // Set QEMU binary to a non-existent path to force exit 77.
    let output = Command::new(probe_binary())
        .args(["sifive_e_prci", "--probe", "reset"])
        .env("MACHINA_QEMU_SYSTEM_RISCV64", "/nonexistent/qemu")
        .output()
        .expect("failed to execute probe");

    assert_eq!(output.status.code(), Some(77));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("SKIP"));
}
