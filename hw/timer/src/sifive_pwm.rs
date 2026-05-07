use std::sync::Arc;

use machina_core::address::GPA;
use machina_core::device_cell::DeviceRefCell;
use machina_core::mobject::{MObject, MObjectInfo};
use machina_hw_core::bus::{SysBus, SysBusDeviceState, SysBusError};
use machina_hw_core::irq::InterruptSource;
use machina_hw_core::mdev::MDevice;
use machina_memory::address_space::AddressSpace;
use machina_memory::region::{MemoryRegion, MmioOps};

const R_CONFIG: u64 = 0x00;
const R_COUNT: u64 = 0x08;
const R_PWMS: u64 = 0x10;
const R_PWMCMP0: u64 = 0x20;
const R_PWMCMP1: u64 = 0x24;
const R_PWMCMP2: u64 = 0x28;
const R_PWMCMP3: u64 = 0x2C;

const PWM_CHANS: usize = 4;

const PWMCMP_MASK: u32 = 0xFFFF;
const PWMCOUNT_MASK: u32 = 0x7FFF_FFFF;

// Config register fields
const CONFIG_SCALE: u32 = 0x0F;
const CONFIG_ZEROCMP: u32 = 1 << 9;
const CONFIG_ENALWAYS: u32 = 1 << 12;
const CONFIG_ENONESHOT: u32 = 1 << 13;
const CONFIG_CMP0IP: u32 = 1 << 28;
const CONFIG_CMP1IP: u32 = 1 << 29;
const CONFIG_CMP2IP: u32 = 1 << 30;
const CONFIG_CMP3IP: u32 = 1 << 31;

fn has_pwm_en_bits(cfg: u32) -> bool {
    (cfg & (CONFIG_ENONESHOT | CONFIG_ENALWAYS)) != 0
}

fn width_mask(size: u32) -> u64 {
    match size {
        1 => 0xff,
        2 => 0xffff,
        4 => 0xffff_ffff,
        _ => u64::MAX,
    }
}

fn read_unaligned(mmio: &SiFivePwmMmio, offset: u64, size: u32) -> Option<u64> {
    if !needs_unaligned_split(offset, size) {
        return None;
    }

    let mut value = 0u64;
    let mut done = 0u32;
    while done < size {
        let cur = offset + u64::from(done);
        let chunk = aligned_chunk_size(cur, size - done);
        value |= (mmio.read(cur, chunk) & width_mask(chunk)) << (done * 8);
        done += chunk;
    }
    Some(value)
}

fn write_unaligned(
    mmio: &SiFivePwmMmio,
    offset: u64,
    size: u32,
    val: u64,
) -> bool {
    if !needs_unaligned_split(offset, size) {
        return false;
    }

    let mut done = 0u32;
    while done < size {
        let cur = offset + u64::from(done);
        let chunk = aligned_chunk_size(cur, size - done);
        let chunk_value = (val >> (done * 8)) & width_mask(chunk);
        mmio.write(cur, chunk, chunk_value);
        done += chunk;
    }
    true
}

fn needs_unaligned_split(offset: u64, size: u32) -> bool {
    matches!(size, 2 | 4 | 8) && !offset.is_multiple_of(u64::from(size))
}

fn aligned_chunk_size(offset: u64, remaining: u32) -> u32 {
    for size in [8u32, 4, 2, 1] {
        if remaining >= size && offset.is_multiple_of(u64::from(size)) {
            return size;
        }
    }
    1
}

struct SiFivePwmRegs {
    freq_hz: u64,
    pwmcfg: u32,
    pwmcmp: [u32; PWM_CHANS],
    now_ticks: u64,
    tick_offset: u64,
    irq_state: [bool; PWM_CHANS],
}

impl SiFivePwmRegs {
    fn new(freq_hz: u64) -> Self {
        Self {
            freq_hz,
            pwmcfg: 0,
            pwmcmp: [0; PWM_CHANS],
            now_ticks: 0,
            tick_offset: 0,
            irq_state: [false; PWM_CHANS],
        }
    }

    fn reset(&mut self) {
        self.pwmcfg = 0;
        self.pwmcmp = [0; PWM_CHANS];
        self.tick_offset = self.now_ticks;
        self.irq_state = [false; PWM_CHANS];
    }

    fn ns_to_ticks(&self, ns: u64) -> u64 {
        // ticks = ns * freq_hz / 1_000_000_000
        (ns as u128 * self.freq_hz as u128 / 1_000_000_000u128) as u64
    }

    fn ticks_to_ns(&self, ticks: u64) -> u64 {
        (ticks as u128 * 1_000_000_000u128 / self.freq_hz as u128) as u64
    }

    fn scale(&self) -> u32 {
        self.pwmcfg & CONFIG_SCALE
    }

