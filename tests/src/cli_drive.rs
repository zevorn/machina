//! Regression tests for #78: `-drive` only honours `file=<path>`, but
//! other suboptions such as `format=qcow2`, `id=disk0`, and `if=none`
//! used to be silently dropped. Now machina emits a warning for each
//! unsupported suboption so the user knows the request had no effect.

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

/// Pair the `-drive` value with a sentinel `-kernel` arg that fails
/// parse fast, so machina never tries to boot but warnings still land
/// in stderr.
fn run_with_drive(value: &str) -> String {
    ensure_machina_built();

    let output = Command::new(machina_bin())
        .args(["-drive", value, "-kernel", "/no/such/kernel.img"])
        .output()
        .expect("failed to spawn machina");

    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn drive_format_suboption_warns() {
    let stderr = run_with_drive("file=disk.img,format=qcow2");
    assert!(
        stderr.contains("warning") && stderr.contains("-drive format"),
        "expected warning naming format=; got: {stderr}",
    );
    assert!(
        stderr.contains("not implemented"),
        "expected 'not implemented' wording; got: {stderr}",
    );
}

#[test]
fn drive_multiple_unknown_suboptions_each_warn() {
    let stderr = run_with_drive("file=disk.img,format=qcow2,id=disk0,if=none");
    for key in ["format", "id", "if"] {
        assert!(
            stderr.contains(&format!("-drive {key}")),
            "expected per-suboption warning for {key}; got: {stderr}",
        );
    }
}

#[test]
fn drive_file_only_does_not_warn() {
    let stderr = run_with_drive("file=disk.img");
    assert!(
        !stderr.contains("-drive ") || !stderr.contains("not implemented"),
        "file-only -drive should not warn; got: {stderr}",
    );
}

#[test]
fn drive_missing_file_is_rejected() {
    let stderr = run_with_drive("format=qcow2,id=disk0");
    assert!(
        stderr.contains("-drive: missing file=<path>"),
        "expected missing-file error; got: {stderr}",
    );
}
