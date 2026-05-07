use std::sync::Arc;

use machina_core::device_cell::DeviceRefCell;
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::irq::InterruptSource;
use machina_memory::region::MmioOps;

const APLIC_DOMAINCFG: u64 = 0x0000;
const APLIC_DOMAINCFG_RDONLY: u32 = 0x8000_0000;
const APLIC_DOMAINCFG_IE: u32 = 1 << 8;
const APLIC_DOMAINCFG_DM: u32 = 1 << 2;
const APLIC_SOURCECFG_BASE: u64 = 0x0004;
const APLIC_SOURCECFG_D: u32 = 1 << 10;
const APLIC_SOURCECFG_SM_MASK: u32 = 0x0000_0007;
const APLIC_SOURCECFG_SM_INACTIVE: u32 = 0x0;
const APLIC_SOURCECFG_SM_EDGE_RISE: u32 = 0x4;
const APLIC_SOURCECFG_SM_EDGE_FALL: u32 = 0x5;
const APLIC_SOURCECFG_SM_LEVEL_HIGH: u32 = 0x6;
const APLIC_SOURCECFG_SM_LEVEL_LOW: u32 = 0x7;

const APLIC_MMSICFGADDR: u64 = 0x1bc0;
const APLIC_MMSICFGADDRH: u64 = 0x1bc4;
const APLIC_SMSICFGADDR: u64 = 0x1bc8;
const APLIC_SMSICFGADDRH: u64 = 0x1bcc;

const APLIC_XMSICFGADDRH_L: u32 = 1 << 31;
const APLIC_XMSICFGADDRH_HHXS_SHIFT: u32 = 24;
const APLIC_XMSICFGADDRH_LHXS_SHIFT: u32 = 20;
const APLIC_XMSICFGADDRH_HHXW_SHIFT: u32 = 16;
const APLIC_XMSICFGADDRH_LHXW_SHIFT: u32 = 12;
const APLIC_XMSICFGADDRH_BAPPN_MASK: u32 = 0xfff;
const APLIC_XMSICFGADDR_PPN_SHIFT: u32 = 12;

const APLIC_MMSICFGADDRH_VALID_MASK: u32 = APLIC_XMSICFGADDRH_L
    | (0x1f << APLIC_XMSICFGADDRH_HHXS_SHIFT)
    | (0x7 << APLIC_XMSICFGADDRH_LHXS_SHIFT)
    | (0x7 << APLIC_XMSICFGADDRH_HHXW_SHIFT)
    | (0xf << APLIC_XMSICFGADDRH_LHXW_SHIFT)
    | APLIC_XMSICFGADDRH_BAPPN_MASK;

const APLIC_SETIP_BASE: u64 = 0x1c00;
const APLIC_SETIPNUM: u64 = 0x1cdc;

const APLIC_CLRIP_BASE: u64 = 0x1d00;
const APLIC_CLRIPNUM: u64 = 0x1ddc;

const APLIC_SETIE_BASE: u64 = 0x1e00;
const APLIC_SETIENUM: u64 = 0x1edc;

const APLIC_CLRIE_BASE: u64 = 0x1f00;
const APLIC_CLRIENUM: u64 = 0x1fdc;

const APLIC_SETIPNUM_LE: u64 = 0x2000;
const APLIC_SETIPNUM_BE: u64 = 0x2004;

const APLIC_ISTATE_PENDING: u32 = 1 << 0;
const APLIC_ISTATE_ENABLED: u32 = 1 << 1;
const APLIC_ISTATE_ENPEND: u32 = APLIC_ISTATE_ENABLED | APLIC_ISTATE_PENDING;
const APLIC_ISTATE_INPUT: u32 = 1 << 8;

const APLIC_GENMSI: u64 = 0x3000;

const APLIC_TARGET_BASE: u64 = 0x3004;
const APLIC_TARGET_HART_IDX_SHIFT: u32 = 18;
const APLIC_TARGET_HART_IDX_MASK: u32 = 0x3fff;
const APLIC_TARGET_GUEST_IDX_SHIFT: u32 = 12;
const APLIC_TARGET_GUEST_IDX_MASK: u32 = 0x3f;
const APLIC_TARGET_IPRIO_MASK: u32 = 0xff;
const APLIC_TARGET_EIID_MASK: u32 = 0x7ff;

const APLIC_IDC_BASE: u64 = 0x4000;
const APLIC_IDC_SIZE: u64 = 32;

const APLIC_IDC_IDELIVERY: u64 = 0x00;
const APLIC_IDC_IFORCE: u64 = 0x04;
const APLIC_IDC_ITHRESHOLD: u64 = 0x08;
const APLIC_IDC_TOPI: u64 = 0x18;
const APLIC_IDC_TOPI_ID_SHIFT: u32 = 16;
const APLIC_IDC_TOPI_ID_MASK: u32 = 0x3ff;
const APLIC_IDC_CLAIMI: u64 = 0x1c;

