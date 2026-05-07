//! Integration tests for the `machina-qemu-hw-probe` CLI contract.
//!
//! Uses `CARGO_BIN_EXE` so the test always locates the real binary
//! regardless of target directory — no artifact-walking needed.

use std::process::Command;
use std::sync::{Mutex, OnceLock};

fn probe_binary() -> &'static str {
    env!("CARGO_BIN_EXE_machina-qemu-hw-probe")
}

/// Run the probe and return (status, stdout, stderr).
fn run_probe(args: &[&str]) -> (std::process::ExitStatus, String, String) {
    static PROBE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    // QEMU-backed probe processes are not isolated enough to run reliably
    // under the Rust test harness' default test-level concurrency.
    let _guard = PROBE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("probe lock poisoned");

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

#[test]
fn test_oracle_device_manifest_resolves_all_entries() {
    let names = machina_oracle::descriptors::all_oracle_device_names();
    assert_eq!(names.len(), 55);

    let mut seen = std::collections::BTreeSet::new();
    for name in names {
        assert!(seen.insert(name), "duplicate oracle device name: {name}");
        assert!(
            machina_oracle::descriptors::get_descriptor(name).is_some()
                || machina_oracle::descriptors::get_qtest_descriptor(name)
                    .is_some(),
            "missing oracle descriptor for {name}"
        );
    }
    assert!(
        seen.contains(&"pvpanic-mmio"),
        "pvpanic-mmio inventory entry must have runtime oracle coverage"
    );
}

#[test]
fn test_inventory_devices_have_oracle_manifest_entries() {
    let manifest = machina_oracle::descriptors::all_oracle_device_names()
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(EXPECTED_INVENTORY_ORACLE_NAMES.len(), 55);
    for name in EXPECTED_INVENTORY_ORACLE_NAMES {
        assert!(
            machina_oracle::descriptors::get_descriptor(name).is_some()
                || machina_oracle::descriptors::get_qtest_descriptor(name)
                    .is_some(),
            "missing oracle descriptor for inventory device {name}"
        );
        assert!(
            manifest.contains(name),
            "oracle manifest missing inventory device {name}"
        );
    }
}

const EXPECTED_INVENTORY_ORACLE_NAMES: &[&str] = &[
    "sifive_e_prci",
    "sifive_u_prci",
    "gpio_key",
    "gpio_pwr",
    "pvpanic",
    "pvpanic-mmio",
    "unimp",
    "led",
    "virt_ctrl",
    "loongarch_dintc",
    "loongarch_pch_msi",
    "loongarch_ipi",
    "liointc",
    "pch_pic",
    "eiointc",
    "riscv_aplic",
    "riscv_imsic",
    "cmgcr",
    "cpc",
    "plic",
    "aclint",
    "sifive_test",
    "pl011",
    "uart16550",
    "sifive_uart",
    "riscv_htif",
    "pl031",
    "ls7a_rtc",
    "goldfish_rtc",
    "ds1338",
    "sifive_pwm",
    "sse_timer",
    "sse_counter",
    "pl061",
    "sifive_gpio",
    "pl022",
    "sifive_spi",
    "pl050",
    "sifive_e_aon",
    "sifive_u_otp",
    "m25p80",
    "pflash_cfi01",
    "pflash_cfi02",
    "sd_card",
    "sdhci",
    "ssi_sd",
    "pl181",
    "eeprom_at24c",
    "smbus_eeprom",
    "fw_cfg",
    "sifive_pdma",
    "pl080",
    "tmp105",
    "tmp421",
    "sbsa_gwdt",
];

// -- CLI contract --

#[test]
fn test_probe_reset_json_format() {
    let (status, stdout, stderr) = run_probe(&["unimp", "--probe", "reset"]);
    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }
    assert!(status.success(), "probe should exit 0 on reset");
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    assert!(parsed["registers"].is_object());
    assert!(parsed["irqs"].is_object());
}

