//! Regression tests for #86: `machina -h` / `--help` should include
//! an Examples section so new users can see how to combine options to
//! start a complete emulation environment.

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

fn run_help(flag: &str) -> String {
    ensure_machina_built();
    let output = Command::new(machina_bin())
        .arg(flag)
        .output()
        .expect("failed to spawn machina");
    assert!(
        output.status.success(),
        "{flag} should exit 0; got {:?}",
        output.status,
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn help_short_flag_includes_examples_section() {
    let stderr = run_help("-h");
    assert!(
        stderr.contains("Examples:"),
        "expected an Examples section in -h output; got: {stderr}",
    );
}

#[test]
fn help_long_flag_includes_examples_section() {
    let stderr = run_help("--help");
    assert!(
        stderr.contains("Examples:"),
        "expected an Examples section in --help output; got: {stderr}",
    );
}

#[test]
fn help_examples_cover_built_in_sbi_disk_network_and_gdb() {
    let stderr = run_help("--help");
    for snippet in [
        "machina -bios builtin -kernel vmlinux",
        "-drive file=rootfs.img",
        "-netdev tap,id=net0,ifname=tap0",
        "-device virtio-net-device,netdev=net0",
        "machina -kernel vmlinux -s -S",
    ] {
        assert!(
            stderr.contains(snippet),
            "expected example to contain `{snippet}`; got: {stderr}",
        );
    }
}