    fn pwmcount(&self) -> u32 {
        let mask = u64::from(PWMCOUNT_MASK);
        if has_pwm_en_bits(self.pwmcfg) {
            (self.now_ticks.wrapping_sub(self.tick_offset) & mask) as u32
        } else {
            (self.tick_offset & mask) as u32
        }
    }

    fn pwms(&self) -> u32 {
        let count = self.pwmcount();
        let s = self.scale();
        (count >> s) & PWMCMP_MASK
    }

    fn check_irqs(&self) -> [bool; PWM_CHANS] {
        let pwms = self.pwms();
        let mut state = [false; PWM_CHANS];
        for (cmp, s) in self.pwmcmp.iter().zip(state.iter_mut()) {
            *s = pwms >= (*cmp & PWMCMP_MASK);
        }
        state
    }
}

pub struct SiFivePwm {
    state: parking_lot::Mutex<SysBusDeviceState>,
    regs: DeviceRefCell<SiFivePwmRegs>,
    outputs: parking_lot::Mutex<[Option<InterruptSource>; PWM_CHANS]>,
}

impl SiFivePwm {
    #[must_use]
    pub fn new() -> Self {
        Self::new_with_freq(500_000_000)
    }

    #[must_use]
    pub fn new_with_freq(freq_hz: u64) -> Self {
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(
                "sifive_pwm",
            )),
            regs: DeviceRefCell::new(SiFivePwmRegs::new(freq_hz)),
            outputs: parking_lot::Mutex::new([None, None, None, None]),
        }
    }

    pub fn attach_to_bus(&self, bus: &mut SysBus) -> Result<(), SysBusError> {
        self.state.lock().attach_to_bus(bus)
    }

    pub fn register_mmio(
        &self,
        region: MemoryRegion,
        base: GPA,
    ) -> Result<(), SysBusError> {
        self.state.lock().register_mmio(region, base)
    }

    pub fn realize_onto(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.state.lock().realize_onto(bus, address_space)?;
        Ok(())
    }

    pub fn unrealize_from(
        &self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        self.lower_outputs();
        self.state.lock().unrealize_from(bus, address_space)?;
        Ok(())
    }

    #[must_use]
    pub fn realized(&self) -> bool {
        self.state.lock().device().is_realized()
    }

    #[must_use]
    pub fn object_info(&self) -> MObjectInfo {
        self.state.lock().object_info()
    }

    pub fn with_mdevice<T>(&self, f: impl FnOnce(&dyn MDevice) -> T) -> T {
        let guard = self.state.lock();
        f(&*guard)
    }

    pub fn connect_output(&self, chan: usize, irq: InterruptSource) {
        self.outputs.lock()[chan] = Some(irq);
    }

    pub fn reset_runtime(&self) {
        self.regs.borrow().reset();
        self.lower_outputs();
    }

    fn lower_outputs(&self) {
        for l in self.outputs.lock().iter().flatten() {
            l.lower();
        }
    }

    fn update_irqs(&self, irq_state: &[bool; PWM_CHANS]) {
        for (i, line) in self.outputs.lock().iter().enumerate() {
            if let Some(ref l) = line {
                l.set(irq_state[i]);
            }
        }
    }

    /// Advance time by `ns` nanoseconds.
    pub fn tick(&self, ns: u64) {
        let mut regs = self.regs.borrow();
        let was_incrementing = has_pwm_en_bits(regs.pwmcfg);
        let delta = regs.ns_to_ticks(ns);
        regs.now_ticks = regs.now_ticks.wrapping_add(delta);
        let now = regs.now_ticks;

        if has_pwm_en_bits(regs.pwmcfg) {
            // Check for overflow (carry out)
            if was_incrementing
                && (now & PWMCOUNT_MASK as u64)
                    < (regs.tick_offset & PWMCOUNT_MASK as u64)
            {
                regs.pwmcfg &= !CONFIG_ENONESHOT;
            }
        }

        // Update IRQ state
        let pwms = regs.pwms();
        for i in 0..PWM_CHANS {
            let pwmcmp = regs.pwmcmp[i] & PWMCMP_MASK;
            let firing = pwms >= pwmcmp;
            if firing {
                regs.pwmcfg |= CONFIG_CMP0IP << i;
            }
            regs.irq_state[i] = firing;
        }
        let irq_state = regs.irq_state;

        // ZEROCMP handling: if cmp0 fired and zerocmp is set, reset
        if (regs.pwmcfg & CONFIG_ZEROCMP) != 0 && irq_state[0] {
            regs.pwmcfg &= !CONFIG_ENONESHOT;
            if was_incrementing {
                regs.tick_offset = now;
            } else {
                regs.tick_offset = 0;
            }
        }

        // If transitioning from enabled to disabled, convert tick_offset
        if was_incrementing && !has_pwm_en_bits(regs.pwmcfg) {
            regs.tick_offset =
                now.wrapping_sub(regs.tick_offset) & PWMCOUNT_MASK as u64;
        }

        drop(regs);
        self.update_irqs(&irq_state);
    }

    /// Get current time in nanoseconds (based on tick_offset and freq).
    #[must_use]
    pub fn current_time_ns(&self) -> u64 {
        let regs = self.regs.borrow();
        regs.ticks_to_ns(regs.tick_offset)
    }

    /// Read config register (for testing).
    #[must_use]
    pub fn config(&self) -> u32 {
        self.regs.borrow().pwmcfg
    }
}

