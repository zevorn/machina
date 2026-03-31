// RISC-V ACLINT (Advanced Core Local Interruptor).
//
// CLINT-compatible single-window MMIO layout:
//   0x0000 + 4*hart : msip register (1 bit used)
//   0x4000 + 8*hart : mtimecmp[hart]
//   0xBFF8           : mtime
//
// mtime is derived from the host monotonic clock at
// 10 MHz (1 tick = 100 ns). When mtimecmp is set to a
// future value, a timer thread sleeps until the deadline
// and then asserts MTI via the IRQ line.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use machina_core::wfi::WfiWaker;
use machina_hw_core::irq::IrqLine;

const MSIP_BASE: u64 = 0x0000;
const MTIMECMP_BASE: u64 = 0x4000;
const MTIME_OFFSET: u64 = 0xBFF8;

/// Shared timer state for the background timer thread.
/// Each hart has its own cancel token (AtomicU64) that
/// the main thread bumps to invalidate stale timers.
struct TimerState {
    cancel_gen: Vec<AtomicU64>,
}

pub struct Aclint {
    num_harts: u32,
    epoch: Instant,
    mtime_base: u64,
    mtimecmp: Vec<u64>,
    msip: Vec<u32>,
    mti_outputs: Vec<Option<IrqLine>>,
    msi_outputs: Vec<Option<IrqLine>>,
    wfi_waker: Option<Arc<WfiWaker>>,
    timer_state: Arc<TimerState>,
}

impl Aclint {
    pub fn new(num_harts: u32) -> Self {
        let n = num_harts as usize;
        let mut mti = Vec::with_capacity(n);
        let mut msi = Vec::with_capacity(n);
        let mut gens = Vec::with_capacity(n);
        for _ in 0..n {
            mti.push(None);
            msi.push(None);
            gens.push(AtomicU64::new(0));
        }
        Self {
            num_harts,
            epoch: Instant::now(),
            mtime_base: 0,
            mtimecmp: vec![u64::MAX; n],
            msip: vec![0u32; n],
            mti_outputs: mti,
            msi_outputs: msi,
            wfi_waker: None,
            timer_state: Arc::new(TimerState { cancel_gen: gens }),
        }
    }

    pub fn connect_mti(&mut self, hart: u32, irq: IrqLine) {
        if (hart as usize) < self.mti_outputs.len() {
            self.mti_outputs[hart as usize] = Some(irq);
        }
    }

    pub fn connect_msi(&mut self, hart: u32, irq: IrqLine) {
        if (hart as usize) < self.msi_outputs.len() {
            self.msi_outputs[hart as usize] = Some(irq);
        }
    }

    pub fn connect_wfi_waker(&mut self, wk: Arc<WfiWaker>) {
        self.wfi_waker = Some(wk);
    }

    /// Current mtime value derived from wall clock.
    pub fn read_mtime(&self) -> u64 {
        let elapsed = self.epoch.elapsed();
        let ticks = (elapsed.as_nanos() / 100) as u64;
        self.mtime_base.wrapping_add(ticks)
    }

    pub fn timer_irq_pending(&self, hart: u32) -> bool {
        if hart < self.num_harts {
            self.read_mtime() >= self.mtimecmp[hart as usize]
        } else {
            false
        }
    }

    /// Schedule a background timer for `hart` that will
    /// assert MTI when the wall clock reaches `mtimecmp`.
    fn schedule_timer(&self, hart: usize) {
        let cmp = self.mtimecmp[hart];
        if cmp == u64::MAX {
            return;
        }
        let now = self.read_mtime();
        if cmp <= now {
            // Already expired — MTI already set in the
            // synchronous write path.
            return;
        }
        let delta_ticks = cmp - now;
        let delta_ns = delta_ticks.saturating_mul(100).min(100_000_000_000);
        let delay = Duration::from_nanos(delta_ns);

        // Bump the cancel generation so any stale timer
        // for this hart is invalidated.
        let gen = self.timer_state.cancel_gen[hart]
            .fetch_add(1, Ordering::SeqCst)
            + 1;

        let line = match &self.mti_outputs[hart] {
            Some(l) => l.clone(),
            None => return,
        };
        let waker = self.wfi_waker.clone();
        let state = Arc::clone(&self.timer_state);

        std::thread::spawn(move || {
            std::thread::sleep(delay);
            // Check if this timer is still valid.
            let cur = state.cancel_gen[hart].load(Ordering::SeqCst);
            if cur != gen {
                return; // Cancelled by newer mtimecmp.
            }
            line.set(true);
            if let Some(ref wk) = waker {
                wk.wake();
            }
        });
    }

    /// Set the WFI condvar deadline for timer wakeup.
    fn update_wfi_deadline(&self) {
        if let Some(ref wk) = self.wfi_waker {
            let nearest =
                self.mtimecmp.iter().copied().min().unwrap_or(u64::MAX);
            if nearest == u64::MAX {
                wk.clear_deadline();
                return;
            }
            let now = self.read_mtime();
            if nearest <= now {
                return;
            }
            let delta_ticks = nearest - now;
            let delta_ns = delta_ticks.saturating_mul(100).min(100_000_000_000);
            let deadline = Instant::now() + Duration::from_nanos(delta_ns);
            wk.set_deadline(deadline);
        }
    }

    fn update_mti(&self) {
        let mtime = self.read_mtime();
        for hart in 0..self.num_harts as usize {
            let pending = mtime >= self.mtimecmp[hart];
            if let Some(ref line) = self.mti_outputs[hart] {
                line.set(pending);
            }
        }
    }

    pub fn read(&self, offset: u64, _size: u32) -> u64 {
        if offset == MTIME_OFFSET {
            return self.read_mtime();
        }
        if offset >= MTIMECMP_BASE {
            let hart = ((offset - MTIMECMP_BASE) / 8) as usize;
            if hart < self.num_harts as usize {
                return self.mtimecmp[hart];
            }
            return 0;
        }
        let hart = ((offset - MSIP_BASE) / 4) as usize;
        if hart < self.num_harts as usize {
            self.msip[hart] as u64
        } else {
            0
        }
    }

    pub fn write(&mut self, offset: u64, _size: u32, val: u64) {
        if offset == MTIME_OFFSET {
            // Invalidate all pending timer threads.
            for gen in &self.timer_state.cancel_gen {
                gen.fetch_add(1, Ordering::SeqCst);
            }
            self.mtime_base = val;
            self.epoch = Instant::now();
            self.update_mti();
            return;
        }
        if offset >= MTIMECMP_BASE {
            let hart = ((offset - MTIMECMP_BASE) / 8) as usize;
            if hart < self.num_harts as usize {
                // Invalidate any pending timer thread
                // for this hart before updating state.
                self.timer_state.cancel_gen[hart]
                    .fetch_add(1, Ordering::SeqCst);

                self.mtimecmp[hart] = val;
                let pending = self.read_mtime() >= self.mtimecmp[hart];
                if let Some(ref line) = self.mti_outputs[hart] {
                    line.set(pending);
                }
                if !pending {
                    self.schedule_timer(hart);
                }
                self.update_wfi_deadline();
            }
            return;
        }
        let hart = ((offset - MSIP_BASE) / 4) as usize;
        if hart < self.num_harts as usize {
            let v = (val as u32) & 1;
            self.msip[hart] = v;
            if let Some(ref line) = self.msi_outputs[hart] {
                line.set(v != 0);
            }
        }
    }
}