type MsiDelivery = Box<dyn Fn(u64, u32) + Send>;

fn bit_mask(width: u32) -> u32 {
    if width >= u32::BITS {
        u32::MAX
    } else {
        (1u32 << width) - 1
    }
}

fn msi_address(
    msicfgaddr: u32,
    msicfgaddr_h: u32,
    hart_idx: u32,
    guest_idx: u32,
) -> u64 {
    let lhxs = (msicfgaddr_h >> APLIC_XMSICFGADDRH_LHXS_SHIFT) & 0x7;
    let lhxw = (msicfgaddr_h >> APLIC_XMSICFGADDRH_LHXW_SHIFT) & 0xf;
    let hhxs = (msicfgaddr_h >> APLIC_XMSICFGADDRH_HHXS_SHIFT) & 0x1f;
    let hhxw = (msicfgaddr_h >> APLIC_XMSICFGADDRH_HHXW_SHIFT) & 0x7;
    let group_idx = hart_idx >> lhxw;

    let mut ppn = u64::from(msicfgaddr);
    ppn |= u64::from(msicfgaddr_h & APLIC_XMSICFGADDRH_BAPPN_MASK) << 32;
    ppn |= u64::from(group_idx & bit_mask(hhxw))
        << (hhxs + APLIC_XMSICFGADDR_PPN_SHIFT);
    ppn |= u64::from(hart_idx & bit_mask(lhxw)) << lhxs;
    ppn |= u64::from(guest_idx & bit_mask(lhxs));
    ppn << APLIC_XMSICFGADDR_PPN_SHIFT
}

#[derive(Clone, Copy, Default)]
struct IdcRegs {
    idelivery: u32,
    iforce: u32,
    ithreshold: u32,
}

#[derive(machina_hw_core::SysBusDevice)]
#[mom(state = state, lock = "parking_lot", before_unrealize = lower_outputs)]
pub struct RiscvAplic {
    state: parking_lot::Mutex<SysBusDeviceState>,
    num_irqs: u32,
    num_harts: u32,
    iprio_mask: u32,
    bitfield_words: u32,
    msimode: bool,
    mmode: bool,
    domaincfg: DeviceRefCell<u32>,
    sourcecfg: DeviceRefCell<Vec<u32>>,
    state_bits: DeviceRefCell<Vec<u32>>,
    target: DeviceRefCell<Vec<u32>>,
    idc: DeviceRefCell<Vec<IdcRegs>>,
    mmsicfgaddr: DeviceRefCell<u32>,
    mmsicfgaddr_h: DeviceRefCell<u32>,
    smsicfgaddr: DeviceRefCell<u32>,
    smsicfgaddr_h: DeviceRefCell<u32>,
    genmsi: DeviceRefCell<u32>,
    /// MSI delivery callback — (address, data).
    msi_delivery: parking_lot::Mutex<Option<MsiDelivery>>,
    outputs: parking_lot::Mutex<Vec<Option<InterruptSource>>>,
}

impl RiscvAplic {
    #[must_use]
    pub fn new() -> Self {
        Self::new_named("riscv_aplic", 32, 1, 7, false, false)
    }

    #[must_use]
    pub fn new_named(
        local_id: &str,
        num_irqs: u32,
        num_harts: u32,
        iprio_bits: u32,
        msimode: bool,
        mmode: bool,
    ) -> Self {
        let ni = num_irqs.max(2);
        let nh = num_harts.max(1);
        let ipb = iprio_bits.clamp(1, 8);
        let iprio_mask = (1u32 << ipb) - 1;
        let bitfield_words = ni.div_ceil(32);
        let mut target = vec![0u32; ni as usize];
        if !msimode {
            for t in target.iter_mut().skip(1) {
                *t = 1; // default iprio = 1
            }
        }
        let mut idc_regs = Vec::with_capacity(nh as usize);
        idc_regs.resize_with(nh as usize, IdcRegs::default);
        Self {
            state: parking_lot::Mutex::new(SysBusDeviceState::new(local_id)),
            num_irqs: ni,
            num_harts: nh,
            iprio_mask,
            bitfield_words,
            msimode,
            mmode,
            domaincfg: DeviceRefCell::new(0),
            sourcecfg: DeviceRefCell::new(vec![0u32; ni as usize]),
            state_bits: DeviceRefCell::new(vec![0u32; ni as usize]),
            target: DeviceRefCell::new(target),
            idc: DeviceRefCell::new(idc_regs),
            mmsicfgaddr: DeviceRefCell::new(0),
            mmsicfgaddr_h: DeviceRefCell::new(0),
            smsicfgaddr: DeviceRefCell::new(0),
            smsicfgaddr_h: DeviceRefCell::new(0),
            genmsi: DeviceRefCell::new(0),
            msi_delivery: parking_lot::Mutex::new(None),
            outputs: parking_lot::Mutex::new({
                let mut v = Vec::with_capacity(nh as usize);
                v.resize_with(nh as usize, || None);
                v
            }),
        }
    }

