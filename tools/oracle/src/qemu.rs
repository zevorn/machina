//! QEMU qtest backend for runtime device probing.
//!
//! Spawns a QEMU instance with `-qtest stdio` and issues MMIO read/write
//! commands over stdin to observe guest-visible register and IRQ state.
//!
//! QEMU's libc uses full buffering when stdout is a pipe, so responses
//! would stall indefinitely.  We work around this by redirecting stdout to
//! a temp file, batching all commands, closing stdin, sending SIGTERM to
//! flush QEMU's stdio buffers on exit, then reading the file.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A running QEMU instance controlled via qtest protocol.
///
/// Commands are buffered in stdin and flushed together.  QEMU stdout goes to
/// a temp file to avoid pipe-buffering stalls.  Call [`finish`] to flush
/// commands, terminate QEMU, and collect responses.
pub struct QemuProbe {
    stdin: Option<ChildStdin>,
    child: Option<Child>,
    stdout_path: PathBuf,
    cmd_count: usize,
}

impl QemuProbe {
    /// Spawn a QEMU instance for the given machine.
    ///
    /// `qemu_bin` — path to the QEMU system binary
    /// `machine` — `-M` argument (e.g. "sifive_e", "virt")
    /// `extra_args` — additional QEMU CLI arguments
    pub fn spawn(
        qemu_bin: &str,
        machine: &str,
        extra_args: &[String],
    ) -> Result<Self, String> {
        let unique_id = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let stdout_path = std::env::temp_dir().join(format!(
            "machina-qemu-stdout-{}-{}",
            std::process::id(),
            unique_id
        ));
        let stdout_file = fs::File::create(&stdout_path).map_err(|e| {
            format!("create stdout file {}: {e}", stdout_path.display())
        })?;

        let mut child = Command::new(qemu_bin)
            .arg("-M")
            .arg(machine)
            .args(extra_args)
            .arg("-nodefaults")
            .arg("-nographic")
            .arg("-display")
            .arg("none")
            .arg("-qtest")
            .arg("stdio")
            .stdin(Stdio::piped())
            .stdout(stdout_file)
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                let _ = fs::remove_file(&stdout_path);
                format!("cannot spawn QEMU '{qemu_bin}': {e}")
            })?;

        let stdin = child.stdin.take().ok_or("no QEMU stdin")?;

        Ok(Self {
            stdin: Some(stdin),
            child: Some(child),
            stdout_path,
            cmd_count: 0,
        })
    }

    /// Queue a qtest MMIO read: `read <addr> <size>`.
    pub fn send_read(&mut self, addr: u64, size: u8) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("probe already closed")?;
        writeln!(stdin, "read {addr:#x} {size}")
            .map_err(|e| format!("qemu send read: {e}"))?;
        self.cmd_count += 1;
        Ok(())
    }

    /// Queue a qtest MMIO write: `write <addr> <size> <value>`.
    pub fn send_write(
        &mut self,
        addr: u64,
        size: u8,
        value: u64,
    ) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("probe already closed")?;
        writeln!(stdin, "write {addr:#x} {size} {value:#x}")
            .map_err(|e| format!("qemu send write: {e}"))?;
        self.cmd_count += 1;
        Ok(())
    }

    /// Flush commands, close stdin, terminate QEMU, and collect all
    /// response values (only `OK 0x<hex>` results; write ACKs are
    /// filtered out).
    pub fn finish(mut self) -> Result<Vec<u64>, String> {
        // Flush and drop stdin so QEMU sees EOF.
        if let Some(stdin) = self.stdin.as_mut() {
            stdin.flush().map_err(|e| format!("flush stdin: {e}"))?;
        }
        drop(self.stdin.take());

        // Give QEMU time to process commands and write responses
        // before we send SIGTERM.
        std::thread::sleep(std::time::Duration::from_millis(500));

        // QEMU does not exit on stdin EOF alone — send SIGTERM.
        if let Some(ref child) = self.child {
            let pid = child.id() as libc::pid_t;
            // SAFETY: kill(2) is async-signal-safe, pid is valid.
            let rc = unsafe { libc::kill(pid, libc::SIGTERM) };
            if rc != 0 {
                let err = std::io::Error::last_os_error();
                return Err(format!("send SIGTERM to {pid}: {err}"));
            }
        }

        // Wait for QEMU to exit (SIGTERM triggers graceful shutdown /
        // stdio fflush).
        if let Some(mut child) = self.child.take() {
            child.wait().map_err(|e| format!("qemu wait: {e}"))?;
        }

        let content = fs::read_to_string(&self.stdout_path).map_err(|e| {
            format!("read stdout file {}: {e}", self.stdout_path.display())
        })?;

        // Clean up temp file.
        let _ = fs::remove_file(&self.stdout_path);

        // Check for qtest FAIL lines before parsing values.
        if content.lines().any(|l| l.trim().starts_with("FAIL")) {
            return Err("qtest command failed".to_string());
        }

        let values: Vec<u64> = content
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                line.strip_prefix("OK 0x")
                    .and_then(|hex| u64::from_str_radix(hex, 16).ok())
            })
            .collect();

        Ok(values)
    }
}

