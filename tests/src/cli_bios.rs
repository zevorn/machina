//! Regression tests for #84: repeated `-bios` arguments must be
//! rejected with a clear error instead of silently letting the later
//! value override the earlier one.

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn machina_bin() -> PathBuf {
    let base = project_root().join("target").join("debug").join("machina");
    if cfg!(windows) {
        base.with_extension("exe")
    } else {
        base
    }
}

fn ensure_machina_built() {
    // Serialise concurrent builds: on Windows multiple linkers cannot
    // write the same .exe simultaneously.
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = LOCK.get_or_init(Mutex::default).lock().unwrap();

    let status = Command::new("cargo")
        .args(["build", "-p", "machina-emu"])
        .current_dir(project_root())
        .status()
        .expect("cargo build machina-emu failed");
    assert!(status.success(), "cargo build machina-emu failed");
}

fn assert_repeated_bios_rejected(args: &[&str]) {
    ensure_machina_built();

    let output = Command::new(machina_bin())
        .args(args)
        .output()
        .expect("failed to spawn machina");

    assert!(
        !output.status.success(),
        "machina should reject repeated -bios; got success exit for {args:?}",
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("-bios: already specified"),
        "expected repeated-bios rejection in stderr; got: {stderr}",
    );
    assert!(
        !stderr.contains("panicked at"),
        "expected friendly error, not panic; got: {stderr}",
    );
}

#[test]
fn repeated_bios_builtin_then_path_is_rejected() {
    assert_repeated_bios_rejected(&[
        "-bios",
        "builtin",
        "-bios",
        "firmware.bin",
    ]);
}

#[test]
fn repeated_bios_path_then_builtin_is_rejected() {
    assert_repeated_bios_rejected(&[
        "-bios",
        "firmware.bin",
        "-bios",
        "builtin",
    ]);
}

#[test]
fn repeated_bios_two_paths_is_rejected() {
    assert_repeated_bios_rejected(&["-bios", "fw_a.bin", "-bios", "fw_b.bin"]);
}
