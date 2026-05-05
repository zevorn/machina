//! Oracle dynamic comparison tool.
//!
//! Provides runtime comparison of Machina device behavior against
//! a reference implementation (QEMU) via command-line probing.
//!
//! # Workflow
//!
//! 1. A [`RuntimeOracle`] is configured with a fixture (static expected
//!    values) and a probe command (e.g. a wrapper script that runs QEMU
//!    and captures register state as JSON).
//! 2. At test time, `check_reset` tries to run the probe. If the
//!    command is unavailable, it returns `Skip`. Otherwise it compares
//!    the captured state against the actual device state.
//! 3. The fixture serves as offline documentation; the probe provides
//!    runtime verification against the real QEMU behavior.
//!
//! # Probe command contract
//!
//! The probe command receives `--probe <mode>` where mode is:
//! - `reset` — print reset register state as JSON to stdout and exit 0
//! - `scenario <name>` — apply the named scenario and print state
//!
//! Output format (stdout):
//! ```json
//! {"registers": {"REG_NAME": value, ...}, "irqs": {"IRQ_NUM": true, ...}}
//! ```

use std::collections::BTreeMap;
use std::io::Read;
use std::process::{Command, Stdio};

/// A single register snapshot: register name → value.
pub type RegSnapshot = BTreeMap<String, u64>;

/// A function that applies a scenario to a device and returns the
/// resulting register snapshot and IRQ state.
pub type ScenarioApplier =
    dyn Fn(&OracleScenario) -> (RegSnapshot, BTreeMap<u32, bool>);

/// Expected behavior captured from a reference implementation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OracleFixture {
    /// Device type identifier (e.g. "ns16550", "goldfish_rtc").
    pub device: String,
    /// Default register values after reset.
    pub reset_regs: RegSnapshot,
    /// IRQ trigger scenarios: description → expected reg state.
    pub scenarios: Vec<OracleScenario>,
    /// Known quirks / intentional deviations from reference.
    #[serde(default)]
    pub quirks: Vec<OracleQuirk>,
}

/// One test scenario: apply these MMIO writes, then expect these
/// register values and IRQ levels.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OracleScenario {
    /// Human-readable description.
    pub name: String,
    /// Sequence of (offset, value, size) writes to apply.
    #[serde(default)]
    pub writes: Vec<(u64, u64, u8)>,
    /// Expected register state after writes.
    pub expected: RegSnapshot,
    /// Expected IRQ levels: IRQ number → asserted.
    #[serde(default)]
    pub irqs: BTreeMap<u32, bool>,
}

/// A documented intentional deviation from reference behavior.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OracleQuirk {
    /// Which register or behavior is affected.
    pub target: String,
    /// Why the deviation exists.
    pub reason: String,
    /// When this quirk was approved (ISO date).
    pub approved: String,
}

/// Oracle comparison result.
#[derive(Debug)]
pub struct OracleResult {
    /// Total registers checked.
    pub total: usize,
    /// Number of mismatches.
    pub mismatches: usize,
    /// Detailed mismatch info.
    pub details: Vec<OracleMismatch>,
}

/// Single register/IRQ mismatch.
#[derive(Debug)]
pub struct OracleMismatch {
    pub register: String,
    pub expected: u64,
    pub actual: u64,
}

/// Result of a runtime oracle check.
#[derive(Debug)]
pub enum OracleCheckResult {
    /// All values matched.
    Pass { total: usize },
    /// One or more values mismatched.
    Mismatch(OracleResult),
    /// Probe command unavailable — comparison skipped.
    Skip(String),
    /// Probe ran but failed (non-zero exit, bad JSON, etc.).
    Error(String),
}

/// Result of a single probe run.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ProbeOutput {
    registers: RegSnapshot,
    #[serde(default)]
    irqs: BTreeMap<u32, bool>,
}

/// The oracle engine — loads fixtures and compares device state.
pub struct Oracle {
    fixture: OracleFixture,
}