#[test]
fn test_probe_scenario_json_format() {
    let (status, stdout, stderr) =
        run_probe(&["unimp", "--probe", "scenario", "write ignored"]);
    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }
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
fn test_probe_device_without_runtime_descriptor_exits_nonzero() {
    let (status, _, stderr) = run_probe(&["pvpanic-pci", "--probe", "reset"]);
    assert!(!status.success());
    assert!(
        stderr.contains("unknown device")
            || stderr.contains("runtime descriptor"),
        "stderr should explain missing runtime descriptor: {stderr}"
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
fn test_probe_qemu_unimp_write_ignored() {
    let (status, stdout, stderr) =
        run_probe(&["unimp", "--probe", "scenario", "write ignored"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["READ0"].as_u64().unwrap(), 0);
    assert_eq!(regs["READ4"].as_u64().unwrap(), 0);
    assert_eq!(regs["READB0"].as_u64().unwrap(), 0);
    assert_eq!(regs["READW2"].as_u64().unwrap(), 0);
}

#[test]
fn test_probe_qemu_pvpanic_isa_write_panicked() {
    let (status, stdout, stderr) =
        run_probe(&["pvpanic", "--probe", "scenario", "write PANICKED"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["EVENTS"].as_u64().unwrap(), 0x07);
}

#[test]
fn test_probe_qemu_pvpanic_mmio_write_shutdown() {
    let (status, stdout, stderr) =
        run_probe(&["pvpanic-mmio", "--probe", "scenario", "write SHUTDOWN"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["EVENTS"].as_u64().unwrap(), 0x07);
}

#[test]
fn test_probe_qemu_gpio_key_press_and_release_irq() {
    let (status, stdout, stderr) =
        run_probe(&["gpio_key", "--probe", "scenario", "press and release"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(
        status.success(),
        "probe failed, stdout={stdout:?}, stderr={stderr:?}"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    let irqs = &parsed["irqs"];
    assert_eq!(regs["IRQ3_RAISES"].as_u64().unwrap(), 1);
    assert_eq!(regs["IRQ3_LOWERS"].as_u64().unwrap(), 1);
    assert_eq!(irqs["3"].as_bool().unwrap(), false);
}

#[test]
fn test_probe_qemu_led_set_gpio_high_then_low() {
    let (status, stdout, stderr) =
        run_probe(&["led", "--probe", "scenario", "set gpio high then low"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(
        status.success(),
        "probe failed, stdout={stdout:?}, stderr={stderr:?}"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["INTENSITY"].as_u64().unwrap(), 0);
}

#[test]
fn test_probe_qemu_gpio_pwr_shutdown_trigger() {
    let (status, stdout, stderr) =
        run_probe(&["gpio_pwr", "--probe", "scenario", "shutdown trigger"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(
        status.success(),
        "probe failed, stdout={stdout:?}, stderr={stderr:?}"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["ACTION"].as_u64().unwrap(), 1);
}

#[test]
fn test_probe_qemu_virt_ctrl_write_cmd_noop() {
    let (status, stdout, stderr) =
        run_probe(&["virt_ctrl", "--probe", "scenario", "write CMD_NOOP"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["FEATURES"].as_u64().unwrap(), 0x0000_0001);
    assert_eq!(regs["CMD"].as_u64().unwrap(), 0);
}

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
fn test_probe_qemu_riscv_aplic_write_direct_mode_regs() {
    let (status, stdout, stderr) = run_probe(&[
        "riscv_aplic",
        "--probe",
        "scenario",
        "write direct mode regs",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["DOMAINCFG"].as_u64().unwrap(), 0x8000_0100);
    assert_eq!(regs["SOURCECFG1"].as_u64().unwrap(), 0x4);
    assert_eq!(regs["SETIP0"].as_u64().unwrap(), 0x2);
    assert_eq!(regs["SETIE0"].as_u64().unwrap(), 0x2);
    assert_eq!(regs["TARGET1"].as_u64().unwrap(), 0x0001_2305);
    assert_eq!(regs["IDELIVERY0"].as_u64().unwrap(), 0x1);
    assert_eq!(regs["ITHRESHOLD0"].as_u64().unwrap(), 0x5);
}

#[test]
fn test_probe_qemu_riscv_imsic_write_msi_doorbells() {
    let (status, stdout, stderr) = run_probe(&[
        "riscv_imsic",
        "--probe",
        "scenario",
        "write msi doorbells",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["LE_DOORBELL"].as_u64().unwrap(), 0);
    assert_eq!(regs["BE_DOORBELL"].as_u64().unwrap(), 0);
}

#[test]
fn test_probe_qemu_pflash_cfi01_cfi_query() {
    let (status, stdout, stderr) =
        run_probe(&["pflash_cfi01", "--probe", "scenario", "cfi query"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["READ_0040"].as_u64().unwrap(), u64::from(b'Q'));
    assert_eq!(regs["READ_0044"].as_u64().unwrap(), u64::from(b'R'));
    assert_eq!(regs["READ_0048"].as_u64().unwrap(), u64::from(b'Y'));
}

#[test]
fn test_probe_qemu_pflash_cfi01_id_query() {
    let (status, stdout, stderr) =
        run_probe(&["pflash_cfi01", "--probe", "scenario", "id query"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["READ_0000"].as_u64().unwrap(), 0x89);
    assert_eq!(regs["READ_0004"].as_u64().unwrap(), 0x18);
}

#[test]
fn test_probe_qemu_pflash_cfi01_erase_then_program() {
    let (status, stdout, stderr) = run_probe(&[
        "pflash_cfi01",
        "--probe",
        "scenario",
        "erase then program",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["READ_0020"].as_u64().unwrap(), 0x0f);
    assert_eq!(regs["READ_0040"].as_u64().unwrap(), 0xff);
}

#[test]
fn test_probe_qemu_pflash_cfi02_cfi_query() {
    let (status, stdout, stderr) =
        run_probe(&["pflash_cfi02", "--probe", "scenario", "cfi query"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["READ_0010"].as_u64().unwrap(), u64::from(b'Q'));
    assert_eq!(regs["READ_0011"].as_u64().unwrap(), u64::from(b'R'));
    assert_eq!(regs["READ_0012"].as_u64().unwrap(), u64::from(b'Y'));
}

#[test]
fn test_probe_qemu_pflash_cfi02_id_query() {
    let (status, stdout, stderr) =
        run_probe(&["pflash_cfi02", "--probe", "scenario", "id query"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["READ_0000"].as_u64().unwrap(), 0x66);
    assert_eq!(regs["READ_0001"].as_u64().unwrap(), 0x22);
    assert_eq!(regs["READ_000e"].as_u64().unwrap(), 0);
    assert_eq!(regs["READ_000f"].as_u64().unwrap(), 0);
}

#[test]
fn test_probe_qemu_pflash_cfi02_erase_then_program() {
    let (status, stdout, stderr) = run_probe(&[
        "pflash_cfi02",
        "--probe",
        "scenario",
        "erase then program",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(
        status.success(),
        "probe failed, stdout={stdout:?}, stderr={stderr:?}"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["READ_0020"].as_u64().unwrap(), 0x0f);
    assert_eq!(regs["READ_0010"].as_u64().unwrap(), 0xff);
}

#[test]
fn test_probe_qemu_tmp105_write_and_read_t_high() {
    let (status, stdout, stderr) =
        run_probe(&["tmp105", "--probe", "scenario", "write and read t_high"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["T_HIGH_MSB"].as_u64().unwrap(), 0xde);
    assert_eq!(regs["T_HIGH_LSB"].as_u64().unwrap(), 0xa0);
}

#[test]
fn test_probe_qemu_tmp421_write_and_read_config1() {
    let (status, stdout, stderr) =
        run_probe(&["tmp421", "--probe", "scenario", "write and read config1"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CONFIG1"].as_u64().unwrap(), 0x44);
}

#[test]
fn test_probe_qemu_ds1338_write_and_read_nvram() {
    let (status, stdout, stderr) =
        run_probe(&["ds1338", "--probe", "scenario", "write and read nvram"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["NVRAM10"].as_u64().unwrap(), 0xab);
}

#[test]
fn test_probe_qemu_eeprom_at24c_write_and_read_byte() {
    let (status, stdout, stderr) = run_probe(&[
        "eeprom_at24c",
        "--probe",
        "scenario",
        "write and read byte",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["DATA20"].as_u64().unwrap(), 0xaa);
}

#[test]
fn test_probe_qemu_smbus_eeprom_write_and_read_byte_data() {
    let (status, stdout, stderr) = run_probe(&[
        "smbus_eeprom",
        "--probe",
        "scenario",
        "write and read byte data",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["DATA20"].as_u64().unwrap(), 0xaa);
}

#[test]
fn test_probe_qemu_sd_card_cmd8_interface_condition() {
    let (status, stdout, stderr) = run_probe(&[
        "sd_card",
        "--probe",
        "scenario",
        "cmd8 interface condition",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["RESPONSE0"].as_u64().unwrap(), 0x1aa);
    assert_eq!(regs["NORMAL_INT_STATUS"].as_u64().unwrap(), 0x1);
}

#[test]
fn test_probe_qemu_ssi_sd_cmd8_response_prefix() {
    let (status, stdout, stderr) =
        run_probe(&["ssi_sd", "--probe", "scenario", "cmd8 response prefix"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    for index in 0..7 {
        let name = format!("RX_{index:02}");
        assert_eq!(regs[&name].as_u64().unwrap(), 0xff);
    }
    assert_eq!(regs["RX_07"].as_u64().unwrap(), 0x01);
}

#[test]
fn test_probe_qemu_sbsa_gwdt_write_control_regs() {
    let (status, stdout, stderr) =
        run_probe(&["sbsa_gwdt", "--probe", "scenario", "write control regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["WCS"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["WOR"].as_u64().unwrap(), 0x1234_5678);
    assert_eq!(regs["WORU"].as_u64().unwrap(), 0x5555);
    assert_eq!(regs["WCV"].as_u64().unwrap(), 0xfeed_cafe);
    assert_eq!(regs["WCVU"].as_u64().unwrap(), 0x8765_4321);
    assert_eq!(regs["W_IIDR"].as_u64().unwrap(), 0x1043b);
}

#[test]
fn test_probe_qemu_pl080_write_channel_regs() {
    let (status, stdout, stderr) =
        run_probe(&["pl080", "--probe", "scenario", "write channel regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CH2_SRC"].as_u64().unwrap(), 0x1000_0000);
    assert_eq!(regs["CH2_DEST"].as_u64().unwrap(), 0x2000_0000);
    assert_eq!(regs["CH2_LLI"].as_u64().unwrap(), 0x3000_0000);
    assert_eq!(regs["CH2_CTRL"].as_u64().unwrap(), 0x8000_0010);
    assert_eq!(regs["CH2_CONF"].as_u64().unwrap(), 0x0000_0001);
    assert_eq!(regs["ENABLED"].as_u64().unwrap(), 1 << 2);
    assert_eq!(regs["ID0"].as_u64().unwrap(), 0x80);
    assert_eq!(regs["ID_UNALIGNED1"].as_u64().unwrap(), 0x1000_8080);
    assert_eq!(regs["ID_UNALIGNED2"].as_u64().unwrap(), 0x0010_0080);
    assert_eq!(regs["ID_UNALIGNED3"].as_u64().unwrap(), 0x1000_1080);
    assert_eq!(regs["ID7"].as_u64().unwrap(), 0xb1);
}

#[test]
fn test_probe_qemu_pl080_unaligned_wide_access() {
    let (status, stdout, stderr) =
        run_probe(&["pl080", "--probe", "scenario", "unaligned wide access"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CONFIG"].as_u64().unwrap(), 0x0203);
    assert_eq!(regs["SYNC"].as_u64().unwrap(), 0x0001);
    assert_eq!(regs["ID_UNALIGNED1"].as_u64().unwrap(), 0x1000_8080);
    assert_eq!(regs["ID_UNALIGNED2"].as_u64().unwrap(), 0x0010_0080);
    assert_eq!(regs["ID_UNALIGNED3"].as_u64().unwrap(), 0x1000_1080);
}

#[test]
fn test_probe_qemu_pl050_write_control_regs() {
    let (status, stdout, stderr) =
        run_probe(&["pl050", "--probe", "scenario", "write control regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CR"].as_u64().unwrap(), 0x18);
    assert_eq!(regs["CR_LO8"].as_u64().unwrap(), 0x18);
    assert_eq!(regs["CR_LO16"].as_u64().unwrap(), 0x18);
    assert_eq!(regs["STAT"].as_u64().unwrap(), 0x40);
    assert_eq!(regs["DATA"].as_u64().unwrap(), 0);
    assert_eq!(regs["CLKDIV"].as_u64().unwrap(), 0x1234);
    assert_eq!(regs["CLKDIV_LO16"].as_u64().unwrap(), 0x1234);
    assert_eq!(regs["IIR"].as_u64().unwrap(), 0x02);
    assert_eq!(regs["ID0"].as_u64().unwrap(), 0x50);
    assert_eq!(regs["ID_UNALIGNED1"].as_u64().unwrap(), 0x1000_5050);
    assert_eq!(regs["ID_UNALIGNED2"].as_u64().unwrap(), 0x0010_0050);
    assert_eq!(regs["ID_UNALIGNED3"].as_u64().unwrap(), 0x1000_1050);
    assert_eq!(regs["ID7"].as_u64().unwrap(), 0xb1);
}

#[test]
fn test_probe_qemu_pl050_write_data_resend() {
    let (status, stdout, stderr) =
        run_probe(&["pl050", "--probe", "scenario", "write data resend"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CR"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["CR_LO8"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["CR_LO16"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["STAT"].as_u64().unwrap(), 0x50);
    assert_eq!(regs["DATA"].as_u64().unwrap(), 0xfe);
    assert_eq!(regs["CLKDIV"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["CLKDIV_LO16"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["IIR"].as_u64().unwrap(), 0x02);
    assert_eq!(regs["ID0"].as_u64().unwrap(), 0x50);
    assert_eq!(regs["ID7"].as_u64().unwrap(), 0xb1);
}

#[test]
fn test_probe_qemu_pl050_narrow_access_regs() {
    let (status, stdout, stderr) =
        run_probe(&["pl050", "--probe", "scenario", "narrow access regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CR"].as_u64().unwrap(), 0x1234_5678);
    assert_eq!(regs["CR_LO8"].as_u64().unwrap(), 0x78);
    assert_eq!(regs["CR_LO16"].as_u64().unwrap(), 0x5678);
    assert_eq!(regs["CLKDIV"].as_u64().unwrap(), 0x5678);
    assert_eq!(regs["CLKDIV_LO16"].as_u64().unwrap(), 0x5678);
}

#[test]
fn test_probe_qemu_pl050_unaligned_wide_access() {
    let (status, stdout, stderr) =
        run_probe(&["pl050", "--probe", "scenario", "unaligned wide access"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CR"].as_u64().unwrap(), 0x0203);
    assert_eq!(regs["STAT"].as_u64().unwrap(), 0x40);
    assert_eq!(regs["CLKDIV"].as_u64().unwrap(), 0x0203);
    assert_eq!(regs["IIR"].as_u64().unwrap(), 0x02);
    assert_eq!(regs["ID_UNALIGNED1"].as_u64().unwrap(), 0x1000_5050);
    assert_eq!(regs["ID_UNALIGNED2"].as_u64().unwrap(), 0x0010_0050);
    assert_eq!(regs["ID_UNALIGNED3"].as_u64().unwrap(), 0x1000_1050);
}

#[test]
fn test_probe_qemu_sifive_e_aon_write_compare() {
    let (status, stdout, stderr) =
        run_probe(&["sifive_e_aon", "--probe", "scenario", "write compare"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["WDOGCFG"].as_u64().unwrap(), 0);
    assert_eq!(regs["WDOGCOUNT"].as_u64().unwrap(), 0);
    assert_eq!(regs["WDOGS"].as_u64().unwrap(), 0);
    assert_eq!(regs["WDOGFEED"].as_u64().unwrap(), 0);
    assert_eq!(regs["WDOGKEY"].as_u64().unwrap(), 0);
    assert_eq!(regs["WDOGCMP0"].as_u64().unwrap(), 0x1234);
    assert_eq!(regs["RTC"].as_u64().unwrap(), 0);
    assert_eq!(regs["LFROSC"].as_u64().unwrap(), 0);
}

#[test]
fn test_probe_qemu_sifive_e_aon_scale_wdogs() {
    let (status, stdout, stderr) =
        run_probe(&["sifive_e_aon", "--probe", "scenario", "scale wdogs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["WDOGCFG"].as_u64().unwrap(), 0x03);
    assert_eq!(regs["WDOGCOUNT"].as_u64().unwrap(), 0x80);
    assert_eq!(regs["WDOGS"].as_u64().unwrap(), 0x10);
}

#[test]
fn test_probe_qemu_loongarch_dintc_send_cpu0_vector_5() {
    let (status, stdout, stderr) = run_probe(&[
        "loongarch_dintc",
        "--probe",
        "scenario",
        "send cpu0 vector 5",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["ZERO0"].as_u64().unwrap(), 0);
    assert_eq!(regs["CPU1_VEC5"].as_u64().unwrap(), 0);
}

#[test]
fn test_probe_qemu_loongarch_pch_msi_write_vectors() {
    let (status, stdout, stderr) = run_probe(&[
        "loongarch_pch_msi",
        "--probe",
        "scenario",
        "write msi vectors",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["MSI0"].as_u64().unwrap(), 0);
    assert_eq!(regs["MSI1"].as_u64().unwrap(), 0);
}

#[test]
fn test_probe_qemu_sifive_pdma_claim_channel_0() {
    let (status, stdout, stderr) =
        run_probe(&["sifive_pdma", "--probe", "scenario", "claim channel 0"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CONTROL"].as_u64().unwrap(), 0x0000_0001);
    assert_eq!(regs["NEXT_CONFIG"].as_u64().unwrap(), 0x6600_0000);
    assert_eq!(regs["NEXT_BYTES"].as_u64().unwrap(), 0);
    assert_eq!(regs["NEXT_DST"].as_u64().unwrap(), 0);
    assert_eq!(regs["NEXT_SRC"].as_u64().unwrap(), 0);
    assert_eq!(regs["EXEC_CONFIG"].as_u64().unwrap(), 0);
    assert_eq!(regs["EXEC_BYTES"].as_u64().unwrap(), 0);
    assert_eq!(regs["EXEC_DST"].as_u64().unwrap(), 0);
    assert_eq!(regs["EXEC_SRC"].as_u64().unwrap(), 0);
}

#[test]
fn test_probe_qemu_sifive_pdma_unaligned_qword_access() {
    let (status, stdout, stderr) = run_probe(&[
        "sifive_pdma",
        "--probe",
        "scenario",
        "unaligned qword access",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["NEXT_BYTES"].as_u64().unwrap(), 0x0203_0405_0000_0000);
    assert_eq!(
        regs["NEXT_BYTES_UNALIGNED"].as_u64().unwrap(),
        0x0002_0304_0500_0000
    );
    assert_eq!(regs["NEXT_DST"].as_u64().unwrap(), 0);
}

#[test]
fn test_probe_qemu_sdhci_write_interrupt_enables() {
    let (status, stdout, stderr) =
        run_probe(&["sdhci", "--probe", "scenario", "write interrupt enables"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["BLOCK_SIZE"].as_u64().unwrap(), 0);
    assert_eq!(regs["BLOCK_COUNT"].as_u64().unwrap(), 0);
    assert_eq!(regs["ARGUMENT"].as_u64().unwrap(), 0);
    assert_eq!(regs["COMMAND"].as_u64().unwrap(), 0);
    assert_eq!(regs["SOFTWARE_RESET"].as_u64().unwrap(), 0);
    assert_eq!(regs["NORMAL_INT_STATUS"].as_u64().unwrap(), 0);
    assert_eq!(regs["ERROR_INT_STATUS"].as_u64().unwrap(), 0);
    assert_eq!(regs["NORMAL_INT_ENABLE"].as_u64().unwrap(), 0xffff);
    assert_eq!(regs["NORMAL_INT_SIGNAL_ENABLE"].as_u64().unwrap(), 0x00ff);
}

#[test]
fn test_probe_qemu_m25p80_jedec_id() {
    let (status, stdout, stderr) =
        run_probe(&["m25p80", "--probe", "scenario", "jedec id"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["RX_00"].as_u64().unwrap(), 0);
    assert_eq!(regs["RX_01"].as_u64().unwrap(), 0x9d);
    assert_eq!(regs["RX_02"].as_u64().unwrap(), 0x70);
    assert_eq!(regs["RX_03"].as_u64().unwrap(), 0x19);
}

#[test]
fn test_probe_qemu_pl022_loopback_fifo() {
    let (status, stdout, stderr) =
        run_probe(&["pl022", "--probe", "scenario", "loopback fifo"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CR0"].as_u64().unwrap(), 0x07);
    assert_eq!(regs["CR0_LO8"].as_u64().unwrap(), 0x07);
    assert_eq!(regs["CR0_LO16"].as_u64().unwrap(), 0x07);
    assert_eq!(regs["CR1"].as_u64().unwrap(), 0x03);
    assert_eq!(regs["DR"].as_u64().unwrap(), 0xab);
    assert_eq!(regs["SR"].as_u64().unwrap(), 0x03);
    assert_eq!(regs["CPSR"].as_u64().unwrap(), 0xfe);
    assert_eq!(regs["CPSR_LO8"].as_u64().unwrap(), 0xfe);
    assert_eq!(regs["IMSC"].as_u64().unwrap(), 0x08);
    assert_eq!(regs["RIS"].as_u64().unwrap(), 0x08);
    assert_eq!(regs["MIS"].as_u64().unwrap(), 0x08);
    assert_eq!(regs["PID0"].as_u64().unwrap(), 0x22);
    assert_eq!(regs["PID_UNALIGNED1"].as_u64().unwrap(), 0x1000_2222);
    assert_eq!(regs["PID_UNALIGNED2"].as_u64().unwrap(), 0x0010_0022);
    assert_eq!(regs["PID_UNALIGNED3"].as_u64().unwrap(), 0x1000_1022);
    assert_eq!(regs["PID1"].as_u64().unwrap(), 0x10);
    assert_eq!(regs["PID2"].as_u64().unwrap(), 0x04);
    assert_eq!(regs["PID3"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["CID0"].as_u64().unwrap(), 0x0d);
    assert_eq!(regs["CID1"].as_u64().unwrap(), 0xf0);
    assert_eq!(regs["CID2"].as_u64().unwrap(), 0x05);
    assert_eq!(regs["CID3"].as_u64().unwrap(), 0xb1);
}

#[test]
fn test_probe_qemu_pl022_narrow_access_regs() {
    let (status, stdout, stderr) =
        run_probe(&["pl022", "--probe", "scenario", "narrow access regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CR0"].as_u64().unwrap(), 0x1234_5678);
    assert_eq!(regs["CR0_LO8"].as_u64().unwrap(), 0x78);
    assert_eq!(regs["CR0_LO16"].as_u64().unwrap(), 0x5678);
    assert_eq!(regs["CPSR"].as_u64().unwrap(), 0x78);
    assert_eq!(regs["CPSR_LO8"].as_u64().unwrap(), 0x78);
}

#[test]
fn test_probe_qemu_reset_sifive_spi() {
    let (status, stdout, stderr) =
        run_probe(&["sifive_spi", "--probe", "reset"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["SCKDIV"].as_u64().unwrap(), 0x03);
    assert_eq!(regs["SCKDIV_UNALIGNED"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["CSID"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["CSDEF"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["CSDEF_UNALIGNED"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["CSMODE"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["DELAY0"].as_u64().unwrap(), 0x1001);
    assert_eq!(regs["DELAY1"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["TXDATA"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["RXDATA"].as_u64().unwrap(), 0x8000_0000);
    assert_eq!(regs["IE"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["IP"].as_u64().unwrap(), 0x00);
}

#[test]
fn test_probe_qemu_sifive_spi_unaligned_access_ignored() {
    let (status, stdout, stderr) = run_probe(&[
        "sifive_spi",
        "--probe",
        "scenario",
        "unaligned access ignored",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["SCKDIV"].as_u64().unwrap(), 0x03);
    assert_eq!(regs["SCKDIV_UNALIGNED"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["CSDEF"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["CSDEF_UNALIGNED"].as_u64().unwrap(), 0x00);
}

#[test]
fn test_probe_qemu_sifive_pwm_write_compare_regs() {
    let (status, stdout, stderr) =
        run_probe(&["sifive_pwm", "--probe", "scenario", "write compare regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CONFIG"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["CONFIG_LO8"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["CONFIG_LO16"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["COUNT"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["PWMS"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["PWMCMP0"].as_u64().unwrap(), 0x5678);
    assert_eq!(regs["PWMCMP0_LO8"].as_u64().unwrap(), 0x78);
    assert_eq!(regs["PWMCMP0_LO16"].as_u64().unwrap(), 0x5678);
    assert_eq!(regs["PWMCMP1"].as_u64().unwrap(), 0xffff);
    assert_eq!(regs["PWMCMP2"].as_u64().unwrap(), 0x0000);
    assert_eq!(regs["PWMCMP3"].as_u64().unwrap(), 0xffff);
}

#[test]
fn test_probe_qemu_sifive_pwm_unaligned_wide_access() {
    let (status, stdout, stderr) = run_probe(&[
        "sifive_pwm",
        "--probe",
        "scenario",
        "unaligned wide access",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["PWMCMP0"].as_u64().unwrap(), 0);
    assert_eq!(regs["PWMCMP0_UNALIGNED1"].as_u64().unwrap(), 0x0100_0000);
    assert_eq!(regs["PWMCMP0_UNALIGNED2"].as_u64().unwrap(), 0x0001_0000);
    assert_eq!(regs["PWMCMP0_UNALIGNED3"].as_u64().unwrap(), 0x0000_0100);
    assert_eq!(regs["PWMCMP1"].as_u64().unwrap(), 0x01);
}

#[test]
fn test_probe_qemu_sse_counter_write_counter_regs() {
    let (status, stdout, stderr) = run_probe(&[
        "sse_counter",
        "--probe",
        "scenario",
        "write counter regs",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CNTCR"].as_u64().unwrap(), 0x14);
    assert_eq!(regs["CNTCV_LO"].as_u64().unwrap(), 0x1234_5678);
    assert_eq!(regs["CNTCV_HI"].as_u64().unwrap(), 0x9abc_def0);
    assert_eq!(regs["CNTSCR"].as_u64().unwrap(), 0x0100_0001);
    assert_eq!(regs["CNTID"].as_u64().unwrap(), 0x0002_0001);
    assert_eq!(regs["CNTSCR0"].as_u64().unwrap(), 0x0100_0001);
    assert_eq!(regs["PID4"].as_u64().unwrap(), 0x04);
    assert_eq!(regs["PID0"].as_u64().unwrap(), 0xba);
    assert_eq!(regs["STATUS_CNTCV_LO"].as_u64().unwrap(), 0x1234_5678);
    assert_eq!(regs["STATUS_CNTCV_HI"].as_u64().unwrap(), 0x9abc_def0);
}

#[test]
fn test_probe_qemu_sse_timer_write_timer_regs() {
    let (status, stdout, stderr) =
        run_probe(&["sse_timer", "--probe", "scenario", "write timer regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CNTFRQ"].as_u64().unwrap(), 0x1234_5678);
    assert_eq!(regs["CNTP_CVAL_LO"].as_u64().unwrap(), 0x89ab_cdef);
    assert_eq!(regs["CNTP_CVAL_HI"].as_u64().unwrap(), 0x0123_4567);
    assert_eq!(regs["CNTP_TVAL"].as_u64().unwrap(), 0x89ab_cdef);
    assert_eq!(regs["CNTP_CTL"].as_u64().unwrap(), 0x03);
    assert_eq!(regs["CNTP_AIVAL_RELOAD"].as_u64().unwrap(), 0x55aa);
    assert_eq!(regs["CNTP_AIVAL_CTL"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["CNTP_CFG"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["PID4"].as_u64().unwrap(), 0x04);
    assert_eq!(regs["PID0"].as_u64().unwrap(), 0xb7);
}

#[test]
fn test_probe_qemu_sse_timer_tval_signed_write() {
    let (status, stdout, stderr) =
        run_probe(&["sse_timer", "--probe", "scenario", "tval signed write"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CNTFRQ"].as_u64().unwrap(), 0);
    assert_eq!(regs["CNTP_CVAL_LO"].as_u64().unwrap(), 0xffff_fffe);
    assert_eq!(regs["CNTP_CVAL_HI"].as_u64().unwrap(), 0xffff_ffff);
    assert_eq!(regs["CNTP_TVAL"].as_u64().unwrap(), 0xffff_fffe);
    assert_eq!(regs["CNTP_CTL"].as_u64().unwrap(), 0);
    assert_eq!(regs["CNTP_AIVAL_RELOAD"].as_u64().unwrap(), 0);
    assert_eq!(regs["CNTP_AIVAL_CTL"].as_u64().unwrap(), 0);
    assert_eq!(regs["CNTP_CFG"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["PID4"].as_u64().unwrap(), 0x04);
    assert_eq!(regs["PID0"].as_u64().unwrap(), 0xb7);
}

#[test]
fn test_probe_qemu_sifive_gpio_write_pin_config() {
    let (status, stdout, stderr) =
        run_probe(&["sifive_gpio", "--probe", "scenario", "write pin config"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["VALUE"].as_u64().unwrap(), 0x09);
    assert_eq!(regs["INPUT_EN"].as_u64().unwrap(), 0x0f);
    assert_eq!(regs["OUTPUT_EN"].as_u64().unwrap(), 0x33);
    assert_eq!(regs["PORT"].as_u64().unwrap(), 0x55);
    assert_eq!(regs["PUE"].as_u64().unwrap(), 0xaa);
    assert_eq!(regs["DS"].as_u64().unwrap(), 0xff);
}

#[test]
fn test_probe_qemu_sifive_gpio_narrow_access_regs() {
    let (status, stdout, stderr) = run_probe(&[
        "sifive_gpio",
        "--probe",
        "scenario",
        "narrow access regs",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["INPUT_EN"].as_u64().unwrap(), 0x1234_5678);
    assert_eq!(regs["INPUT_EN_LO8"].as_u64().unwrap(), 0x78);
    assert_eq!(regs["INPUT_EN_LO16"].as_u64().unwrap(), 0x5678);
    assert_eq!(regs["OUTPUT_EN"].as_u64().unwrap(), 0x78);
    assert_eq!(regs["PORT"].as_u64().unwrap(), 0x5678);
}

#[test]
fn test_probe_qemu_sifive_gpio_unaligned_wide_access() {
    let (status, stdout, stderr) = run_probe(&[
        "sifive_gpio",
        "--probe",
        "scenario",
        "unaligned wide access",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["INPUT_EN"].as_u64().unwrap(), 0);
    assert_eq!(regs["INPUT_EN_UNALIGNED1"].as_u64().unwrap(), 0x0100_0000);
    assert_eq!(regs["INPUT_EN_UNALIGNED2"].as_u64().unwrap(), 0x0001_0000);
    assert_eq!(regs["INPUT_EN_UNALIGNED3"].as_u64().unwrap(), 0x0000_0100);
    assert_eq!(regs["OUTPUT_EN"].as_u64().unwrap(), 0x01);
}

#[test]
fn test_probe_qemu_pl061_write_gpio_regs() {
    let (status, stdout, stderr) =
        run_probe(&["pl061", "--probe", "scenario", "write gpio regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["DATA_3FC"].as_u64().unwrap(), 0x05);
    assert_eq!(regs["DIR"].as_u64().unwrap(), 0x0f);
    assert_eq!(regs["ISENSE"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["IBE"].as_u64().unwrap(), 0x02);
    assert_eq!(regs["IEV"].as_u64().unwrap(), 0x03);
    assert_eq!(regs["IM"].as_u64().unwrap(), 0x03);
    assert_eq!(regs["RIS"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["MIS"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["AFSEL"].as_u64().unwrap(), 0x0f);
    assert_eq!(regs["DR2R"].as_u64().unwrap(), 0);
    assert_eq!(regs["LOCK"].as_u64().unwrap(), 0);
    assert_eq!(regs["CR"].as_u64().unwrap(), 0);
    assert_eq!(regs["PID0"].as_u64().unwrap(), 0x61);
    assert_eq!(regs["PID_UNALIGNED1"].as_u64().unwrap(), 0x1000_6161);
    assert_eq!(regs["PID_UNALIGNED2"].as_u64().unwrap(), 0x0010_0061);
    assert_eq!(regs["PID_UNALIGNED3"].as_u64().unwrap(), 0x1000_1061);
    assert_eq!(regs["PID1"].as_u64().unwrap(), 0x10);
    assert_eq!(regs["PID2"].as_u64().unwrap(), 0x04);
    assert_eq!(regs["PID3"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["CID0"].as_u64().unwrap(), 0x0d);
    assert_eq!(regs["CID1"].as_u64().unwrap(), 0xf0);
    assert_eq!(regs["CID2"].as_u64().unwrap(), 0x05);
    assert_eq!(regs["CID3"].as_u64().unwrap(), 0xb1);
}

#[test]
fn test_probe_qemu_pl061_luminary_regs_ignored() {
    let (status, stdout, stderr) =
        run_probe(&["pl061", "--probe", "scenario", "luminary regs ignored"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["DR2R"].as_u64().unwrap(), 0);
    assert_eq!(regs["DR4R"].as_u64().unwrap(), 0);
    assert_eq!(regs["DR8R"].as_u64().unwrap(), 0);
    assert_eq!(regs["ODR"].as_u64().unwrap(), 0);
    assert_eq!(regs["PUR"].as_u64().unwrap(), 0);
    assert_eq!(regs["PDR"].as_u64().unwrap(), 0);
    assert_eq!(regs["SLR"].as_u64().unwrap(), 0);
    assert_eq!(regs["DEN"].as_u64().unwrap(), 0);
    assert_eq!(regs["LOCK"].as_u64().unwrap(), 0);
    assert_eq!(regs["CR"].as_u64().unwrap(), 0);
    assert_eq!(regs["AMSEL"].as_u64().unwrap(), 0);
}

#[test]
fn test_probe_qemu_pl061_unaligned_wide_access() {
    let (status, stdout, stderr) =
        run_probe(&["pl061", "--probe", "scenario", "unaligned wide access"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["DATA_3FC"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["PID_UNALIGNED1"].as_u64().unwrap(), 0x1000_6161);
    assert_eq!(regs["PID_UNALIGNED2"].as_u64().unwrap(), 0x0010_0061);
    assert_eq!(regs["PID_UNALIGNED3"].as_u64().unwrap(), 0x1000_1061);
}

#[test]
fn test_probe_qemu_sifive_u_otp_program_bit() {
    let (status, stdout, stderr) =
        run_probe(&["sifive_u_otp", "--probe", "scenario", "program bit"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["PA"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["PAIO"].as_u64().unwrap(), 0x05);
    assert_eq!(regs["PAS"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["PCE"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["PDIN"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["PDOUT"].as_u64().unwrap(), 0xffff_ffdf);
    assert_eq!(regs["PDSTB"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["PTRIM"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["PWE"].as_u64().unwrap(), 0x01);
}

#[test]
fn test_probe_qemu_sifive_u_otp_pdin_value_shift() {
    let (status, stdout, stderr) =
        run_probe(&["sifive_u_otp", "--probe", "scenario", "pdin value shift"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["PA"].as_u64().unwrap(), 0xfc);
    assert_eq!(regs["PAIO"].as_u64().unwrap(), 0x05);
    assert_eq!(regs["PAS"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["PCE"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["PDIN"].as_u64().unwrap(), 0x02);
    assert_eq!(regs["PDOUT"].as_u64().unwrap(), 0x41);
    assert_eq!(regs["PDSTB"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["PTRIM"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["PWE"].as_u64().unwrap(), 0x01);
}

#[test]
fn test_probe_qemu_fw_cfg_signature() {
    let (status, stdout, stderr) =
        run_probe(&["fw_cfg", "--probe", "scenario", "signature"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["DATA_0"].as_u64().unwrap(), u64::from(b'Q'));
    assert_eq!(regs["DATA_1"].as_u64().unwrap(), u64::from(b'E'));
    assert_eq!(regs["DATA_2"].as_u64().unwrap(), u64::from(b'M'));
    assert_eq!(regs["DATA_3"].as_u64().unwrap(), u64::from(b'U'));
}

#[test]
fn test_probe_qemu_pl181_write_control_regs() {
    let (status, stdout, stderr) =
        run_probe(&["pl181", "--probe", "scenario", "write control regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["POWER"].as_u64().unwrap(), 0x03);
    assert_eq!(regs["CLOCK"].as_u64().unwrap(), 0xff);
    assert_eq!(regs["ARGUMENT"].as_u64().unwrap(), 0xdead_beef);
    assert_eq!(regs["COMMAND"].as_u64().unwrap(), 0x00);
}

#[test]
fn test_probe_qemu_pl181_unaligned_wide_access() {
    let (status, stdout, stderr) =
        run_probe(&["pl181", "--probe", "scenario", "unaligned wide access"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["ARGUMENT"].as_u64().unwrap(), 0);
    assert_eq!(regs["ARGUMENT_UNALIGNED1"].as_u64().unwrap(), 0x0100_0000);
    assert_eq!(regs["ARGUMENT_UNALIGNED2"].as_u64().unwrap(), 0x0001_0000);
    assert_eq!(regs["ARGUMENT_UNALIGNED3"].as_u64().unwrap(), 0x0000_0100);
    assert_eq!(regs["COMMAND"].as_u64().unwrap(), 0x01);
}

#[test]
fn test_probe_qemu_pl011_write_control_regs() {
    let (status, stdout, stderr) =
        run_probe(&["pl011", "--probe", "scenario", "write control regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["FR"].as_u64().unwrap(), 0x90);
    assert_eq!(regs["IBRD"].as_u64().unwrap(), 0xffff);
    assert_eq!(regs["FBRD"].as_u64().unwrap(), 0x3f);
    assert_eq!(regs["LCRH"].as_u64().unwrap(), 0x70);
    assert_eq!(regs["CR"].as_u64().unwrap(), 0x380);
    assert_eq!(regs["IFLS"].as_u64().unwrap(), 0x3f);
    assert_eq!(regs["IMSC"].as_u64().unwrap(), 0x7f2);
    assert_eq!(regs["RIS"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["MIS"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["DMACR"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["PID0"].as_u64().unwrap(), 0x11);
    assert_eq!(regs["PID1"].as_u64().unwrap(), 0x10);
    assert_eq!(regs["PID2"].as_u64().unwrap(), 0x14);
    assert_eq!(regs["PID3"].as_u64().unwrap(), 0x00);
}

#[test]
fn test_probe_qemu_pl011_narrow_access_regs() {
    let (status, stdout, stderr) =
        run_probe(&["pl011", "--probe", "scenario", "narrow access regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["IBRD"].as_u64().unwrap(), 0x5678);
    assert_eq!(regs["IBRD_LO8"].as_u64().unwrap(), 0x78);
    assert_eq!(regs["IBRD_LO16"].as_u64().unwrap(), 0x5678);
    assert_eq!(regs["FBRD"].as_u64().unwrap(), 0x38);
    assert_eq!(regs["CR"].as_u64().unwrap(), 0x5678);
}

#[test]
fn test_probe_qemu_pl011_unaligned_wide_access() {
    let (status, stdout, stderr) =
        run_probe(&["pl011", "--probe", "scenario", "unaligned wide access"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["IBRD"].as_u64().unwrap(), 0x0203);
    assert_eq!(regs["FBRD"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["PID_UNALIGNED1"].as_u64().unwrap(), 0x1000_1111);
    assert_eq!(regs["PID_UNALIGNED2"].as_u64().unwrap(), 0x0010_0011);
    assert_eq!(regs["PID_UNALIGNED3"].as_u64().unwrap(), 0x1000_1011);
}

#[test]
fn test_probe_qemu_pl031_write_alarm_regs() {
    let (status, stdout, stderr) =
        run_probe(&["pl031", "--probe", "scenario", "write alarm regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["MR"].as_u64().unwrap(), 0x1234_5678);
    assert_eq!(regs["LR"].as_u64().unwrap(), 0x11);
    assert_eq!(regs["CR"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["IMSC"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["RIS"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["MIS"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["PID0"].as_u64().unwrap(), 0x31);
    assert_eq!(regs["PID_UNALIGNED1"].as_u64().unwrap(), 0x1000_3131);
    assert_eq!(regs["PID_UNALIGNED2"].as_u64().unwrap(), 0x0010_0031);
    assert_eq!(regs["PID_UNALIGNED3"].as_u64().unwrap(), 0x1000_1031);
    assert_eq!(regs["PID1"].as_u64().unwrap(), 0x10);
    assert_eq!(regs["PID2"].as_u64().unwrap(), 0x14);
    assert_eq!(regs["PID3"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["CID0"].as_u64().unwrap(), 0x0d);
    assert_eq!(regs["CID1"].as_u64().unwrap(), 0xf0);
    assert_eq!(regs["CID2"].as_u64().unwrap(), 0x05);
    assert_eq!(regs["CID3"].as_u64().unwrap(), 0xb1);
}

#[test]
fn test_probe_qemu_pl031_zero_load_alarm() {
    let (status, stdout, stderr) =
        run_probe(&["pl031", "--probe", "scenario", "zero load alarm"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["MR"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["LR"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["CR"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["IMSC"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["RIS"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["MIS"].as_u64().unwrap(), 0x01);
}

#[test]
fn test_probe_qemu_pl031_narrow_access_regs() {
    let (status, stdout, stderr) =
        run_probe(&["pl031", "--probe", "scenario", "narrow access regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["MR"].as_u64().unwrap(), 0x78);
    assert_eq!(regs["MR_LO8"].as_u64().unwrap(), 0x78);
    assert_eq!(regs["MR_LO16"].as_u64().unwrap(), 0x78);
    assert_eq!(regs["LR"].as_u64().unwrap(), 0x5678);
    assert_eq!(regs["LR_LO8"].as_u64().unwrap(), 0x78);
    assert_eq!(regs["LR_LO16"].as_u64().unwrap(), 0x5678);
    assert_eq!(regs["RIS"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["MIS"].as_u64().unwrap(), 0x00);
}

#[test]
fn test_probe_qemu_goldfish_rtc_write_alarm_regs() {
    let (status, stdout, stderr) =
        run_probe(&["goldfish_rtc", "--probe", "scenario", "write alarm regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["ALARM_LOW"].as_u64().unwrap(), 0x2345_6789);
    assert_eq!(regs["ALARM_HIGH"].as_u64().unwrap(), 0x1);
    assert_eq!(regs["IRQ_ENABLED"].as_u64().unwrap(), 0x1);
    assert_eq!(regs["ALARM_STATUS"].as_u64().unwrap(), 0x1);
}

#[test]
fn test_probe_qemu_goldfish_rtc_time_write_does_not_fire_alarm() {
    let (status, stdout, stderr) = run_probe(&[
        "goldfish_rtc",
        "--probe",
        "scenario",
        "time write does not fire alarm",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["ALARM_LOW"].as_u64().unwrap(), 0x0);
    assert_eq!(regs["ALARM_HIGH"].as_u64().unwrap(), 0x1);
    assert_eq!(regs["IRQ_ENABLED"].as_u64().unwrap(), 0x1);
    assert_eq!(regs["ALARM_STATUS"].as_u64().unwrap(), 0x1);
}

#[test]
fn test_probe_qemu_ls7a_rtc_write_match_regs() {
    let (status, stdout, stderr) =
        run_probe(&["ls7a_rtc", "--probe", "scenario", "write match regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["TOYMATCH0"].as_u64().unwrap(), 0x0012_3456);
    assert_eq!(regs["TOYMATCH1"].as_u64().unwrap(), 0x0065_4321);
    assert_eq!(regs["TOYMATCH2"].as_u64().unwrap(), 0);
    assert_eq!(regs["RTCCTRL"].as_u64().unwrap(), 0x2900);
    assert_eq!(regs["RTCMATCH0"].as_u64().unwrap(), 0x1234);
    assert_eq!(regs["RTCMATCH1"].as_u64().unwrap(), 0x5678);
    assert_eq!(regs["RTCMATCH2"].as_u64().unwrap(), 0);
}

#[test]
fn test_probe_qemu_ls7a_rtc_rtc_past_match_preserved() {
    let (status, stdout, stderr) = run_probe(&[
        "ls7a_rtc",
        "--probe",
        "scenario",
        "rtc past match preserved",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["RTCCTRL"].as_u64().unwrap(), 0x2900);
    assert_eq!(regs["RTCMATCH0"].as_u64().unwrap(), 0x80);
    assert_eq!(regs["RTCMATCH1"].as_u64().unwrap(), 0);
    assert_eq!(regs["RTCMATCH2"].as_u64().unwrap(), 0);
}

#[test]
fn test_probe_qemu_ls7a_rtc_rtc_time_write_preserves_match() {
    let (status, stdout, stderr) = run_probe(&[
        "ls7a_rtc",
        "--probe",
        "scenario",
        "rtc time write preserves match",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["RTCCTRL"].as_u64().unwrap(), 0x2900);
    assert_eq!(regs["RTCMATCH0"].as_u64().unwrap(), 0x80);
    assert_eq!(regs["RTCMATCH1"].as_u64().unwrap(), 0);
    assert_eq!(regs["RTCMATCH2"].as_u64().unwrap(), 0);
}

#[test]
fn test_probe_qemu_ls7a_rtc_toy_current_match_preserved() {
    let (status, stdout, stderr) = run_probe(&[
        "ls7a_rtc",
        "--probe",
        "scenario",
        "toy current match preserved",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["TOYMATCH0"].as_u64().unwrap(), 0x0420_0000);
    assert_eq!(regs["TOYMATCH1"].as_u64().unwrap(), 0);
    assert_eq!(regs["RTCCTRL"].as_u64().unwrap(), 0x900);
}

#[test]
fn test_probe_qemu_ls7a_rtc_toy_time_write_preserves_match() {
    let (status, stdout, stderr) = run_probe(&[
        "ls7a_rtc",
        "--probe",
        "scenario",
        "toy time write preserves match",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["TOYMATCH0"].as_u64().unwrap(), 0x0420_0000);
    assert_eq!(regs["TOYMATCH1"].as_u64().unwrap(), 0);
    assert_eq!(regs["RTCCTRL"].as_u64().unwrap(), 0x900);
}

#[test]
fn test_probe_qemu_sifive_uart_write_control_regs() {
    let (status, stdout, stderr) = run_probe(&[
        "sifive_uart",
        "--probe",
        "scenario",
        "write control regs",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["TXFIFO"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["RXFIFO"].as_u64().unwrap(), 0x8000_0000);
    assert_eq!(regs["TXCTRL"].as_u64().unwrap(), 0x0003_0001);
    assert_eq!(regs["RXCTRL"].as_u64().unwrap(), 0x0002_0001);
    assert_eq!(regs["IE"].as_u64().unwrap(), 0x03);
    assert_eq!(regs["IP"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["DIV"].as_u64().unwrap(), 0x1234);
}

#[test]
fn test_probe_qemu_riscv_htif_console_putc() {
    let (status, stdout, stderr) =
        run_probe(&["riscv_htif", "--probe", "scenario", "console putc"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["FROMHOST_LO"].as_u64().unwrap(), 0x141);
    assert_eq!(regs["FROMHOST_HI"].as_u64().unwrap(), 0x0101_0000);
    assert_eq!(regs["TOHOST_LO"].as_u64().unwrap(), 0);
    assert_eq!(regs["TOHOST_HI"].as_u64().unwrap(), 0);
}

#[test]
fn test_probe_qemu_riscv_htif_narrow_write_regs() {
    let (status, stdout, stderr) =
        run_probe(&["riscv_htif", "--probe", "scenario", "narrow write regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["FROMHOST_LO"].as_u64().unwrap(), 0x5678);
    assert_eq!(regs["FROMHOST_B0"].as_u64().unwrap(), 0x78);
    assert_eq!(regs["FROMHOST_W0"].as_u64().unwrap(), 0x5678);
    assert_eq!(regs["FROMHOST_B1"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["FROMHOST_W2"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["FROMHOST_HI"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["TOHOST_LO"].as_u64().unwrap(), 0x78);
    assert_eq!(regs["TOHOST_HI"].as_u64().unwrap(), 0x00);
}

#[test]
fn test_probe_qemu_uart16550_write_divisor_latch() {
    let (status, stdout, stderr) =
        run_probe(&["uart16550", "--probe", "scenario", "write divisor latch"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["DLL"].as_u64().unwrap(), 0x34);
    assert_eq!(regs["DLM"].as_u64().unwrap(), 0x12);
    assert_eq!(regs["IIR"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["LCR"].as_u64().unwrap(), 0x80);
    assert_eq!(regs["LSR"].as_u64().unwrap(), 0x60);
    assert_eq!(regs["SCR"].as_u64().unwrap(), 0x5a);
}

#[test]
fn test_probe_qemu_plic_write_priority_enable_threshold() {
    let (status, stdout, stderr) = run_probe(&[
        "plic",
        "--probe",
        "scenario",
        "write priority enable threshold",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["PRIORITY1"].as_u64().unwrap(), 0x07);
    assert_eq!(regs["PRIORITY2"].as_u64().unwrap(), 0x03);
    assert_eq!(regs["PRIORITY5"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["PRIORITY5_UNALIGNED"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["PENDING0"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["ENABLE0"].as_u64().unwrap(), 0x06);
    assert_eq!(regs["THRESHOLD0"].as_u64().unwrap(), 0x05);
    assert_eq!(regs["CLAIM0"].as_u64().unwrap(), 0x00);
}

#[test]
fn test_probe_qemu_plic_unaligned_priority_access() {
    let (status, stdout, stderr) = run_probe(&[
        "plic",
        "--probe",
        "scenario",
        "unaligned priority access",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["PRIORITY5"].as_u64().unwrap(), 0x07);
    assert_eq!(regs["PRIORITY5_UNALIGNED"].as_u64().unwrap(), 0x00);
}

#[test]
fn test_probe_qemu_aclint_write_msip_mtimecmp() {
    let (status, stdout, stderr) = run_probe(&[
        "aclint",
        "--probe",
        "scenario",
        "write msip and mtimecmp",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["MSIP0"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["MSIP1"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["MTIMECMP0"].as_u64().unwrap(), 0x10);
}

#[test]
fn test_probe_qemu_reset_sifive_test() {
    let (status, stdout, stderr) =
        run_probe(&["sifive_test", "--probe", "reset"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["FINISHER"].as_u64().unwrap(), 0x00);
}

#[test]
fn test_probe_qemu_loongarch_ipi_write_enable_mailbox() {
    let (status, stdout, stderr) = run_probe(&[
        "loongarch_ipi",
        "--probe",
        "scenario",
        "write enable mailbox",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["STATUS"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["ENABLE"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["MAILBOX0"].as_u64().unwrap(), 0x1122_3344_5566_7788);
}

#[test]
fn test_probe_qemu_liointc_map_irq3_enable() {
    let (status, stdout, stderr) =
        run_probe(&["liointc", "--probe", "scenario", "map irq3 enable"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["MAPPER3"].as_u64().unwrap(), 0x11);
    assert_eq!(regs["ISR"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["IEN"].as_u64().unwrap(), 0x08);
    assert_eq!(regs["PER_CORE_ISR0"].as_u64().unwrap(), 0x00);
}

#[test]
fn test_probe_qemu_pch_pic_write_mask_route_vector() {
    let (status, stdout, stderr) = run_probe(&[
        "pch_pic",
        "--probe",
        "scenario",
        "write mask route vector polarity",
    ]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["ID"].as_u64().unwrap(), 0x001f_0001_0700_0000);
    assert_eq!(regs["INT_MASK"].as_u64().unwrap(), 0xffff_ffff_ffff_fffb);
    assert_eq!(regs["HTMSI_EN"].as_u64().unwrap(), 0x04);
    assert_eq!(regs["INT_EDGE"].as_u64().unwrap(), 0x04);
    assert_eq!(regs["ROUTE0"].as_u64().unwrap(), 0x0105_0101);
    assert_eq!(regs["HTMSI_VEC0"].as_u64().unwrap(), 0x0023_0000);
    assert_eq!(regs["INT_STATUS"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["INT_POL"].as_u64().unwrap(), 0x04);
}

#[test]
fn test_probe_qemu_eiointc_write_routing_regs() {
    let (status, stdout, stderr) =
        run_probe(&["eiointc", "--probe", "scenario", "write routing regs"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["ZERO"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["NODEMAP0"].as_u64().unwrap(), 0x0102_0304);
    assert_eq!(regs["IPMAP0"].as_u64().unwrap(), 0x08);
    assert_eq!(regs["ENABLE0"].as_u64().unwrap(), 0x04);
    assert_eq!(regs["BOUNCE0"].as_u64().unwrap(), 0x02);
    assert_eq!(regs["CORE_ISR0"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["COREMAP0"].as_u64().unwrap(), 0x01);
}

#[test]
fn test_probe_qemu_cmgcr_write_masked_gcr_base() {
    let (status, stdout, stderr) =
        run_probe(&["cmgcr", "--probe", "scenario", "write masked gcr base"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["GCR_CONFIG"].as_u64().unwrap(), 0x00);
    assert_eq!(regs["GCR_BASE"].as_u64().unwrap(), 0x1fb8_0000);
    assert_eq!(regs["GCR_REV"].as_u64().unwrap(), 0x0a00);
    assert_eq!(regs["GCR_CPC_STATUS"].as_u64().unwrap(), 0x01);
    assert_eq!(regs["GCR_L2_CONFIG"].as_u64().unwrap(), 0x0010_0000);
}

#[test]
fn test_probe_qemu_cpc_run_and_stop_vp0() {
    let (status, stdout, stderr) =
        run_probe(&["cpc", "--probe", "scenario", "run and stop vp0"]);

    if status.code() == Some(77) {
        eprintln!("SKIP: QEMU not available ({stderr})");
        return;
    }

    assert!(status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("valid JSON");
    let regs = &parsed["registers"];
    assert_eq!(regs["CM_STAT_CONF"].as_u64().unwrap(), 0x0030_0000);
    assert_eq!(regs["CL0_STAT_CONF"].as_u64().unwrap(), 0x0038_0000);
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
