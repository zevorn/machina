//! Oracle dynamic comparison tool.
//!
//! The oracle framework captures guest-visible behavior (register
//! values, IRQ state) from QEMU and compares it against Machina
//! device models at runtime.  Oracle data lives in standalone JSON
//! fixture files — no QEMU file paths or commit hashes appear in
//! device source.
//!
//! # Workflow
//!
//! 1. Run QEMU with the target device, capture register / IRQ
//!    state into a JSON fixture (offline, one-time).
//! 2. Machina tests load the fixture and call [`Oracle::check`]
//!    to compare live device state against expected values.
//! 3. If QEMU is unavailable, oracle tests are skipped gracefully.

use std::collections::BTreeMap;

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

    /// Compare `actual` register state against the fixture reset
    /// values, ignoring any registers listed in quirk targets.
    pub fn check_reset(&self, actual: &RegSnapshot) -> OracleResult {
        let quirk_targets: Vec<&str> = self
            .fixture
            .quirks
            .iter()
            .map(|q| q.target.as_str())
            .collect();

        let mut result = OracleResult {
            total: 0,
            mismatches: 0,
            details: Vec::new(),
        };

        for (reg, &expected) in &self.fixture.reset_regs {
            if quirk_targets.contains(&reg.as_str()) {
                continue;
            }
            result.total += 1;
            let actual_val = actual.get(reg).copied().unwrap_or(0);
            if actual_val != expected {
                result.mismatches += 1;
                result.details.push(OracleMismatch {
                    register: reg.clone(),
                    expected,
                    actual: actual_val,
                });
            }
        }
        result
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
                let quirk_targets: Vec<&str> = self
                    .fixture
                    .quirks
                    .iter()
                    .map(|q| q.target.as_str())
                    .collect();

                let mut result = OracleResult {
                    total: 0,
                    mismatches: 0,
                    details: Vec::new(),
                };

                for (reg, &expected) in &scenario.expected {
                    if quirk_targets.contains(&reg.as_str()) {
                        continue;
                    }
                    result.total += 1;
                    let actual_val = actual_regs.get(reg).copied().unwrap_or(0);
                    if actual_val != expected {
                        result.mismatches += 1;
                        result.details.push(OracleMismatch {
                            register: reg.clone(),
                            expected,
                            actual: actual_val,
                        });
                    }
                }

                for (&irq, &expected) in &scenario.irqs {
                    if quirk_targets.contains(&irq.to_string().as_str()) {
                        continue;
                    }
                    result.total += 1;
                    let actual_val =
                        actual_irqs.get(&irq).copied().unwrap_or(false);
                    if actual_val != expected {
                        result.mismatches += 1;
                        result.details.push(OracleMismatch {
                            register: format!("IRQ_{irq}"),
                            expected: u64::from(expected),
                            actual: u64::from(actual_val),
                        });
                    }
                }

                result
            })
            .collect()
    }
}