    pub fn set_msi_delivery(&self, cb: MsiDelivery) {
        *self.msi_delivery.lock() = Some(cb);
    }

    pub fn connect_output(&self, hart_idx: u32, irq: InterruptSource) {
        let mut outputs = self.outputs.lock();
        while outputs.len() <= hart_idx as usize {
            outputs.push(None);
        }
        outputs[hart_idx as usize] = Some(irq);
        drop(outputs);
        self.idc_update(hart_idx);
    }

    pub fn set_irq(&self, irq: u32, level: bool) {
        if irq == 0 || irq >= self.num_irqs {
            return;
        }
        let level_int = if level { 1 } else { 0 };
        let sourcecfg = self.sourcecfg.borrow();
        let sc = sourcecfg[irq as usize];
        drop(sourcecfg);

        let state = self.state_bits.borrow()[irq as usize];
        let mut update = false;

        match sc & APLIC_SOURCECFG_SM_MASK {
            APLIC_SOURCECFG_SM_EDGE_RISE
                if level_int > 0
                    && (state & APLIC_ISTATE_INPUT) == 0
                    && (state & APLIC_ISTATE_PENDING) == 0 =>
            {
                self.set_pending_raw(irq, true);
                update = true;
            }
            APLIC_SOURCECFG_SM_EDGE_FALL
                if level_int <= 0
                    && (state & APLIC_ISTATE_INPUT) != 0
                    && (state & APLIC_ISTATE_PENDING) == 0 =>
            {
                self.set_pending_raw(irq, true);
                update = true;
            }
            APLIC_SOURCECFG_SM_LEVEL_HIGH
                if level_int > 0 && (state & APLIC_ISTATE_PENDING) == 0 =>
            {
                self.set_pending_raw(irq, true);
                update = true;
            }
            APLIC_SOURCECFG_SM_LEVEL_LOW
                if level_int <= 0 && (state & APLIC_ISTATE_PENDING) == 0 =>
            {
                self.set_pending_raw(irq, true);
                update = true;
            }
            _ => {}
        }

        let mut state_bits = self.state_bits.borrow();
        if level_int <= 0 {
            state_bits[irq as usize] &= !APLIC_ISTATE_INPUT;
        } else {
            state_bits[irq as usize] |= APLIC_ISTATE_INPUT;
        }
        drop(state_bits);

        if update {
            if self.msimode {
                self.msi_irq_update(irq);
            } else {
                let target = self.target.borrow();
                let idc = (target[irq as usize] >> APLIC_TARGET_HART_IDX_SHIFT)
                    & APLIC_TARGET_HART_IDX_MASK;
                drop(target);
                self.idc_update(idc);
            }
        }
    }

    pub fn reset_runtime(&self) {
        self.lower_outputs();
        let mut domaincfg = self.domaincfg.borrow();
        *domaincfg = 0;
        drop(domaincfg);
        let mut sourcecfg = self.sourcecfg.borrow();
        for v in sourcecfg.iter_mut() {
            *v = 0;
        }
        drop(sourcecfg);
        let mut state_bits = self.state_bits.borrow();
        for v in state_bits.iter_mut() {
            *v = 0;
        }
        drop(state_bits);
        let mut target = self.target.borrow();
        if !self.msimode {
            for t in target.iter_mut().skip(1) {
                *t = 1;
            }
        } else {
            for t in target.iter_mut() {
                *t = 0;
            }
        }
        drop(target);
        let mut idc = self.idc.borrow();
        for r in idc.iter_mut() {
            *r = IdcRegs::default();
        }
        drop(idc);
        let mut mmsicfgaddr = self.mmsicfgaddr.borrow();
        *mmsicfgaddr = 0;
        drop(mmsicfgaddr);
        let mut mmsicfgaddr_h = self.mmsicfgaddr_h.borrow();
        *mmsicfgaddr_h = 0;
        drop(mmsicfgaddr_h);
        let mut smsicfgaddr = self.smsicfgaddr.borrow();
        *smsicfgaddr = 0;
        drop(smsicfgaddr);
        let mut smsicfgaddr_h = self.smsicfgaddr_h.borrow();
        *smsicfgaddr_h = 0;
        drop(smsicfgaddr_h);
        let mut genmsi = self.genmsi.borrow();
        *genmsi = 0;
    }