impl Oracle {
    /// Load an oracle fixture from a JSON byte slice.
    pub fn load(json: &[u8]) -> Result<Self, String> {
        let fixture: OracleFixture = serde_json::from_slice(json)
            .map_err(|e| format!("failed to parse oracle fixture: {e}"))?;
        Ok(Self { fixture })
    }

    /// Return the device name from the fixture.
    pub fn device(&self) -> &str {
        &self.fixture.device
    }

    /// Return a reference to the fixture.
    pub fn fixture(&self) -> &OracleFixture {
        &self.fixture
    }

    /// Compare `actual` register state against the fixture reset
    /// values, ignoring any registers listed in quirk targets.
    pub fn check_reset(&self, actual: &RegSnapshot) -> OracleResult {
        check_snapshot(
            &self.fixture.reset_regs,
            actual,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &self.fixture.quirks,
        )
    }

    /// Check all scenarios, returning a Vec of per-scenario results.
    pub fn check_scenarios(
        &self,
        _apply: &ScenarioApplier,
    ) -> Vec<OracleResult> {
        self.fixture
            .scenarios
            .iter()
            .map(|scenario| {
                let (actual_regs, actual_irqs) = _apply(scenario);
                check_snapshot(
                    &scenario.expected,
                    &actual_regs,
                    &actual_irqs,
                    &scenario.irqs,
                    &self.fixture.quirks,
                )
            })
            .collect()
    }
}

/// Runtime oracle that probes a reference implementation via CLI.
pub struct RuntimeOracle {
    oracle: Oracle,
    probe_cmd: String,
    probe_args: Vec<String>,
}

impl RuntimeOracle {
    /// Create a new runtime oracle.
    ///
    /// `probe_cmd` — path to the probe executable (e.g. a wrapper
    /// script that runs QEMU and captures register state).
    /// `probe_args` — extra arguments passed before `--probe`.
    pub fn new(
        fixture_json: &[u8],
        probe_cmd: impl Into<String>,
        probe_args: &[String],
    ) -> Result<Self, String> {
        let oracle = Oracle::load(fixture_json)?;
        Ok(Self {
            oracle,
            probe_cmd: probe_cmd.into(),
            probe_args: probe_args.to_vec(),
        })
    }

    /// Return the device name from the underlying fixture.
    pub fn device(&self) -> &str {
        self.oracle.device()
    }

