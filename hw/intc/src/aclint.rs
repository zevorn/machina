// RISC-V ACLINT (Advanced Core Local Interruptor).
//
// CLINT-compatible single-window MMIO layout:
//   0x0000 + 4*hart : msip register (1 bit used)
//   0x4000 + 8*hart : mtimecmp[hart]
//   0xBFF8           : mtime
//
// mtime is derived from the host monotonic clock at
// 10 MHz (1 tick = 100 ns). Each hart uses at most one
// timer worker so frequent mtimecmp retargeting does not
// create an unbounded number of sleeping host threads.
//
// Interior mutability: register state is in
// DeviceRegs<AclintRegs>, setup state in
// parking_lot::Mutex<SysBusDeviceState>.  All public
// methods take &self so the device can be shared via
// Arc<Aclint> without an outer Mutex.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use machina_core::device_cell::DeviceRegs;
use machina_core::wfi::WfiWaker;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::IrqLine;
use machina_memory::region::MmioOps;

const MSIP_BASE: u64 = 0x0000;
const MTIMECMP_BASE: u64 = 0x4000;
const MTIME_OFFSET: u64 = 0xBFF8;
const TIMER_WORKER_POLL: Duration = Duration::from_millis(1);
const TIMER_DISABLED: u64 = u64::MAX;

type ExitRequest = Arc<dyn Fn() + Send + Sync>;

/// Shared timer state for the background timer thread.
struct TimerState {
    worker_active: Vec<AtomicBool>,
    deadline_ns: Vec<AtomicU64>,
    epoch: Instant,
}

/// Mutable register state protected by DeviceRegs.
struct AclintRegs {
    epoch: Instant,
    mtime_base: u64,
    mtimecmp: Vec<u64>,
    msip: Vec<u32>,
}

fn timer_epoch_ns(epoch: Instant) -> u64 {
    let ns = epoch.elapsed().as_nanos();
    ns.min(u128::from(u64::MAX)) as u64
}

fn timer_worker_loop(
    hart: usize,
    state: Arc<TimerState>,
    line: IrqLine,
    waker: Option<Arc<WfiWaker>>,
    exit_request: Option<ExitRequest>,
) {
    loop {
        let deadline = state.deadline_ns[hart].load(Ordering::Acquire);
        if deadline == TIMER_DISABLED {
            state.worker_active[hart].store(false, Ordering::Release);
            if state.deadline_ns[hart].load(Ordering::Acquire) == TIMER_DISABLED
            {
                return;
            }
            if state.worker_active[hart]
                .compare_exchange(
                    false,
                    true,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                return;
            }
            continue;
        }

        let now = timer_epoch_ns(state.epoch);
        if now >= deadline {
            if state.deadline_ns[hart]
                .compare_exchange(
                    deadline,
                    TIMER_DISABLED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                line.set(true);
                if let Some(ref request) = exit_request {
                    request();
                }
                if let Some(ref wk) = waker {
                    wk.wake();
                }
            }
            continue;
        }

        let sleep_ns = deadline
            .saturating_sub(now)
            .min(TIMER_WORKER_POLL.as_nanos() as u64);
        std::thread::sleep(Duration::from_nanos(sleep_ns));
    }
}

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot", before_unrealize = [cancel_timers, lower_outputs, clear_wfi_deadline])]
pub struct Aclint {
    // Setup-only state behind parking_lot::Mutex so that
    // attach_to_bus / register_mmio / realize_onto can be
    // called through &self (Arc<Aclint>).
    state: parking_lot::Mutex<SysBusDeviceState>,
    num_harts: u32,
    // Runtime register state.
    regs: DeviceRegs<AclintRegs>,
    // Output lines. Written only during init (behind
    // parking_lot::Mutex), read at runtime via the lock.
    mti_outputs: parking_lot::Mutex<Vec<Option<IrqLine>>>,
    msi_outputs: parking_lot::Mutex<Vec<Option<IrqLine>>>,
    wfi_waker: parking_lot::Mutex<Option<Arc<WfiWaker>>>,
    timer_state: Arc<TimerState>,
    virtual_clock: AtomicBool,
    /// Per-hart callback used by the timer thread to break
    /// goto_tb chains so the exec loop can deliver timer IRQs.
    exit_requests: parking_lot::Mutex<Vec<Option<ExitRequest>>>,
}

impl Aclint {
    pub fn new(num_harts: u32) -> Self {
        Self::new_named("aclint", num_harts)
    }