    // --- observable state for tests ---

    #[must_use]
    pub fn domaincfg_val(&self) -> u32 {
        self.domaincfg.borrow().to_owned()
    }

    #[must_use]
    pub fn sourcecfg_val(&self, irq: u32) -> u32 {
        self.sourcecfg
            .borrow()
            .get(irq as usize)
            .copied()
            .unwrap_or(0)
    }

    #[must_use]
    pub fn state_val(&self, irq: u32) -> u32 {
        self.state_bits
            .borrow()
            .get(irq as usize)
            .copied()
            .unwrap_or(0)
    }

    #[must_use]
    pub fn target_val(&self, irq: u32) -> u32 {
        self.target.borrow().get(irq as usize).copied().unwrap_or(0)
    }

    #[must_use]
    pub fn idelivery_val(&self, idc: u32) -> u32 {
        self.idc
            .borrow()
            .get(idc as usize)
            .map(|r| r.idelivery)
            .unwrap_or(0)
    }

    #[must_use]
    pub fn iforce_val(&self, idc: u32) -> u32 {
        self.idc
            .borrow()
            .get(idc as usize)
            .map(|r| r.iforce)
            .unwrap_or(0)
    }

    #[must_use]
    pub fn ithreshold_val(&self, idc: u32) -> u32 {
        self.idc
            .borrow()
            .get(idc as usize)
            .map(|r| r.ithreshold)
            .unwrap_or(0)
    }

    #[must_use]
    pub fn genmsi_val(&self) -> u32 {
        self.genmsi.borrow().to_owned()
    }

    #[must_use]
    pub fn mmsicfgaddr_val(&self) -> u32 {
        self.mmsicfgaddr.borrow().to_owned()
    }

    #[must_use]
    pub fn mmsicfgaddr_h_val(&self) -> u32 {
        self.mmsicfgaddr_h.borrow().to_owned()
    }

    // --- private helpers ---

    fn source_active(&self, irq: u32) -> bool {
        if irq == 0 || self.num_irqs <= irq {
            return false;
        }
        let sc = self.sourcecfg.borrow()[irq as usize];
        if sc & APLIC_SOURCECFG_D != 0 {
            return false;
        }
        let sm = sc & APLIC_SOURCECFG_SM_MASK;
        sm != APLIC_SOURCECFG_SM_INACTIVE
    }

    fn irq_rectified_val(&self, irq: u32) -> bool {
        if !self.source_active(irq) {
            return false;
        }
        let sc = self.sourcecfg.borrow()[irq as usize];
        let sm = sc & APLIC_SOURCECFG_SM_MASK;
        let state = self.state_bits.borrow()[irq as usize];
        let raw_input = (state & APLIC_ISTATE_INPUT) != 0;
        let irq_inverted = sm == APLIC_SOURCECFG_SM_LEVEL_LOW
            || sm == APLIC_SOURCECFG_SM_EDGE_FALL;
        raw_input ^ irq_inverted
    }

    fn set_pending_raw(&self, irq: u32, pending: bool) {
        let mut state = self.state_bits.borrow();
        if pending {
            state[irq as usize] |= APLIC_ISTATE_PENDING;
        } else {
            state[irq as usize] &= !APLIC_ISTATE_PENDING;
        }
    }

    fn set_pending(&self, irq: u32, pending: bool) {
        if !self.source_active(irq) {
            return;
        }
        let sc = self.sourcecfg.borrow()[irq as usize];
        let sm = sc & APLIC_SOURCECFG_SM_MASK;
        if sm == APLIC_SOURCECFG_SM_LEVEL_HIGH
            || sm == APLIC_SOURCECFG_SM_LEVEL_LOW
        {
            if !self.msimode {
                return;
            }
            if self.msimode && !pending {
                // fall through to set_pending_raw
            } else {
                let state = self.state_bits.borrow()[irq as usize];
                if (state & APLIC_ISTATE_INPUT) != 0
                    && sm == APLIC_SOURCECFG_SM_LEVEL_LOW
                {
                    return;
                }
                if (state & APLIC_ISTATE_INPUT) == 0
                    && sm == APLIC_SOURCECFG_SM_LEVEL_HIGH
                {
                    return;
                }
            }
        }
        self.set_pending_raw(irq, pending);
    }

    fn set_enabled(&self, irq: u32, enabled: bool) {
        if !self.source_active(irq) {
            return;
        }
        let mut state = self.state_bits.borrow();
        if enabled {
            state[irq as usize] |= APLIC_ISTATE_ENABLED;
        } else {
            state[irq as usize] &= !APLIC_ISTATE_ENABLED;
        }
    }

