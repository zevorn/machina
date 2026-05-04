const NUM_IRQS: usize = 256;
const NUM_U32: usize = NUM_IRQS / 32;

pub struct Eiointc {
    enable: [u32; NUM_U32],
    isr: [u32; NUM_U32],
    coremap: [u8; NUM_IRQS],
    ipmap: [u8; 8],
    bounce: [u32; NUM_U32],
}

impl Eiointc {
    #[must_use]
    pub fn new() -> Self {
        Self {
            enable: [0; NUM_U32],
            isr: [0; NUM_U32],
            coremap: [0; NUM_IRQS],
            ipmap: [0; 8],
            bounce: [0; NUM_U32],
        }
    }

    pub fn set_irq(&mut self, irq: u32, level: bool) {
        if irq >= NUM_IRQS as u32 {
            return;
        }
        let idx = (irq / 32) as usize;
        let bit = 1u32 << (irq % 32);
        if level {
            self.isr[idx] |= bit;
        } else {
            self.isr[idx] &= !bit;
        }
    }

    pub fn ack(&mut self, irq: u32) {
        if irq >= NUM_IRQS as u32 {
            return;
        }
        let idx = (irq / 32) as usize;
        let bit = 1u32 << (irq % 32);
        self.isr[idx] &= !bit;
    }

    #[must_use]
    pub fn pending_for_cpu(&self, cpu_id: u32) -> u8 {
        let mut hwi_bits: u8 = 0;
        for irq in 0..NUM_IRQS {
            let idx = irq / 32;
            let bit = 1u32 << (irq % 32);
            if self.isr[idx] & self.enable[idx] & bit == 0 {
                continue;
            }
            if self.coremap[irq] as u32 != cpu_id {
                continue;
            }
            let group = irq / 32;
            let hwi = self.ipmap.get(group).copied().unwrap_or(0) & 0x7;
            hwi_bits |= 1 << hwi;
        }
        hwi_bits
    }

    pub fn mmio_read(&self, offset: u64) -> u64 {
        match offset {
            0x00C0..=0x00C7 => {
                let mut val = 0u64;
                for i in 0..8 {
                    val |= u64::from(self.ipmap[i]) << (i * 8);
                }
                val
            }
            0x0200..=0x021F => {
                let idx = ((offset - 0x0200) / 4) as usize;
                if idx < NUM_U32 {
                    u64::from(self.enable[idx])
                } else {
                    0
                }
            }
            0x0280..=0x029F => {
                let idx = ((offset - 0x0280) / 4) as usize;
                if idx < NUM_U32 {
                    u64::from(self.bounce[idx])
                } else {
                    0
                }
            }
            0x0300..=0x031F => {
                let idx = ((offset - 0x0300) / 4) as usize;
                if idx < NUM_U32 {
                    u64::from(self.isr[idx])
                } else {
                    0
                }
            }
            0x0400..=0x041F => {
                let idx = ((offset - 0x0400) / 4) as usize;
                if idx < NUM_U32 {
                    u64::from(self.isr[idx] & self.enable[idx])
                } else {
                    0
                }
            }
            0x0800..=0x08FF => {
                let base = (offset - 0x0800) as usize;
                if base + 7 < NUM_IRQS {
                    let mut val = 0u64;
                    for i in 0..8 {
                        val |= u64::from(self.coremap[base + i]) << (i * 8);
                    }
                    val
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    pub fn mmio_write(&mut self, offset: u64, val: u64) {
        match offset {
            0x00C0..=0x00C7 => {
                for i in 0..8 {
                    self.ipmap[i] = (val >> (i * 8)) as u8;
                }
            }
            0x0200..=0x021F => {
                let idx = ((offset - 0x0200) / 4) as usize;
                if idx < NUM_U32 {
                    self.enable[idx] = val as u32;
                }
            }
            0x0280..=0x029F => {
                let idx = ((offset - 0x0280) / 4) as usize;
                if idx < NUM_U32 {
                    self.bounce[idx] = val as u32;
                }
            }
            0x0400..=0x041F => {
                let idx = ((offset - 0x0400) / 4) as usize;
                if idx < NUM_U32 {
                    self.isr[idx] &= !(val as u32);
                }
            }
            0x0800..=0x08FF => {
                let base = (offset - 0x0800) as usize;
                for i in 0..8 {
                    if base + i < NUM_IRQS {
                        self.coremap[base + i] = (val >> (i * 8)) as u8;
                    }
                }
            }
            _ => {}
        }
    }
}

impl Default for Eiointc {
    fn default() -> Self {
        Self::new()
    }
}