impl Drop for QemuProbe {
    fn drop(&mut self) {
        self.stdin.take();
        if let Some(mut child) = self.child.take() {
            let pid = child.id() as libc::pid_t;
            // SAFETY: kill(2) is async-signal-safe, pid is valid.
            let _ = unsafe { libc::kill(pid, libc::SIGTERM) };
            let _ = child.wait();
        }
        let _ = fs::remove_file(&self.stdout_path);
    }
}

/// Resolve the QEMU binary path for a given arch hint.
///
/// Checks environment variables first, then falls back to PATH.
/// - `MACHINA_QEMU_SYSTEM_RISCV64` → `qemu-system-riscv64`
/// - `MACHINA_QEMU_SYSTEM_LOONGARCH64` → `qemu-system-loongarch64`
pub fn find_qemu(machine_hint: &str) -> Option<String> {
    let (env_var, fallback) = if machine_hint.contains("riscv") {
        ("MACHINA_QEMU_SYSTEM_RISCV64", "qemu-system-riscv64")
    } else if machine_hint.contains("loongarch") {
        ("MACHINA_QEMU_SYSTEM_LOONGARCH64", "qemu-system-loongarch64")
    } else {
        return std::env::var("MACHINA_QEMU_SYSTEM_RISCV64")
            .ok()
            .or_else(|| which("qemu-system-riscv64"))
            .or_else(|| std::env::var("MACHINA_QEMU_SYSTEM_LOONGARCH64").ok())
            .or_else(|| which("qemu-system-loongarch64"));
    };

    std::env::var(env_var).ok().or_else(|| which(fallback))
}

fn which(name: &str) -> Option<String> {
    let output = std::process::Command::new("which")
        .arg(name)
        .output()
        .ok()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(path);
        }
    }
    None
}

/// A snapshot captured from a running QEMU instance.
pub type RegSnapshot = BTreeMap<String, u64>;
pub type IrqSnapshot = BTreeMap<u32, bool>;

/// Device descriptor: metadata for how to probe a specific device.
pub struct DeviceDescriptor {
    /// QEMU machine name (`-M` argument).
    pub qemu_machine: &'static str,
    /// Architecture hint for QEMU binary selection.
    pub arch_hint: &'static str,
    /// Extra QEMU arguments (e.g. for adding devices to virt machine).
    pub qemu_extra_args: &'static [&'static str],
    /// Base MMIO address of the device in the target machine.
    pub mmio_base: u64,
    /// Registers to read: (name, offset from mmio_base, access size).
    pub registers: &'static [(&'static str, u64, u8)],
    /// Scenarios: sequences of writes.
    pub scenarios: &'static [ScenarioDescriptor],
}