    fn msi_irq_update(&self, irq: u32) {
        if !self.msimode
            || self.num_irqs <= irq
            || (self.domaincfg.borrow().to_owned() & APLIC_DOMAINCFG_IE) == 0
        {
            return;
        }
        let state = self.state_bits.borrow()[irq as usize];
        if (state & APLIC_ISTATE_ENPEND) != APLIC_ISTATE_ENPEND {
            return;
        }
        self.set_pending_raw(irq, false);
        let target = self.target.borrow()[irq as usize];
        let hart_idx = (target >> APLIC_TARGET_HART_IDX_SHIFT)
            & APLIC_TARGET_HART_IDX_MASK;
        let guest_idx = if self.mmode {
            0
        } else {
            (target >> APLIC_TARGET_GUEST_IDX_SHIFT)
                & APLIC_TARGET_GUEST_IDX_MASK
        };
        let eiid = target & APLIC_TARGET_EIID_MASK;
        // Store genmsi for observable MSI generation.
        let mut genmsi = self.genmsi.borrow();
        *genmsi = (hart_idx << APLIC_TARGET_HART_IDX_SHIFT)
            | (guest_idx << APLIC_TARGET_GUEST_IDX_SHIFT)
            | eiid;
        drop(genmsi);

        // Deliver the MSI write into the IMSIC address space.
        // Compute address from MM/SMSICFGADDR registers,
        // writing eiid as data.
        let (addr_lo, addr_hi) = if self.mmode {
            (*self.mmsicfgaddr.borrow(), *self.mmsicfgaddr_h.borrow())
        } else {
            (*self.smsicfgaddr.borrow(), *self.smsicfgaddr_h.borrow())
        };
        let msi_addr = msi_address(addr_lo, addr_hi, hart_idx, guest_idx);
        if let Some(ref cb) = *self.msi_delivery.lock() {
            cb(msi_addr, eiid);
        }
    }

    fn idc_topi(&self, idc: u32) -> u32 {
        if self.num_harts <= idc {
            return 0;
        }
        let ithres = self.idc.borrow()[idc as usize].ithreshold;
        let mut best_irq = u32::MAX;
        let mut best_iprio = u32::MAX;
        let state = self.state_bits.borrow();
        let target = self.target.borrow();
        for irq in 1..self.num_irqs {
            if (state[irq as usize] & APLIC_ISTATE_ENPEND)
                != APLIC_ISTATE_ENPEND
            {
                continue;
            }
            let ihartidx = (target[irq as usize]
                >> APLIC_TARGET_HART_IDX_SHIFT)
                & APLIC_TARGET_HART_IDX_MASK;
            if ihartidx != idc {
                continue;
            }
            let iprio = target[irq as usize] & self.iprio_mask;
            if ithres != 0 && iprio >= ithres {
                continue;
            }
            if iprio < best_iprio {
                best_irq = irq;
                best_iprio = iprio;
            }
        }
        if best_irq < self.num_irqs && best_iprio <= self.iprio_mask {
            return (best_irq << APLIC_IDC_TOPI_ID_SHIFT) | best_iprio;
        }
        0
    }

    fn idc_update(&self, idc: u32) {
        if self.msimode || self.num_harts <= idc {
            return;
        }
        let topi = self.idc_topi(idc);
        let domaincfg = self.domaincfg.borrow().to_owned();
        let idc_regs = self.idc.borrow();
        let idelivery = idc_regs[idc as usize].idelivery;
        let iforce = idc_regs[idc as usize].iforce;
        drop(idc_regs);
        let outputs = self.outputs.lock();
        if let Some(Some(line)) = outputs.get(idc as usize) {
            if (domaincfg & APLIC_DOMAINCFG_IE) != 0
                && idelivery != 0
                && (iforce != 0 || topi != 0)
            {
                line.raise();
            } else {
                line.lower();
            }
        }
    }

    fn idc_claimi(&self, idc: u32) -> u32 {
        let topi = self.idc_topi(idc);
        if topi == 0 {
            let mut idc_regs = self.idc.borrow();
            idc_regs[idc as usize].iforce = 0;
            drop(idc_regs);
            self.idc_update(idc);
            return 0;
        }
        let irq = (topi >> APLIC_IDC_TOPI_ID_SHIFT) & APLIC_IDC_TOPI_ID_MASK;
        let sm =
            self.sourcecfg.borrow()[irq as usize] & APLIC_SOURCECFG_SM_MASK;
        let state = self.state_bits.borrow()[irq as usize];
        self.set_pending_raw(irq, false);
        let input_high = (state & APLIC_ISTATE_INPUT) != 0;
        let re_pend = (sm == APLIC_SOURCECFG_SM_LEVEL_HIGH && input_high)
            || (sm == APLIC_SOURCECFG_SM_LEVEL_LOW && !input_high);
        if re_pend {
            self.set_pending_raw(irq, true);
        }
        self.idc_update(idc);
        topi
    }