    /// Run the probe command and capture its JSON output.
    ///
    /// Returns `Ok(ProbeOutput)` on success, `Err(msg)` on failure.
    /// The error starts with "NOT_FOUND:" only when the probe command
    /// is genuinely unavailable (`ErrorKind::NotFound`).
    fn run_probe(
        &self,
        mode: &str,
        scenario_name: Option<&str>,
    ) -> Result<ProbeOutput, String> {
        let mut cmd = Command::new(&self.probe_cmd);
        cmd.args(&self.probe_args);
        cmd.arg("--probe");
        cmd.arg(mode);
        if let Some(name) = scenario_name {
            cmd.arg(name);
        }
        cmd.stdin(Stdio::null());
        cmd.stderr(Stdio::piped());
        cmd.stdout(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "NOT_FOUND:cannot start probe '{}': {e}",
                    self.probe_cmd
                )
            } else {
                format!("cannot start probe '{}': {e}", self.probe_cmd)
            }
        })?;

        let mut stdout = Vec::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_end(&mut stdout)
            .map_err(|e| format!("read probe stdout: {e}"))?;

        let status = child.wait().map_err(|e| format!("wait probe: {e}"))?;

        if !status.success() {
            let mut stderr = String::new();
            if let Some(mut s) = child.stderr.take() {
                let _ = s.read_to_string(&mut stderr);
            }
            return Err(format!(
                "probe exited {}: {stderr}",
                status.code().unwrap_or(-1)
            ));
        }

        serde_json::from_slice::<ProbeOutput>(&stdout)
            .map_err(|e| format!("parse probe output: {e}"))
    }

    /// Probe the reference implementation for reset register values
    /// and compare against `actual` and `actual_irqs`.
    ///
    /// Returns `Skip` only when the probe command cannot be started
    /// (command not found). Returns `Error` for probe execution
    /// failures (non-zero exit, bad JSON). Returns `Pass` if all
    /// values match, or `Mismatch` with details.
    pub fn check_reset(
        &self,
        actual: &RegSnapshot,
        actual_irqs: &BTreeMap<u32, bool>,
    ) -> OracleCheckResult {
        let probe = match self.run_probe("reset", None) {
            Ok(p) => p,
            Err(e) => {
                if e.starts_with("NOT_FOUND:") {
                    return OracleCheckResult::Skip(e);
                }
                return OracleCheckResult::Error(e);
            }
        };

        let result = check_snapshot(
            &probe.registers,
            actual,
            actual_irqs,
            &probe.irqs,
            &self.oracle.fixture.quirks,
        );

        if result.mismatches == 0 {
            OracleCheckResult::Pass {
                total: result.total,
            }
        } else {
            OracleCheckResult::Mismatch(result)
        }
    }

    /// Probe the reference for each scenario and compare.
    pub fn check_scenarios(
        &self,
        _apply: &ScenarioApplier,
    ) -> Vec<OracleCheckResult> {
        self.oracle
            .fixture
            .scenarios
            .iter()
            .map(|scenario| {
                let probe =
                    match self.run_probe("scenario", Some(&scenario.name)) {
                        Ok(p) => p,
                        Err(e) => {
                            if e.starts_with("NOT_FOUND:") {
                                return OracleCheckResult::Skip(e);
                            }
                            return OracleCheckResult::Error(e);
                        }
                    };

                let (actual_regs, actual_irqs) = _apply(scenario);
                // Compare Machina output against QEMU probe output
                let result = check_snapshot(
                    &probe.registers,
                    &actual_regs,
                    &actual_irqs,
                    &probe.irqs,
                    &self.oracle.fixture.quirks,
                );

                if result.mismatches == 0 {
                    OracleCheckResult::Pass {
                        total: result.total,
                    }
                } else {
                    OracleCheckResult::Mismatch(result)
                }
            })
            .collect()
    }
}

/// Compare an expected snapshot against actual values, respecting
/// quirks.
fn check_snapshot(
    expected_regs: &RegSnapshot,
    actual_regs: &RegSnapshot,
    actual_irqs: &BTreeMap<u32, bool>,
    expected_irqs: &BTreeMap<u32, bool>,
    quirks: &[OracleQuirk],
) -> OracleResult {
    let quirk_targets: Vec<&str> =
        quirks.iter().map(|q| q.target.as_str()).collect();

    let mut result = OracleResult {
        total: 0,
        mismatches: 0,
        details: Vec::new(),
    };

    for (reg, &expected) in expected_regs {
        if quirk_targets.contains(&reg.as_str()) {
            continue;
        }
        result.total += 1;
        let Some(&actual_val) = actual_regs.get(reg) else {
            result.mismatches += 1;
            result.details.push(OracleMismatch {
                register: reg.clone(),
                expected,
                actual: 0,
            });
            continue;
        };
        if actual_val != expected {
            result.mismatches += 1;
            result.details.push(OracleMismatch {
                register: reg.clone(),
                expected,
                actual: actual_val,
            });
        }
    }

    for (&irq, &expected) in expected_irqs {
        let irq_key = format!("IRQ_{irq}");
        if quirk_targets.contains(&irq_key.as_str()) {
            continue;
        }
        result.total += 1;
        let Some(&actual_val) = actual_irqs.get(&irq) else {
            result.mismatches += 1;
            result.details.push(OracleMismatch {
                register: irq_key,
                expected: u64::from(expected),
                actual: 0,
            });
            continue;
        };
        if actual_val != expected {
            result.mismatches += 1;
            result.details.push(OracleMismatch {
                register: irq_key,
                expected: u64::from(expected),
                actual: u64::from(actual_val),
            });
        }
    }

    result
}