impl Default for SiFivePwm {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SiFivePwmMmio(pub Arc<SiFivePwm>);

impl MmioOps for SiFivePwmMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        if let Some(value) = read_unaligned(self, offset, size) {
            return value;
        }

        if size == 8 {
            let lo = self.read(offset, 4);
            let hi = self.read(offset.wrapping_add(4), 4);
            return lo | (hi << 32);
        }

        let regs = self.0.regs.borrow();
        let value = match offset {
            R_CONFIG => u64::from(regs.pwmcfg),
            R_COUNT => {
                // Return count value with bit 31 always 0
                // Uses tick_offset; now_ns would come from virtual clock.
                // For testing, we use tick() to advance time.
                let count = regs.tick_offset & PWMCOUNT_MASK as u64;
                if has_pwm_en_bits(regs.pwmcfg) {
                    u64::from(regs.pwmcount())
                } else {
                    count
                }
            }
            R_PWMS => u64::from(regs.pwms()),
            R_PWMCMP0 => u64::from(regs.pwmcmp[0] & PWMCMP_MASK),
            R_PWMCMP1 => u64::from(regs.pwmcmp[1] & PWMCMP_MASK),
            R_PWMCMP2 => u64::from(regs.pwmcmp[2] & PWMCMP_MASK),
            R_PWMCMP3 => u64::from(regs.pwmcmp[3] & PWMCMP_MASK),
            _ => 0,
        };
        value & width_mask(size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        if write_unaligned(self, offset, size, val) {
            return;
        }

        if size == 8 {
            self.write(offset, 4, val);
            self.write(offset.wrapping_add(4), 4, val >> 32);
            return;
        }

        let value = (val & width_mask(size)) as u32;
        let mut regs = self.0.regs.borrow();
        match offset {
            R_CONFIG => {
                // Check for enable/disable transition
                let old_has_en = has_pwm_en_bits(regs.pwmcfg);
                let new_has_en = has_pwm_en_bits(value);

                if old_has_en != new_has_en {
                    regs.tick_offset =
                        regs.now_ticks.wrapping_sub(regs.tick_offset)
                            & PWMCOUNT_MASK as u64;
                }

                // If IP bits are cleared, lower IRQs
                if (value & CONFIG_CMP0IP) == 0 {
                    regs.irq_state[0] = false;
                }
                if (value & CONFIG_CMP1IP) == 0 {
                    regs.irq_state[1] = false;
                }
                if (value & CONFIG_CMP2IP) == 0 {
                    regs.irq_state[2] = false;
                }
                if (value & CONFIG_CMP3IP) == 0 {
                    regs.irq_state[3] = false;
                }

                regs.pwmcfg = value;
            }
            R_COUNT => {
                let new_offset = u64::from(value) & PWMCOUNT_MASK as u64;
                if has_pwm_en_bits(regs.pwmcfg) {
                    regs.tick_offset = regs.now_ticks.wrapping_sub(new_offset);
                } else {
                    regs.tick_offset = new_offset;
                }
            }
            R_PWMS => {
                let s = regs.scale();
                let new_offset = ((u64::from(value) & PWMCMP_MASK as u64) << s)
                    & PWMCOUNT_MASK as u64;
                if has_pwm_en_bits(regs.pwmcfg) {
                    regs.tick_offset = regs.now_ticks.wrapping_sub(new_offset);
                } else {
                    regs.tick_offset = new_offset;
                }
            }
            R_PWMCMP0 => regs.pwmcmp[0] = value & PWMCMP_MASK,
            R_PWMCMP1 => regs.pwmcmp[1] = value & PWMCMP_MASK,
            R_PWMCMP2 => regs.pwmcmp[2] = value & PWMCMP_MASK,
            R_PWMCMP3 => regs.pwmcmp[3] = value & PWMCMP_MASK,
            _ => {}
        }
        let irq_state = regs.check_irqs();
        let needs_update = irq_state != regs.irq_state;
        regs.irq_state = irq_state;
        drop(regs);
        if needs_update {
            self.0.update_irqs(&irq_state);
        }
    }
}