    fn lower_outputs(&self) {
        let outputs = self.outputs.lock();
        for line in outputs.iter().flatten() {
            line.lower();
        }
    }
}

impl Default for RiscvAplic {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RiscvAplicMmio(pub Arc<RiscvAplic>);

impl MmioOps for RiscvAplicMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        let a = &self.0;
        if size != 4 {
            return 0;
        }
        if (offset & 0x3) != 0 {
            return 0;
        }
        if offset == APLIC_DOMAINCFG {
            let dc = a.domaincfg.borrow().to_owned();
            let dm = if a.msimode { APLIC_DOMAINCFG_DM } else { 0 };
            return (APLIC_DOMAINCFG_RDONLY | dc | dm) as u64;
        }
        if (APLIC_SOURCECFG_BASE
            ..APLIC_SOURCECFG_BASE + (a.num_irqs as u64 - 1) * 4)
            .contains(&offset)
        {
            let irq = ((offset - APLIC_SOURCECFG_BASE) >> 2) + 1;
            return a.sourcecfg.borrow()[irq as usize] as u64;
        }
        if a.mmode && a.msimode && offset == APLIC_MMSICFGADDR {
            return a.mmsicfgaddr.borrow().to_owned() as u64;
        }
        if a.mmode && a.msimode && offset == APLIC_MMSICFGADDRH {
            return a.mmsicfgaddr_h.borrow().to_owned() as u64;
        }
        if a.mmode && a.msimode && offset == APLIC_SMSICFGADDR {
            return 0;
        }
        if a.mmode && a.msimode && offset == APLIC_SMSICFGADDRH {
            return 0;
        }
        if (APLIC_SETIP_BASE..APLIC_SETIP_BASE + a.bitfield_words as u64 * 4)
            .contains(&offset)
        {
            let word = ((offset - APLIC_SETIP_BASE) >> 2) as u32;
            return read_pending_word(a, word) as u64;
        }
        if offset == APLIC_SETIPNUM {
            return 0;
        }
        if (APLIC_CLRIP_BASE..APLIC_CLRIP_BASE + a.bitfield_words as u64 * 4)
            .contains(&offset)
        {
            let word = ((offset - APLIC_CLRIP_BASE) >> 2) as u32;
            return read_input_word(a, word) as u64;
        }
        if offset == APLIC_CLRIPNUM {
            return 0;
        }
        if (APLIC_SETIE_BASE..APLIC_SETIE_BASE + a.bitfield_words as u64 * 4)
            .contains(&offset)
        {
            let word = ((offset - APLIC_SETIE_BASE) >> 2) as u32;
            return read_enabled_word(a, word) as u64;
        }
        if [
            APLIC_SETIENUM,
            APLIC_CLRIENUM,
            APLIC_SETIPNUM_LE,
            APLIC_SETIPNUM_BE,
        ]
        .contains(&offset)
        {
            return 0;
        }
        if (APLIC_CLRIE_BASE..APLIC_CLRIE_BASE + a.bitfield_words as u64 * 4)
            .contains(&offset)
        {
            return 0;
        }
        if offset == APLIC_GENMSI {
            return if a.msimode {
                a.genmsi.borrow().to_owned() as u64
            } else {
                0
            };
        }
        if (APLIC_TARGET_BASE..APLIC_TARGET_BASE + (a.num_irqs as u64 - 1) * 4)
            .contains(&offset)
        {
            let irq = (((offset - APLIC_TARGET_BASE) >> 2) + 1) as u32;
            if !a.source_active(irq) {
                return 0;
            }
            return a.target.borrow()[irq as usize] as u64;
        }
        if !a.msimode
            && (APLIC_IDC_BASE
                ..APLIC_IDC_BASE + a.num_harts as u64 * APLIC_IDC_SIZE)
                .contains(&offset)
        {
            let idc = ((offset - APLIC_IDC_BASE) / APLIC_IDC_SIZE) as u32;
            let sub = offset - (APLIC_IDC_BASE + idc as u64 * APLIC_IDC_SIZE);
            return match sub {
                APLIC_IDC_IDELIVERY => {
                    a.idc.borrow()[idc as usize].idelivery as u64
                }
                APLIC_IDC_IFORCE => a.idc.borrow()[idc as usize].iforce as u64,
                APLIC_IDC_ITHRESHOLD => {
                    a.idc.borrow()[idc as usize].ithreshold as u64
                }
                APLIC_IDC_TOPI => a.idc_topi(idc) as u64,
                APLIC_IDC_CLAIMI => a.idc_claimi(idc) as u64,
                _ => 0,
            };
        }
        0
    }

    fn write(&self, offset: u64, size: u32, value: u64) {
        let a = &self.0;
        if size != 4 {
            return;
        }
        if (offset & 0x3) != 0 {
            return;
        }
        let val = value as u32;

        if offset == APLIC_DOMAINCFG {
            let v = val & APLIC_DOMAINCFG_IE;
            *a.domaincfg.borrow() = v;
        } else if (APLIC_SOURCECFG_BASE
            ..APLIC_SOURCECFG_BASE + (a.num_irqs as u64 - 1) * 4)
            .contains(&offset)
        {
            let irq = (((offset - APLIC_SOURCECFG_BASE) >> 2) + 1) as u32;
            let mut v = val;
            if v & APLIC_SOURCECFG_D != 0 {
                v = 0;
            } else {
                v &= APLIC_SOURCECFG_SM_MASK;
            }
            a.sourcecfg.borrow()[irq as usize] = v;
            let new_sc = v;
            if (new_sc & APLIC_SOURCECFG_D) != 0 || new_sc == 0 {
                a.set_pending_raw(irq, false);
                let mut state = a.state_bits.borrow();
                state[irq as usize] &= !APLIC_ISTATE_ENABLED;
                drop(state);
            } else if a.irq_rectified_val(irq) {
                a.set_pending_raw(irq, true);
            }
        } else if a.mmode && a.msimode && offset == APLIC_MMSICFGADDR {
            if a.mmsicfgaddr_h.borrow().to_owned() & APLIC_XMSICFGADDRH_L == 0 {
                *a.mmsicfgaddr.borrow() = val;
            }
        } else if a.mmode && a.msimode && offset == APLIC_MMSICFGADDRH {
            if a.mmsicfgaddr_h.borrow().to_owned() & APLIC_XMSICFGADDRH_L == 0 {
                *a.mmsicfgaddr_h.borrow() = val & APLIC_MMSICFGADDRH_VALID_MASK;
            }
        } else if a.mmode
            && a.msimode
            && matches!(offset, APLIC_SMSICFGADDR | APLIC_SMSICFGADDRH)
        {
            // Hidden until child supervisor domains are modelled.
        } else if (APLIC_SETIP_BASE
            ..APLIC_SETIP_BASE + a.bitfield_words as u64 * 4)
            .contains(&offset)
        {
            let word = ((offset - APLIC_SETIP_BASE) >> 2) as u32;
            set_pending_word(a, word, val, true);
        } else if offset == APLIC_SETIPNUM {
            a.set_pending(val, true);
        } else if (APLIC_CLRIP_BASE
            ..APLIC_CLRIP_BASE + a.bitfield_words as u64 * 4)
            .contains(&offset)
        {
            let word = ((offset - APLIC_CLRIP_BASE) >> 2) as u32;
            set_pending_word(a, word, val, false);
        } else if offset == APLIC_CLRIPNUM {
            a.set_pending(val, false);
        } else if (APLIC_SETIE_BASE
            ..APLIC_SETIE_BASE + a.bitfield_words as u64 * 4)
            .contains(&offset)
        {
            let word = ((offset - APLIC_SETIE_BASE) >> 2) as u32;
            set_enabled_word(a, word, val, true);
        } else if offset == APLIC_SETIENUM {
            a.set_enabled(val, true);
        } else if (APLIC_CLRIE_BASE
            ..APLIC_CLRIE_BASE + a.bitfield_words as u64 * 4)
            .contains(&offset)
        {
            let word = ((offset - APLIC_CLRIE_BASE) >> 2) as u32;
            set_enabled_word(a, word, val, false);
        } else if offset == APLIC_CLRIENUM {
            a.set_enabled(val, false);
        } else if offset == APLIC_SETIPNUM_LE {
            a.set_pending(val, true);
        } else if offset == APLIC_SETIPNUM_BE {
            a.set_pending(val.swap_bytes(), true);
        } else if offset == APLIC_GENMSI {
            if a.msimode {
                let v = val
                    & !(APLIC_TARGET_GUEST_IDX_MASK
                        << APLIC_TARGET_GUEST_IDX_SHIFT);
                *a.genmsi.borrow() = v;
                let hart_idx = (val >> APLIC_TARGET_HART_IDX_SHIFT)
                    & APLIC_TARGET_HART_IDX_MASK;
                let eiid = val & APLIC_TARGET_EIID_MASK;
                let mut genmsi = a.genmsi.borrow();
                *genmsi = (hart_idx << APLIC_TARGET_HART_IDX_SHIFT) | eiid;
                drop(genmsi);
                let (addr_lo, addr_hi) = if a.mmode {
                    (*a.mmsicfgaddr.borrow(), *a.mmsicfgaddr_h.borrow())
                } else {
                    (*a.smsicfgaddr.borrow(), *a.smsicfgaddr_h.borrow())
                };
                let msi_addr = msi_address(addr_lo, addr_hi, hart_idx, 0);
                if let Some(ref cb) = *a.msi_delivery.lock() {
                    cb(msi_addr, eiid);
                }
            }
        } else if (APLIC_TARGET_BASE
            ..APLIC_TARGET_BASE + (a.num_irqs as u64 - 1) * 4)
            .contains(&offset)
        {
            let irq = (((offset - APLIC_TARGET_BASE) >> 2) + 1) as u32;
            if !a.source_active(irq) {
                return;
            }
            if a.msimode {
                a.target.borrow()[irq as usize] = val;
            } else {
                let masked_prio = if val & a.iprio_mask != 0 {
                    val & a.iprio_mask
                } else {
                    1
                };
                a.target.borrow()[irq as usize] =
                    (val & !APLIC_TARGET_IPRIO_MASK) | masked_prio;
            }
        } else if !a.msimode
            && (APLIC_IDC_BASE
                ..APLIC_IDC_BASE + a.num_harts as u64 * APLIC_IDC_SIZE)
                .contains(&offset)
        {
            let idc = ((offset - APLIC_IDC_BASE) / APLIC_IDC_SIZE) as u32;
            let sub = offset - (APLIC_IDC_BASE + idc as u64 * APLIC_IDC_SIZE);
            match sub {
                APLIC_IDC_IDELIVERY => {
                    a.idc.borrow()[idc as usize].idelivery = val & 0x1;
                }
                APLIC_IDC_IFORCE => {
                    a.idc.borrow()[idc as usize].iforce = val & 0x1;
                }
                APLIC_IDC_ITHRESHOLD => {
                    a.idc.borrow()[idc as usize].ithreshold =
                        val & a.iprio_mask;
                }
                _ => return,
            }
        } else {
            return;
        }

        // Post-write: update all relevant outputs
        if a.msimode {
            for irq in 1..a.num_irqs {
                a.msi_irq_update(irq);
            }
        } else {
            let idc = if (APLIC_IDC_BASE
                ..APLIC_IDC_BASE + a.num_harts as u64 * APLIC_IDC_SIZE)
                .contains(&offset)
            {
                Some(((offset - APLIC_IDC_BASE) / APLIC_IDC_SIZE) as u32)
            } else {
                None
            };
            if let Some(idc) = idc {
                a.idc_update(idc);
            } else {
                for idc in 0..a.num_harts {
                    a.idc_update(idc);
                }
            }
        }
    }
}

