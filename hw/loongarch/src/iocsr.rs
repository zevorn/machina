use std::sync::Arc;

use machina_guest_loongarch::loongarch::cpu::{IocsrDispatcher, LoongArchCpu};
use machina_hw_intc::eiointc::Eiointc;
use machina_hw_intc::ipi::LoongArchIpi;

const IPI_LOCAL_BASE: u32 = 0x1000;
const IPI_LOCAL_END: u32 = 0x1040;
const IPI_SEND: u32 = 0x1040;
const MAIL_SEND: u32 = 0x1048;
const ANY_SEND: u32 = 0x1158;

const EIO_NODEMAP_BASE: u32 = 0x14a0;
const EIO_NODEMAP_END: u32 = 0x14c0;
const EIO_IPMAP_BASE: u32 = 0x14c0;
const EIO_IPMAP_END: u32 = 0x14c8;
const EIO_ENABLE_BASE: u32 = 0x1600;
const EIO_ENABLE_END: u32 = 0x1620;
const EIO_BOUNCE_BASE: u32 = 0x1680;
const EIO_BOUNCE_END: u32 = 0x16a0;
const EIO_ISR_BASE: u32 = 0x1800;
const EIO_ISR_END: u32 = 0x1820;
const EIO_ROUTE_BASE: u32 = 0x1c00;
const EIO_ROUTE_END: u32 = 0x1d00;

const EIOINTC_IOCSR_BASE: u32 = 0x1400;
const EIOINTC_NODEMAP_BASE: u32 = 0x00a0;
const EIOINTC_CORE_ISR_BASE: u32 = 0x0400;
const EIOINTC_COREMAP_BASE: u32 = 0x0800;

pub struct VirtIocsrBus {
    ipi: Arc<LoongArchIpi>,
    eiointc: Arc<Eiointc>,
}

impl VirtIocsrBus {
    #[must_use]
    pub fn new(ipi: Arc<LoongArchIpi>, eiointc: Arc<Eiointc>) -> Arc<Self> {
        Arc::new(Self { ipi, eiointc })
    }

    pub fn install_on(self: &Arc<Self>, cpu: &mut LoongArchCpu) {
        let opaque = Arc::as_ptr(self).cast_mut().cast::<()>();
        cpu.set_iocsr_dispatcher(IocsrDispatcher::new(
            opaque,
            Self::read_callback,
            Self::write_callback,
        ));
    }

    #[must_use]
    pub fn read(&self, cpu_id: u32, addr: u32, width: u32) -> Option<u64> {
        if is_ipi_addr(addr) {
            return Some(self.ipi.mmio_read_sized(
                cpu_id,
                u64::from(addr),
                width,
            ));
        }
        let offset = eiointc_offset(addr)?;
        Some(self.eiointc.mmio_read_sized(cpu_id, offset, width))
    }

    pub fn write(&self, cpu_id: u32, addr: u32, width: u32, val: u64) -> bool {
        if addr == ANY_SEND && width == 8 {
            return self.write_any_send(val);
        }
        if is_ipi_addr(addr) {
            self.ipi
                .mmio_write_sized(cpu_id, u64::from(addr), width, val);
            return true;
        }
        let Some(offset) = eiointc_offset(addr) else {
            return false;
        };
        self.eiointc.mmio_write_sized(cpu_id, offset, width, val);
        true
    }

    fn write_any_send(&self, val: u64) -> bool {
        let target_cpu = ((val >> 16) & 0x3ff) as u32;
        let dest = (val & 0xffff) as u32;
        if dest == ANY_SEND {
            return false;
        }

        let data = (val >> 32) as u32;
        let byte_mask = ((val >> 27) & 0xf) as u32;
        let old = self.read(target_cpu, dest, 4).unwrap_or(0) as u32;
        let mut merged = data;
        for byte in 0..4 {
            if byte_mask & (1 << byte) != 0 {
                let mask = 0xffu32 << (byte * 8);
                merged = (merged & !mask) | (old & mask);
            }
        }
        self.write(target_cpu, dest, 4, u64::from(merged))
    }

    unsafe extern "C" fn read_callback(
        opaque: *mut (),
        cpu_id: u32,
        addr: u32,
        width: u32,
        out: *mut u64,
    ) -> bool {
        let bus = unsafe { &*opaque.cast::<VirtIocsrBus>() };
        let Some(val) = bus.read(cpu_id, addr, width) else {
            return false;
        };
        unsafe {
            *out = val;
        }
        true
    }

    unsafe extern "C" fn write_callback(
        opaque: *mut (),
        cpu_id: u32,
        addr: u32,
        width: u32,
        val: u64,
    ) -> bool {
        let bus = unsafe { &*opaque.cast::<VirtIocsrBus>() };
        bus.write(cpu_id, addr, width, val)
    }
}

fn is_ipi_addr(addr: u32) -> bool {
    (IPI_LOCAL_BASE..IPI_LOCAL_END).contains(&addr)
        || (IPI_SEND..IPI_SEND + 8).contains(&addr)
        || (MAIL_SEND..MAIL_SEND + 8).contains(&addr)
        || (ANY_SEND..ANY_SEND + 8).contains(&addr)
}

fn eiointc_offset(addr: u32) -> Option<u64> {
    let offset = match addr {
        EIO_NODEMAP_BASE..EIO_NODEMAP_END => {
            EIOINTC_NODEMAP_BASE + (addr - EIO_NODEMAP_BASE)
        }
        EIO_IPMAP_BASE..EIO_IPMAP_END
        | EIO_ENABLE_BASE..EIO_ENABLE_END
        | EIO_BOUNCE_BASE..EIO_BOUNCE_END => addr - EIOINTC_IOCSR_BASE,
        EIO_ISR_BASE..EIO_ISR_END => {
            EIOINTC_CORE_ISR_BASE + (addr - EIO_ISR_BASE)
        }
        EIO_ROUTE_BASE..EIO_ROUTE_END => {
            EIOINTC_COREMAP_BASE + (addr - EIO_ROUTE_BASE)
        }
        _ => return None,
    };
    Some(u64::from(offset))
}
