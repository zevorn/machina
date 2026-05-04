// machina-system: CPU management and GuestCpu bridge.

pub mod builtin;
pub mod cpus;
pub mod gdb;
pub mod gdb_csr;
pub mod loongarch_cpu;

pub use builtin::FirmwareCallFn;
pub use cpus::FullSystemCpu;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use machina_accel::exec::exec_loop::{cpu_exec_loop, ExitReason};
use machina_accel::exec::{PerCpuState, SharedState};
use machina_accel::GuestCpu;
use machina_accel::HostCodeGen;
use machina_core::wfi::WfiWaker;

use crate::loongarch_cpu::SharedLoongArchFullSystemCpu;

enum ManagedCpu {
    Riscv(Box<FullSystemCpu>),
    LoongArch(SharedLoongArchFullSystemCpu),
}

pub struct CpuManager {
    running: Arc<AtomicBool>,
    wfi_waker: Option<Arc<WfiWaker>>,
    cpus: Vec<ManagedCpu>,
    /// Optional firmware call handler for builtin mode.
    /// When set, S-mode ecalls are dispatched here instead
    /// of being delivered as CPU trap exceptions.
    firmware_handler: Option<FirmwareCallFn>,
}

impl CpuManager {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(true)),
            wfi_waker: None,
            cpus: Vec::new(),
            firmware_handler: None,
        }
    }

    /// Install a firmware call handler for builtin mode.
    /// S-mode ecalls will be dispatched to this handler
    /// instead of being raised as exceptions.
    pub fn set_firmware_handler(&mut self, handler: FirmwareCallFn) {
        self.firmware_handler = Some(handler);
    }

    pub fn set_wfi_waker(&mut self, wk: Arc<WfiWaker>) {
        self.wfi_waker = Some(wk);
    }

    /// Get a clone of the running flag for external stop.
    pub fn running_flag(&self) -> Arc<AtomicBool> {
        self.running.clone()
    }

    /// Add a CPU to be managed. Ownership is transferred.
    pub fn add_cpu(&mut self, cpu: FullSystemCpu) {
        self.cpus.push(ManagedCpu::Riscv(Box::new(cpu)));
    }

    /// Add a LoongArch CPU to be managed. Ownership is transferred.
    pub fn add_loongarch_cpu(&mut self, cpu: SharedLoongArchFullSystemCpu) {
        self.cpus.push(ManagedCpu::LoongArch(cpu));
    }

    /// Access a managed CPU by index.
    pub fn cpu(&self, idx: usize) -> &FullSystemCpu {
        match &self.cpus[idx] {
            ManagedCpu::Riscv(cpu) => cpu.as_ref(),
            ManagedCpu::LoongArch(_) => {
                panic!("managed CPU {idx} is not a RISC-V CPU")
            }
        }
    }

    /// Access a managed CPU mutably by index.
    pub fn cpu_mut(&mut self, idx: usize) -> &mut FullSystemCpu {
        match &mut self.cpus[idx] {
            ManagedCpu::Riscv(cpu) => cpu.as_mut(),
            ManagedCpu::LoongArch(_) => {
                panic!("managed CPU {idx} is not a RISC-V CPU")
            }
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(ref wk) = self.wfi_waker {
            wk.stop();
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Run all owned CPUs. For single-CPU, runs on the
    /// current thread. Blocks until execution exits.
    ///
    /// When GDB is active, after the exec loop pauses,
    /// this method hands control to the GDB server to
    /// process commands synchronously on this thread.
    ///
    /// # Safety
    /// Each CPU's `env_ptr()` must return a valid pointer
    /// to its RiscvCpu struct matching translation globals.
    pub unsafe fn run<B>(&mut self, shared: &SharedState<B>) -> ExitReason
    where
        B: HostCodeGen,
    {
        if self.cpus.is_empty() {
            return ExitReason::Exit(0);
        }
        let running = Arc::clone(&self.running);
        let firmware_handler = self.firmware_handler.clone();
        let cpu = &mut self.cpus[0];
        match cpu {
            ManagedCpu::Riscv(cpu) => Self::run_riscv_cpu(
                shared,
                &running,
                firmware_handler.as_ref(),
                cpu.as_mut(),
            ),
            ManagedCpu::LoongArch(cpu) => {
                Self::run_loongarch_cpu(shared, &running, cpu)
            }
        }
    }

    unsafe fn run_riscv_cpu<B>(
        shared: &SharedState<B>,
        running: &Arc<AtomicBool>,
        firmware_handler: Option<&FirmwareCallFn>,
        cpu: &mut FullSystemCpu,
    ) -> ExitReason
    where
        B: HostCodeGen,
    {
        let mut per_cpu = PerCpuState::new();
        loop {
            let r = cpu_exec_loop(shared, &mut per_cpu, cpu);
            if !running.load(Ordering::SeqCst) {
                return ExitReason::Halted;
            }
            match r {
                ExitReason::Halted => {}
                ExitReason::BufferFull => {
                    let _guard = shared.translate_lock.lock().unwrap();
                    shared
                        .tb_store
                        .invalidate_all(shared.code_buf(), &shared.backend);
                    shared.tb_store.flush();
                    shared.code_buf_mut().set_offset(shared.code_gen_start);
                    per_cpu.jump_cache.invalidate();
                }
                ExitReason::Ecall { priv_level } => {
                    // In builtin mode, S-mode ecalls (priv 1)
                    // are dispatched to the host firmware
                    // handler instead of being raised as traps.
                    if priv_level == 1 {
                        if let Some(fw) = firmware_handler {
                            fw(&mut cpu.cpu);
                            if !running.load(Ordering::Relaxed) {
                                return ExitReason::Halted;
                            }
                            continue;
                        }
                    }
                    // Route ECALL as trap exception.
                    // 8=EcallFromU, 9=EcallFromS,
                    // 11=EcallFromM.
                    let cause = match priv_level {
                        0 => 8,
                        1 => 9,
                        3 => 11,
                        _ => 11,
                    };
                    cpu.handle_exception(cause, 0);
                }
                other => return other,
            }
        }
    }

    unsafe fn run_loongarch_cpu<B>(
        shared: &SharedState<B>,
        running: &Arc<AtomicBool>,
        cpu: &mut SharedLoongArchFullSystemCpu,
    ) -> ExitReason
    where
        B: HostCodeGen,
    {
        let mut per_cpu = PerCpuState::new();
        loop {
            let r = cpu_exec_loop(shared, &mut per_cpu, cpu);
            if !running.load(Ordering::SeqCst) {
                return ExitReason::Halted;
            }
            match r {
                ExitReason::BufferFull => {
                    let _guard = shared.translate_lock.lock().unwrap();
                    shared
                        .tb_store
                        .invalidate_all(shared.code_buf(), &shared.backend);
                    shared.tb_store.flush();
                    shared.code_buf_mut().set_offset(shared.code_gen_start);
                    per_cpu.jump_cache.invalidate();
                }
                other => return other,
            }
        }
    }
}

impl Default for CpuManager {
    fn default() -> Self {
        Self::new()
    }
}
