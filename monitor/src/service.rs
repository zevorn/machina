// MonitorService: shared backend for MMP and HMP.

use std::sync::Arc;

use machina_core::monitor::{CpuSnapshot, MonitorState, VmState};

/// Central monitor service shared by all transports.
pub struct MonitorService {
    pub state: Arc<MonitorState>,
    /// Total guest RAM in bytes, populated by `main.rs` after CLI
    /// parsing and read by `info memory` / `query-memory`. `0` until
    /// it is explicitly set, which is the case in unit tests that do
    /// not exercise memory-aware commands.
    ram_size_bytes: u64,
}

impl MonitorService {
    pub fn new(state: Arc<MonitorState>) -> Self {
        Self {
            state,
            ram_size_bytes: 0,
        }
    }

    /// Record the guest RAM size in bytes so the monitor can report
    /// it through `info memory`.
    pub fn set_ram_size(&mut self, bytes: u64) {
        self.ram_size_bytes = bytes;
    }

    /// Guest RAM size in bytes as last set by `set_ram_size`.
    pub fn ram_size(&self) -> u64 {
        self.ram_size_bytes
    }

    pub fn query_status(&self) -> bool {
        // Only report paused when actually parked.
        let s = self.state.vm_state();
        s == VmState::Running || s == VmState::PauseRequested
    }

    pub fn stop(&self) {
        self.state.request_stop();
    }

    pub fn cont(&self) {
        self.state.request_cont();
    }

    pub fn quit(&self) {
        self.state.request_quit();
    }

    pub fn query_cpus(&self) -> Vec<CpuInfo> {
        let running = self.query_status();
        let snap = self.state.read_snapshot();
        vec![CpuInfo {
            cpu_index: 0,
            // PC is only accurate when paused.
            pc: if running {
                0
            } else {
                snap.as_ref().map(|s| s.pc).unwrap_or(0)
            },
            halted: if running {
                false
            } else {
                snap.as_ref().map(|s| s.halted).unwrap_or(false)
            },
            arch: "riscv64".to_string(),
        }]
    }

    pub fn take_snapshot(&self) -> Option<CpuSnapshot> {
        self.state.read_snapshot()
    }
}

pub struct CpuInfo {
    pub cpu_index: u32,
    pub pc: u64,
    pub halted: bool,
    pub arch: String,
}
