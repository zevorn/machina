// RISC-V ACLINT (Advanced Core Local Interruptor).
//
// CLINT-compatible single-window MMIO layout:
//   0x0000 + 4*hart : msip register (1 bit used)
//   0x4000 + 8*hart : mtimecmp[hart]
//   0xBFF8           : mtime
//
// mtime is derived from the host monotonic clock at
// 10 MHz (1 tick = 100 ns).

use std::sync::Arc;
use std::time::Instant;

use machina_core::wfi::WfiWaker;
use machina_hw_core::irq::IrqLine;

const MSIP_BASE: u64 = 0x0000;
const MTIMECMP_BASE: u64 = 0x4000;
const MTIME_OFFSET: u64 = 0xBFF8;

pub struct Aclint {
    num_harts: u32,
    /// Host time corresponding to mtime_base.
    epoch: Instant,
    /// mtime value at the epoch instant.
    mtime_base: u64,
    mtimecmp: Vec<u64>,
    msip: Vec<u32>,
    mti_outputs: Vec<Option<IrqLine>>,
    msi_outputs: Vec<Option<IrqLine>>,
    wfi_waker: Option<Arc<WfiWaker>>,
}

impl Aclint {
    pub fn new(num_harts: u32) -> Self {
        let n = num_harts as usize;
        let mut mti = Vec::with_capacity(n);
        let mut msi = Vec::with_capacity(n);
        for _ in 0..n {
            mti.push(None);
            msi.push(None);
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

    /// Connect a WFI waker for timer-driven wakeup.
    pub fn connect_wfi_waker(&mut self, wk: Arc<WfiWaker>) {
        self.wfi_waker = Some(wk);
    }

    /// Compute and set the WFI deadline from the nearest
    /// mtimecmp across all harts.
    fn update_wfi_deadline(&self) {
        if let Some(ref wk) = self.wfi_waker {
            let nearest =
                self.mtimecmp.iter().copied().min().unwrap_or(u64::MAX);
            let now = self.read_mtime();
            if nearest <= now {
                // Already past — wake immediately.
                wk.wake();
            } else {
                let delta_ticks = nearest - now;
                let delta_ns = delta_ticks * 100;
                let deadline =
                    Instant::now() + std::time::Duration::from_nanos(delta_ns);
                wk.set_deadline(deadline);
                wk.wake(); // re-evaluate ongoing wait
            }
        }
    }

    /// Current mtime value derived from wall clock.
    pub fn read_mtime(&self) -> u64 {
        let elapsed = self.epoch.elapsed();
        let ticks = (elapsed.as_nanos() / 100) as u64;
        self.mtime_base.wrapping_add(ticks)
    }

    /// Returns whether mtime >= mtimecmp[hart].
    pub fn timer_irq_pending(&self, hart: u32) -> bool {
        if hart < self.num_harts {
            self.read_mtime() >= self.mtimecmp[hart as usize]
        } else {
            false
        }
    }

    /// Update MTI output lines based on current mtime.
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
            // Reset epoch so read_mtime() returns val.
            self.mtime_base = val;
            self.epoch = Instant::now();
            self.update_mti();
            return;
        }
        if offset >= MTIMECMP_BASE {
            let hart = ((offset - MTIMECMP_BASE) / 8) as usize;
            if hart < self.num_harts as usize {
                self.mtimecmp[hart] = val;
                let pending = self.read_mtime() >= self.mtimecmp[hart];
                if let Some(ref line) = self.mti_outputs[hart] {
                    line.set(pending);
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
