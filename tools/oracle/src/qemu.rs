//! QEMU qtest backend for runtime device probing.
//!
//! Spawns a QEMU instance with `-qtest stdio` and issues MMIO read/write
//! commands over stdin to observe guest-visible register and IRQ state.
//!
//! Commands are batched through stdin.  Responses are collected from stdout
//! after terminating QEMU, which gives the qtest stdio backend an EOF on the
//! output pipe.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::{
    Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio,
};
use std::time::Duration;

const DEFAULT_SETTLE_DELAY: Duration = Duration::from_millis(500);
const RETRY_SETTLE_DELAY: Duration = Duration::from_secs(3);
const RESPONSE_COUNT_MISMATCH: &str = "qtest response count mismatch";

/// A running QEMU instance controlled via qtest protocol.
///
/// Commands are buffered in stdin and flushed together.  QEMU stdout/stderr
/// are piped so qtest responses and trace output can be collected after
/// QEMU terminates.
pub struct QemuProbe {
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    child: Option<Child>,
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
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot spawn QEMU '{qemu_bin}': {e}"))?;

        let stdin = child.stdin.take().ok_or("no QEMU stdin")?;
        let stdout = child.stdout.take().ok_or("no QEMU stdout")?;
        let stderr = child.stderr.take().ok_or("no QEMU stderr")?;

