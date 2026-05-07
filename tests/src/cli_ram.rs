//! Regression tests for #77: `-m 0` should fail fast with a clear
//! error instead of silently booting the machine with zero bytes of
//! RAM and producing confusing memory access errors later.

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

#[test]
fn ram_zero_is_rejected() {
    ensure_machina_built();

    let output = Command::new(machina_bin())
        .args(["-m", "0"])
        .output()
        .expect("failed to spawn machina");

    assert!(
        !output.status.success(),
        "machina should reject -m 0; got success exit",
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("-m: RAM size must be greater than 0"),
        "expected RAM-size validation error in stderr; got: {stderr}",
    );
}

#[test]
fn ram_zero_with_m_suffix_is_rejected() {
    ensure_machina_built();

    let output = Command::new(machina_bin())
        .args(["-m", "0M"])
        .output()
        .expect("failed to spawn machina");

    assert!(
        !output.status.success(),
        "machina should reject -m 0M; got success exit",
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("-m: RAM size must be greater than 0"),
        "expected RAM-size validation error in stderr; got: {stderr}",
    );
}