    pub fn new_named(local_id: &str, num_harts: u32) -> Self {
        let n = num_harts as usize;
        let mut mti = Vec::with_capacity(n);
        let mut msi = Vec::with_capacity(n);
        let mut workers = Vec::with_capacity(n);
        let mut deadlines = Vec::with_capacity(n);
        for _ in 0..n {
            mti.push(None);
            msi.push(None);
            workers.push(AtomicBool::new(false));
            deadlines.push(AtomicU64::new(TIMER_DISABLED));
        }
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            num_harts,
            regs: DeviceRegs::new(AclintRegs {
                epoch: Instant::now(),
                mtime_base: 0,
                mtimecmp: vec![u64::MAX; n],
                msip: vec![0u32; n],
            }),
            mti_outputs: parking_lot::Mutex::new(mti),
            msi_outputs: parking_lot::Mutex::new(msi),
            wfi_waker: parking_lot::Mutex::new(None),
            timer_state: Arc::new(TimerState {
                worker_active: workers,
                deadline_ns: deadlines,
                epoch: Instant::now(),
            }),
            virtual_clock: AtomicBool::new(false),
            exit_requests: parking_lot::Mutex::new(vec![None; n]),
        }
    }

    fn cancel_timer(&self, hart: usize) {
        self.timer_state.deadline_ns[hart]
            .store(TIMER_DISABLED, Ordering::Release);
    }

    pub fn connect_mti(&self, hart: u32, irq: IrqLine) {
        let mut outputs = self.mti_outputs.lock();
        if (hart as usize) < outputs.len() {
            outputs[hart as usize] = Some(irq);
        }
    }

    pub fn connect_msi(&self, hart: u32, irq: IrqLine) {
        let mut outputs = self.msi_outputs.lock();
        if (hart as usize) < outputs.len() {
            outputs[hart as usize] = Some(irq);
        }
    }

    /// Register an exit-request callback for a hart so timer
    /// interrupts can break goto_tb chains.
    pub fn connect_exit_request(&self, hart: u32, request: ExitRequest) {
        let mut requests = self.exit_requests.lock();
        if (hart as usize) < requests.len() {
            requests[hart as usize] = Some(request);
        }
    }

    pub fn connect_wfi_waker(&self, wk: Arc<WfiWaker>) {
        *self.wfi_waker.lock() = Some(wk);
    }

    pub fn set_virtual_clock(&self, enabled: bool) {
        self.virtual_clock.store(enabled, Ordering::SeqCst);
        self.cancel_timers();
        self.update_mti();
    }

    pub fn tick(&self, ticks: u64) {
        if ticks == 0 || !self.virtual_clock.load(Ordering::Relaxed) {
            return;
        }
        {
            let mut regs = self.regs.borrow();
            regs.mtime_base = regs.mtime_base.wrapping_add(ticks);
        }
        self.update_mti();
    }

    pub fn reset_runtime(&self) {
        self.cancel_timers();
        {
            let mut regs = self.regs.borrow();
            regs.epoch = Instant::now();
            regs.mtime_base = 0;
            regs.mtimecmp.fill(u64::MAX);
            regs.msip.fill(0);
        }
        self.lower_outputs();
        {
            let wk = self.wfi_waker.lock();
            if let Some(ref w) = *wk {
                w.clear_deadline();
            }
        }
    }

    /// Set mtimecmp[hart] directly (for SBI SET_TIMER).
    ///
    /// Equivalent to a guest write at ACLINT_BASE + 0x4000 +
    /// hart * 8; used by the host SBI backend in builtin mode
    /// so it does not need to go through MMIO decode.
    pub fn set_mtimecmp(&self, hart: usize, val: u64) {
        const MTIMECMP_OFF: u64 = 0x4000;
        self.write(MTIMECMP_OFF + hart as u64 * 8, 8, val);
    }

    /// Current mtime value derived from wall clock.
    pub fn read_mtime(&self) -> u64 {
        let regs = self.regs.borrow();
        self.read_mtime_live(&regs)
    }

    /// Read mtime from an already-borrowed regs guard.
    fn read_mtime_with(regs: &AclintRegs) -> u64 {
        let elapsed = regs.epoch.elapsed();
        let ticks = (elapsed.as_nanos() / 100) as u64;
        regs.mtime_base.wrapping_add(ticks)
    }

    fn read_mtime_live(&self, regs: &AclintRegs) -> u64 {
        if self.virtual_clock.load(Ordering::Relaxed) {
            regs.mtime_base
        } else {
            Self::read_mtime_with(regs)
        }
    }

    pub fn timer_irq_pending(&self, hart: u32) -> bool {
        if hart < self.num_harts {
            let regs = self.regs.borrow();
            self.read_mtime_live(&regs) >= regs.mtimecmp[hart as usize]
        } else {
            false
        }
    }

    /// Schedule a background timer for `hart` that will
    /// assert MTI when the wall clock reaches `mtimecmp`.
    fn schedule_timer(&self, hart: usize) {
        if self.virtual_clock.load(Ordering::Relaxed) {
            return;
        }
        // Read values under regs lock.
        let (cmp, now) = {
            let regs = self.regs.borrow();
            (regs.mtimecmp[hart], self.read_mtime_live(&regs))
        };
        if cmp == u64::MAX {
            return;
        }
        if cmp <= now {
            return;
        }
        let delta_ticks = cmp - now;

        let line = {
            let outputs = self.mti_outputs.lock();
            match &outputs[hart] {
                Some(l) => l.clone(),
                None => return,
            }
        };
        let waker = self.wfi_waker.lock().clone();
        let state = Arc::clone(&self.timer_state);
        let exit_request = self.exit_requests.lock()[hart].clone();

        let delta_ns = delta_ticks.saturating_mul(100).min(100_000_000_000);
        let deadline = timer_epoch_ns(state.epoch).saturating_add(delta_ns);
        state.deadline_ns[hart].store(deadline, Ordering::Release);

        if state.worker_active[hart]
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let worker_state = Arc::clone(&state);
        let spawn = std::thread::Builder::new()
            .name(format!("aclint-timer-{hart}"))
            .spawn(move || {
                timer_worker_loop(
                    hart,
                    worker_state,
                    line,
                    waker,
                    exit_request,
                );
            });
        if spawn.is_err() {
            state.worker_active[hart].store(false, Ordering::Release);
        }
    }

    /// Set the WFI condvar deadline for timer wakeup.
    fn update_wfi_deadline(&self) {
        let wk = self.wfi_waker.lock();
        if let Some(ref w) = *wk {
            let regs = self.regs.borrow();
            let nearest =
                regs.mtimecmp.iter().copied().min().unwrap_or(u64::MAX);
            if nearest == u64::MAX {
                w.clear_deadline();
                return;
            }
            let now = self.read_mtime_live(&regs);
            if nearest <= now {
                return;
            }
            let delta_ticks = nearest - now;
            let delta_ns = delta_ticks.saturating_mul(100).min(100_000_000_000);
            let deadline = Instant::now() + Duration::from_nanos(delta_ns);
            w.set_deadline(deadline);
        }
    }

    fn update_mti(&self) {
        let pending = {
            let regs = self.regs.borrow();
            let mtime = self.read_mtime_live(&regs);
            (0..self.num_harts as usize)
                .map(|hart| mtime >= regs.mtimecmp[hart])
                .collect::<Vec<_>>()
        };
        let outputs = self.mti_outputs.lock();
        for hart in 0..self.num_harts as usize {
            if let Some(ref line) = outputs[hart] {
                line.set(pending[hart]);
            }
        }
        drop(outputs);

        let requests = self.exit_requests.lock();
        for hart in 0..self.num_harts as usize {
            if pending[hart] {
                if let Some(ref request) = requests[hart] {
                    request();
                }
            }
        }
        if pending.iter().any(|p| *p) {
            if let Some(ref wk) = *self.wfi_waker.lock() {
                wk.wake();
            }
        }
    }

    /// Cancel all pending timer deadlines.  After this call
    /// any sleeping timer thread that wakes up will find its
    /// stored deadline stale and return without requesting an
    /// exec-loop exit.
    ///
    /// Must be called before CPU teardown so stale timer
    /// callbacks cannot target a dropped runtime.
    pub fn cancel_timers(&self) {
        for hart in 0..self.num_harts as usize {
            self.cancel_timer(hart);
        }
    }

    fn lower_outputs(&self) {
        let mti = self.mti_outputs.lock();
        for line in mti.iter().flatten() {
            line.lower();
        }
        let msi = self.msi_outputs.lock();
        for line in msi.iter().flatten() {
            line.lower();
        }
    }

    fn clear_wfi_deadline(&self) {
        let wk = self.wfi_waker.lock();
        if let Some(ref w) = *wk {
            w.clear_deadline();
        }
    }

    pub fn read(&self, offset: u64, size: u32) -> u64 {
        if offset == MTIME_OFFSET {
            if size != 4 && size != 8 {
                return 0;
            }
            let mtime = self.read_mtime();
            return if size == 4 {
                mtime & 0xFFFF_FFFF
            } else {
                mtime
            };
        }
        if offset == MTIME_OFFSET + 4 && size == 4 {
            return (self.read_mtime() >> 32) & 0xFFFF_FFFF;
        }
        let regs = self.regs.borrow();
        if offset >= MTIMECMP_BASE {
            let sub = (offset - MTIMECMP_BASE) % 8;
            let hart = ((offset - MTIMECMP_BASE) / 8) as usize;
            if hart < self.num_harts as usize {
                let val = regs.mtimecmp[hart];
                return if sub == 0 && size == 4 {
                    val & 0xFFFF_FFFF
                } else if sub == 0 && size == 8 {
                    val
                } else if sub == 4 && size == 4 {
                    (val >> 32) & 0xFFFF_FFFF
                } else {
                    0
                };
            }
            return 0;
        }
        let hart = ((offset - MSIP_BASE) / 4) as usize;
        if size == 4
            && offset.is_multiple_of(4)
            && hart < self.num_harts as usize
        {
            regs.msip[hart] as u64
        } else {
            0
        }
    }

    pub fn write(&self, offset: u64, size: u32, val: u64) {
        if offset == MTIME_OFFSET {
            if size != 4 && size != 8 {
                return;
            }
            self.cancel_timers();
            {
                let mut regs = self.regs.borrow();
                if size == 4 {
                    // Preserve the high half of the live mtime,
                    // not the stored mtime_base.
                    let cur = self.read_mtime_live(&regs);
                    regs.mtime_base =
                        (cur & 0xFFFF_FFFF_0000_0000) | (val & 0xFFFF_FFFF);
                } else {
                    regs.mtime_base = val;
                }
                regs.epoch = Instant::now();
            }
            self.update_mti();
            return;
        }
        if offset == MTIME_OFFSET + 4 {
            if size != 4 {
                return;
            }
            self.cancel_timers();
            {
                let mut regs = self.regs.borrow();
                let cur = self.read_mtime_live(&regs);
                regs.mtime_base =
                    (cur & 0x0000_0000_FFFF_FFFF) | ((val & 0xFFFF_FFFF) << 32);
                regs.epoch = Instant::now();
            }
            self.update_mti();
            return;
        }
        if offset >= MTIMECMP_BASE {
            let sub = (offset - MTIMECMP_BASE) % 8;
            let hart = ((offset - MTIMECMP_BASE) / 8) as usize;
            if hart < self.num_harts as usize {
                let old = self.regs.borrow().mtimecmp[hart];
                let new_val = if sub == 0 && size == 4 {
                    (old & 0xFFFF_FFFF_0000_0000) | (val & 0xFFFF_FFFF)
                } else if sub == 0 && size == 8 {
                    val
                } else if sub == 4 && size == 4 {
                    (old & 0x0000_0000_FFFF_FFFF) | ((val & 0xFFFF_FFFF) << 32)
                } else {
                    return;
                };

                self.cancel_timer(hart);

                let pending = {
                    let mut regs = self.regs.borrow();
                    regs.mtimecmp[hart] = new_val;
                    self.read_mtime_live(&regs) >= regs.mtimecmp[hart]
                };
                {
                    let outputs = self.mti_outputs.lock();
                    if let Some(ref line) = outputs[hart] {
                        line.set(pending);
                    }
                }
                if pending {
                    {
                        let requests = self.exit_requests.lock();
                        if let Some(ref request) = requests[hart] {
                            request();
                        }
                    }
                    if let Some(ref wk) = *self.wfi_waker.lock() {
                        wk.wake();
                    }
                }
                if !pending {
                    self.schedule_timer(hart);
                }
                self.update_wfi_deadline();
            }
            return;
        }
        let hart = ((offset - MSIP_BASE) / 4) as usize;
        if size == 4
            && offset.is_multiple_of(4)
            && hart < self.num_harts as usize
        {
            let v = (val as u32) & 1;
            self.regs.borrow().msip[hart] = v;
            let outputs = self.msi_outputs.lock();
            if let Some(ref line) = outputs[hart] {
                line.set(v != 0);
            }
        }
    }
}

pub struct AclintMmio(pub Arc<Aclint>);

impl MmioOps for AclintMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.write(offset, size, val);
    }
}