        Ok(Self {
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr: Some(stderr),
            child: Some(child),
            cmd_count: 0,
        })
    }

    /// Queue a typed qtest MMIO read.
    ///
    /// Uses typed accessors (`readb`/`readw`/`readl`/`readq`) that
    /// handle target-endian decoding, so the returned value matches
    /// the guest-visible register value.  Raw `read <addr> <size>`
    /// transmits hex byte streams without endian conversion.
    pub fn send_read(&mut self, addr: u64, size: u8) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("probe already closed")?;
        let cmd = match size {
            1 => format!("readb {addr:#x}"),
            2 => format!("readw {addr:#x}"),
            4 => format!("readl {addr:#x}"),
            8 => format!("readq {addr:#x}"),
            other => {
                return Err(format!("unsupported qtest read size {other}"))
            }
        };
        writeln!(stdin, "{cmd}").map_err(|e| format!("qemu send read: {e}"))?;
        self.cmd_count += 1;
        Ok(())
    }

    /// Queue a typed qtest MMIO write.
    pub fn send_write(
        &mut self,
        addr: u64,
        size: u8,
        value: u64,
    ) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("probe already closed")?;
        let cmd = match size {
            1 => format!("writeb {addr:#x} {value:#x}"),
            2 => format!("writew {addr:#x} {value:#x}"),
            4 => format!("writel {addr:#x} {value:#x}"),
            8 => format!("writeq {addr:#x} {value:#x}"),
            other => {
                return Err(format!("unsupported qtest write size {other}"))
            }
        };
        writeln!(stdin, "{cmd}")
            .map_err(|e| format!("qemu send write: {e}"))?;
        self.cmd_count += 1;
        Ok(())
    }

    /// Queue a typed qtest I/O port read.
    pub fn send_io_read(&mut self, port: u64, size: u8) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("probe already closed")?;
        let cmd = match size {
            1 => format!("inb {port:#x}"),
            2 => format!("inw {port:#x}"),
            4 => format!("inl {port:#x}"),
            other => {
                return Err(format!("unsupported qtest I/O read size {other}"))
            }
        };
        writeln!(stdin, "{cmd}").map_err(|e| format!("qemu send in: {e}"))?;
        self.cmd_count += 1;
        Ok(())
    }

    /// Queue a typed qtest I/O port write.
    pub fn send_io_write(
        &mut self,
        port: u64,
        size: u8,
        value: u64,
    ) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("probe already closed")?;
        let cmd = match size {
            1 => format!("outb {port:#x} {value:#x}"),
            2 => format!("outw {port:#x} {value:#x}"),
            4 => format!("outl {port:#x} {value:#x}"),
            other => {
                return Err(format!("unsupported qtest I/O write size {other}"))
            }
        };
        writeln!(stdin, "{cmd}").map_err(|e| format!("qemu send out: {e}"))?;
        self.cmd_count += 1;
        Ok(())
    }

    /// Queue a raw qtest command for non-MMIO interactions.
    pub fn send_command(&mut self, command: &str) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("probe already closed")?;
        writeln!(stdin, "{command}")
            .map_err(|e| format!("qemu send command '{command}': {e}"))?;
        self.cmd_count += 1;
        Ok(())
    }

    /// Flush commands, close stdin, terminate QEMU, and collect all
    /// response values (only `OK 0x<hex>` results; write ACKs are
    /// filtered out).
    pub fn finish(mut self) -> Result<Vec<u64>, String> {
        let content = self.finish_content(DEFAULT_SETTLE_DELAY)?;

        Self::parse_values(&content)
    }

    fn finish_after(
        mut self,
        settle_delay: Duration,
    ) -> Result<Vec<u64>, String> {
        let content = self.finish_content(settle_delay)?;

        Self::parse_values(&content)
    }

    fn parse_values(content: &str) -> Result<Vec<u64>, String> {
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

    /// Flush commands and collect qtest values plus IRQ edge/final state.
    pub fn finish_qtest(
        mut self,
    ) -> Result<(Vec<u64>, RegSnapshot, IrqSnapshot), String> {
        let content = self.finish_content(DEFAULT_SETTLE_DELAY)?;

        if content.lines().any(|l| l.trim().starts_with("FAIL")) {
            return Err("qtest command failed".to_string());
        }

        let mut values = Vec::new();
        let mut regs = RegSnapshot::new();
        let mut irqs = IrqSnapshot::new();

        for line in content.lines().map(str::trim) {
            if let Some(hex) = line.strip_prefix("OK 0x") {
                if let Ok(value) = u64::from_str_radix(hex, 16) {
                    values.push(value);
                }
                continue;
            }

            let Some(rest) = line.strip_prefix("IRQ ") else {
                continue;
            };
            let mut parts = rest.split_whitespace();
            let Some(action) = parts.next() else {
                continue;
            };
            let Some(num) = parts.next().and_then(|n| n.parse::<u32>().ok())
            else {
                continue;
            };

            match action {
                "raise" => {
                    *regs.entry(format!("IRQ{num}_RAISES")).or_insert(0) += 1;
                    irqs.insert(num, true);
                }
                "lower" => {
                    *regs.entry(format!("IRQ{num}_LOWERS")).or_insert(0) += 1;
                    irqs.insert(num, false);
                }
                _ => {}
            }
        }

        for line in content.lines().map(str::trim) {
            if let Some(intensity) = parse_led_intensity(line) {
                regs.insert("INTENSITY".to_string(), intensity);
            }
            if line.contains("qemu_system_shutdown_request") {
                regs.insert("ACTION".to_string(), 1);
            }
        }

        Ok((values, regs, irqs))
    }

    fn finish_content(
        &mut self,
        settle_delay: Duration,
    ) -> Result<String, String> {
        // Flush and drop stdin so QEMU sees EOF.
        if let Some(stdin) = self.stdin.as_mut() {
            stdin.flush().map_err(|e| format!("flush stdin: {e}"))?;
        }
        drop(self.stdin.take());

        // Give QEMU time to process commands and write responses
        // before we send SIGTERM.
        std::thread::sleep(settle_delay);

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

        let mut content = String::new();
        if let Some(stdout) = self.stdout.as_mut() {
            stdout
                .read_to_string(&mut content)
                .map_err(|e| format!("read qemu stdout: {e}"))?;
        }
        if let Some(stderr) = self.stderr.as_mut() {
            stderr
                .read_to_string(&mut content)
                .map_err(|e| format!("read qemu stderr: {e}"))?;
        }

        Ok(content)
    }
}

fn parse_led_intensity(line: &str) -> Option<u64> {
    let start = line.find("intensity: ")? + "intensity: ".len();
    let digits: String = line[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
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
    }
}

