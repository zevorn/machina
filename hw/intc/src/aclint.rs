// RISC-V ACLINT (Advanced Core Local Interruptor).
//
// CLINT-compatible single-window MMIO layout:
//   0x0000 + 4*hart : msip register (1 bit used)
//   0x4000 + 8*hart : mtimecmp[hart]
//   0xBFF8           : mtime

use machina_hw_core::irq::IrqLine;

// CLINT-compatible offsets within the single MMIO window.
const MSIP_BASE: u64 = 0x0000;
const MTIMECMP_BASE: u64 = 0x4000;
const MTIME_OFFSET: u64 = 0xBFF8;

pub struct Aclint {
    num_harts: u32,
    mtime: u64,
    mtimecmp: Vec<u64>,
    msip: Vec<u32>,
    /// Per-hart timer-interrupt pending flag, set by `tick`.
    timer_pending: Vec<bool>,
    mti_outputs: Vec<Option<IrqLine>>,
    msi_outputs: Vec<Option<IrqLine>>,
}

impl Aclint {
    pub fn new(num_harts: u32) -> Self {
        let mut mti = Vec::with_capacity(num_harts as usize);
        let mut msi = Vec::with_capacity(num_harts as usize);
        for _ in 0..num_harts {
            mti.push(None);
            msi.push(None);
        }
        Self {
            num_harts,
            mtime: 0,
            mtimecmp: vec![u64::MAX; num_harts as usize],
            msip: vec![0u32; num_harts as usize],
            timer_pending: vec![false; num_harts as usize],
            mti_outputs: mti,
            msi_outputs: msi,
        }
    }

    /// Connect an MTI output line for `hart`.
    pub fn connect_mti(&mut self, hart: u32, irq: IrqLine) {
        if (hart as usize) < self.mti_outputs.len() {
            self.mti_outputs[hart as usize] = Some(irq);
        }
    }

    /// Connect an MSI output line for `hart`.
    pub fn connect_msi(&mut self, hart: u32, irq: IrqLine) {
        if (hart as usize) < self.msi_outputs.len() {
            self.msi_outputs[hart as usize] = Some(irq);
        }
    }

    /// Increment mtime by 1 and check mtimecmp for each
    /// hart, updating `timer_pending` accordingly.
    pub fn tick(&mut self) {
        self.mtime = self.mtime.wrapping_add(1);
        for hart in 0..self.num_harts as usize {
            let pending = self.mtime >= self.mtimecmp[hart];
            self.timer_pending[hart] = pending;
            if let Some(ref line) = self.mti_outputs[hart] {
                line.set(pending);
            }
        }
    }

    /// Returns whether the timer interrupt is pending for
    /// `hart`.
    pub fn timer_irq_pending(&self, hart: u32) -> bool {
        if hart < self.num_harts {
            self.timer_pending[hart as usize]
        } else {
            false
        }
    }

    // ---- CLINT-compatible unified MMIO ----

    pub fn read(&self, offset: u64, _size: u32) -> u64 {
        if offset == MTIME_OFFSET {
            return self.mtime;
        }
        if offset >= MTIMECMP_BASE {
            // mtimecmp[hart] at 0x4000 + 8*hart.
            let hart = ((offset - MTIMECMP_BASE) / 8) as usize;
            if hart < self.num_harts as usize {
                return self.mtimecmp[hart];
            }
            return 0;
        }
        // msip[hart] at 0x0000 + 4*hart.
        let hart = ((offset - MSIP_BASE) / 4) as usize;
        if hart < self.num_harts as usize {
            self.msip[hart] as u64
        } else {
            0
        }
    }

    pub fn write(&mut self, offset: u64, _size: u32, val: u64) {
        if offset == MTIME_OFFSET {
            self.mtime = val;
            // Re-evaluate all harts after mtime change.
            for hart in 0..self.num_harts as usize {
                let pending = self.mtime >= self.mtimecmp[hart];
                self.timer_pending[hart] = pending;
                if let Some(ref line) = self.mti_outputs[hart] {
                    line.set(pending);
                }
            }
            return;
        }
        if offset >= MTIMECMP_BASE {
            // mtimecmp[hart] at 0x4000 + 8*hart.
            let hart = ((offset - MTIMECMP_BASE) / 8) as usize;
            if hart < self.num_harts as usize {
                self.mtimecmp[hart] = val;
                let pending = self.mtime >= self.mtimecmp[hart];
                self.timer_pending[hart] = pending;
                if let Some(ref line) = self.mti_outputs[hart] {
                    line.set(pending);
                }
            }
            return;
        }
        // msip[hart] at 0x0000 + 4*hart.
        let hart = ((offset - MSIP_BASE) / 4) as usize;
        if hart < self.num_harts as usize {
            // Only bit 0 is writable.
            let v = (val as u32) & 1;
            self.msip[hart] = v;
            if let Some(ref line) = self.msi_outputs[hart] {
                line.set(v != 0);
            }
        }
    }
}