pub struct ScenarioDescriptor {
    pub name: &'static str,
    /// (offset from mmio_base, value, size)
    pub writes: &'static [(u64, u64, u8)],
}

/// Probe a device's reset state via QEMU qtest.
///
/// Returns the register snapshot from QEMU guest-visible state,
/// or a `SKIP:` reason string if QEMU is unavailable.
pub fn probe_reset(
    descriptor: &DeviceDescriptor,
) -> Result<(RegSnapshot, IrqSnapshot), String> {
    let qemu_bin = find_qemu(descriptor.arch_hint).ok_or_else(|| {
        format!(
            "SKIP: QEMU binary not found for arch hint '{}'",
            descriptor.arch_hint
        )
    })?;

    let extra: Vec<String> = descriptor
        .qemu_extra_args
        .iter()
        .map(|s| s.to_string())
        .collect();

    let mut probe =
        QemuProbe::spawn(&qemu_bin, descriptor.qemu_machine, &extra)
            .map_err(|e| format!("SKIP: {e}"))?;

    for &(_name, offset, size) in descriptor.registers {
        let addr = descriptor.mmio_base + offset;
        probe
            .send_read(addr, size)
            .map_err(|e| format!("qemu send read {addr:#x}/{size}: {e}"))?;
    }

    let values = probe
        .finish()
        .map_err(|e| format!("qemu collect responses: {e}"))?;

    if values.len() != descriptor.registers.len() {
        return Err(format!(
            "qtest response count mismatch: expected {}, got {}",
            descriptor.registers.len(),
            values.len()
        ));
    }

    let mut regs = RegSnapshot::new();
    for (i, &(name, _offset, _size)) in descriptor.registers.iter().enumerate()
    {
        let val = values[i];
        regs.insert(name.to_string(), val);
    }

    Ok((regs, IrqSnapshot::new()))
}

/// Probe a scenario by applying writes then reading register state.
pub fn probe_scenario(
    descriptor: &DeviceDescriptor,
    scenario_name: &str,
) -> Result<(RegSnapshot, IrqSnapshot), String> {
    let scenario = descriptor
        .scenarios
        .iter()
        .find(|s| s.name == scenario_name)
        .ok_or_else(|| {
            format!("unknown scenario '{scenario_name}' for device")
        })?;

    let qemu_bin = find_qemu(descriptor.arch_hint).ok_or_else(|| {
        format!(
            "SKIP: QEMU binary not found for arch hint '{}'",
            descriptor.arch_hint
        )
    })?;

    let extra: Vec<String> = descriptor
        .qemu_extra_args
        .iter()
        .map(|s| s.to_string())
        .collect();

    let mut probe =
        QemuProbe::spawn(&qemu_bin, descriptor.qemu_machine, &extra)
            .map_err(|e| format!("SKIP: {e}"))?;

    for &(offset, value, size) in scenario.writes {
        let addr = descriptor.mmio_base + offset;
        probe
            .send_write(addr, size, value)
            .map_err(|e| format!("qemu send write {addr:#x}/{size}: {e}"))?;
    }

    for &(_name, offset, size) in descriptor.registers {
        let addr = descriptor.mmio_base + offset;
        probe
            .send_read(addr, size)
            .map_err(|e| format!("qemu send read {addr:#x}/{size}: {e}"))?;
    }

    let values = probe
        .finish()
        .map_err(|e| format!("qemu collect responses: {e}"))?;

    // `values` contains only read results — write "OK" ACKs are
    // filtered out by the parser.
    if values.len() != descriptor.registers.len() {
        return Err(format!(
            "qtest response count mismatch: expected {}, got {}",
            descriptor.registers.len(),
            values.len()
        ));
    }

    let mut regs = RegSnapshot::new();
    for (i, &(name, _offset, _size)) in descriptor.registers.iter().enumerate()
    {
        let val = values[i];
        regs.insert(name.to_string(), val);
    }

    Ok((regs, IrqSnapshot::new()))
}