// --- bitfield helpers ---

fn read_pending_word(a: &RiscvAplic, word: u32) -> u32 {
    let mut ret = 0u32;
    let state = a.state_bits.borrow();
    for i in 0..32 {
        let irq = word * 32 + i;
        if irq == 0 || a.num_irqs <= irq {
            continue;
        }
        if (state[irq as usize] & APLIC_ISTATE_PENDING) != 0 {
            ret |= 1 << i;
        }
    }
    ret
}

fn read_input_word(a: &RiscvAplic, word: u32) -> u32 {
    let mut ret = 0u32;
    for i in 0..32 {
        let irq = word * 32 + i;
        if a.irq_rectified_val(irq) {
            ret |= 1 << i;
        }
    }
    ret
}

fn read_enabled_word(a: &RiscvAplic, word: u32) -> u32 {
    let mut ret = 0u32;
    let state = a.state_bits.borrow();
    for i in 0..32 {
        let irq = word * 32 + i;
        if irq == 0 || a.num_irqs <= irq {
            continue;
        }
        if (state[irq as usize] & APLIC_ISTATE_ENABLED) != 0 {
            ret |= 1 << i;
        }
    }
    ret
}

fn set_pending_word(a: &RiscvAplic, word: u32, value: u32, pending: bool) {
    for i in 0..32 {
        let irq = word * 32 + i;
        if irq == 0 || a.num_irqs <= irq {
            continue;
        }
        if value & (1 << i) != 0 {
            a.set_pending(irq, pending);
        }
    }
}

fn set_enabled_word(a: &RiscvAplic, word: u32, value: u32, enabled: bool) {
    for i in 0..32 {
        let irq = word * 32 + i;
        if irq == 0 || a.num_irqs <= irq {
            continue;
        }
        if value & (1 << i) != 0 {
            a.set_enabled(irq, enabled);
        }
    }
}