/// Resolve the QEMU binary path for a given arch hint.
///
/// Checks environment variables first, then falls back to PATH.
/// - `MACHINA_QEMU_SYSTEM_RISCV64_BOSTON` → Boston-aia-capable
///   `qemu-system-riscv64`
/// - `MACHINA_QEMU_SYSTEM_RISCV64` → `qemu-system-riscv64`
/// - `MACHINA_QEMU_SYSTEM_LOONGARCH64` → `qemu-system-loongarch64`
/// - `MACHINA_QEMU_SYSTEM_AARCH64` → `qemu-system-aarch64`
/// - `MACHINA_QEMU_SYSTEM_ARM` → `qemu-system-arm`
/// - `MACHINA_QEMU_SYSTEM_X86_64` → `qemu-system-x86_64`
/// - `MACHINA_QEMU_SYSTEM_M68K` → `qemu-system-m68k`
/// - `MACHINA_QEMU_SYSTEM_MIPS64EL` → `qemu-system-mips64el`
pub fn find_qemu(machine_hint: &str) -> Option<String> {
    if machine_hint == "riscv-boston" {
        return std::env::var("MACHINA_QEMU_SYSTEM_RISCV64_BOSTON").ok();
    }

    let (env_var, fallback) = if machine_hint.contains("riscv") {
        ("MACHINA_QEMU_SYSTEM_RISCV64", "qemu-system-riscv64")
    } else if machine_hint.contains("loongarch") {
        ("MACHINA_QEMU_SYSTEM_LOONGARCH64", "qemu-system-loongarch64")
    } else if machine_hint.contains("aarch64") {
        ("MACHINA_QEMU_SYSTEM_AARCH64", "qemu-system-aarch64")
    } else if machine_hint.contains("arm") {
        ("MACHINA_QEMU_SYSTEM_ARM", "qemu-system-arm")
    } else if machine_hint.contains("x86_64") {
        ("MACHINA_QEMU_SYSTEM_X86_64", "qemu-system-x86_64")
    } else if machine_hint.contains("m68k") {
        ("MACHINA_QEMU_SYSTEM_M68K", "qemu-system-m68k")
    } else if machine_hint.contains("mips64el") {
        ("MACHINA_QEMU_SYSTEM_MIPS64EL", "qemu-system-mips64el")
    } else {
        return std::env::var("MACHINA_QEMU_SYSTEM_RISCV64")
            .ok()
            .or_else(|| which("qemu-system-riscv64"))
            .or_else(|| std::env::var("MACHINA_QEMU_SYSTEM_LOONGARCH64").ok())
            .or_else(|| which("qemu-system-loongarch64"))
            .or_else(|| std::env::var("MACHINA_QEMU_SYSTEM_AARCH64").ok())
            .or_else(|| which("qemu-system-aarch64"))
            .or_else(|| std::env::var("MACHINA_QEMU_SYSTEM_ARM").ok())
            .or_else(|| which("qemu-system-arm"))
            .or_else(|| std::env::var("MACHINA_QEMU_SYSTEM_X86_64").ok())
            .or_else(|| which("qemu-system-x86_64"))
            .or_else(|| std::env::var("MACHINA_QEMU_SYSTEM_M68K").ok())
            .or_else(|| which("qemu-system-m68k"))
            .or_else(|| std::env::var("MACHINA_QEMU_SYSTEM_MIPS64EL").ok())
            .or_else(|| which("qemu-system-mips64el"));
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

/// qtest-only descriptor for devices without guest-visible registers.
pub struct QtestDeviceDescriptor {
    pub qemu_machine: &'static str,
    pub arch_hint: &'static str,
    pub qemu_extra_args: &'static [&'static str],
    pub scenarios: &'static [QtestScenarioDescriptor],
}

pub struct QtestScenarioDescriptor {
    pub name: &'static str,
    pub commands: &'static [&'static str],
}

fn uses_io_ports(descriptor: &DeviceDescriptor) -> bool {
    descriptor.arch_hint.contains("ioport")
}

/// Probe a device's reset state via QEMU qtest.
///
/// Returns the register snapshot from QEMU guest-visible state,
/// or a `SKIP:` reason string if QEMU is unavailable.
pub fn probe_reset(
    descriptor: &DeviceDescriptor,
) -> Result<(RegSnapshot, IrqSnapshot), String> {
    match probe_reset_with_delay(descriptor, DEFAULT_SETTLE_DELAY) {
        Ok(snapshot) if should_retry_zero_snapshot(descriptor, &snapshot.0) => {
            probe_reset_with_delay(descriptor, RETRY_SETTLE_DELAY)
        }
        Ok(snapshot) => Ok(snapshot),
        Err(error) if error.starts_with(RESPONSE_COUNT_MISMATCH) => {
            probe_reset_with_delay(descriptor, RETRY_SETTLE_DELAY)
        }
        Err(error) => Err(error),
    }
}

fn probe_reset_with_delay(
    descriptor: &DeviceDescriptor,
    settle_delay: Duration,
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
        if uses_io_ports(descriptor) {
            probe
                .send_io_read(addr, size)
                .map_err(|e| format!("qemu send in {addr:#x}/{size}: {e}"))?;
        } else {
            probe
                .send_read(addr, size)
                .map_err(|e| format!("qemu send read {addr:#x}/{size}: {e}"))?;
        }
    }

    let values = probe
        .finish_after(settle_delay)
        .map_err(|e| format!("qemu collect responses: {e}"))?;

    if values.len() != descriptor.registers.len() {
        return Err(format!(
            "{RESPONSE_COUNT_MISMATCH}: expected {}, got {}",
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
    match probe_scenario_with_delay(
        descriptor,
        scenario_name,
        DEFAULT_SETTLE_DELAY,
    ) {
        Ok(snapshot) if should_retry_zero_snapshot(descriptor, &snapshot.0) => {
            probe_scenario_with_delay(
                descriptor,
                scenario_name,
                RETRY_SETTLE_DELAY,
            )
        }
        Ok(snapshot) => Ok(snapshot),
        Err(error) if error.starts_with(RESPONSE_COUNT_MISMATCH) => {
            probe_scenario_with_delay(
                descriptor,
                scenario_name,
                RETRY_SETTLE_DELAY,
            )
        }
        Err(error) => Err(error),
    }
}

fn probe_scenario_with_delay(
    descriptor: &DeviceDescriptor,
    scenario_name: &str,
    settle_delay: Duration,
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
        if uses_io_ports(descriptor) {
            probe
                .send_io_write(addr, size, value)
                .map_err(|e| format!("qemu send out {addr:#x}/{size}: {e}"))?;
        } else {
            probe.send_write(addr, size, value).map_err(|e| {
                format!("qemu send write {addr:#x}/{size}: {e}")
            })?;
        }
    }

    for &(_name, offset, size) in descriptor.registers {
        let addr = descriptor.mmio_base + offset;
        if uses_io_ports(descriptor) {
            probe
                .send_io_read(addr, size)
                .map_err(|e| format!("qemu send in {addr:#x}/{size}: {e}"))?;
        } else {
            probe
                .send_read(addr, size)
                .map_err(|e| format!("qemu send read {addr:#x}/{size}: {e}"))?;
        }
    }

    let values = probe
        .finish_after(settle_delay)
        .map_err(|e| format!("qemu collect responses: {e}"))?;

    // `values` contains only read results — write "OK" ACKs are
    // filtered out by the parser.
    if values.len() != descriptor.registers.len() {
        return Err(format!(
            "{RESPONSE_COUNT_MISMATCH}: expected {}, got {}",
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

fn should_retry_zero_snapshot(
    descriptor: &DeviceDescriptor,
    regs: &RegSnapshot,
) -> bool {
    descriptor.arch_hint == "riscv-boston"
        && !regs.is_empty()
        && regs.values().all(|&value| value == 0)
}

pub fn probe_qtest_reset(
    descriptor: &QtestDeviceDescriptor,
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

    let probe = QemuProbe::spawn(&qemu_bin, descriptor.qemu_machine, &extra)
        .map_err(|e| format!("SKIP: {e}"))?;
    let (_values, regs, irqs) = probe
        .finish_qtest()
        .map_err(|e| format!("qemu collect responses: {e}"))?;
    Ok((regs, irqs))
}

pub fn probe_qtest_scenario(
    descriptor: &QtestDeviceDescriptor,
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

    for command in scenario.commands {
        probe.send_command(command)?;
    }

    let (_values, regs, irqs) = probe
        .finish_qtest()
        .map_err(|e| format!("qemu collect responses: {e}"))?;
    Ok((regs, irqs))
}
