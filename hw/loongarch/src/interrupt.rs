use std::sync::{Arc, Mutex};

use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;
use machina_hw_char::uart::Uart16550;
use machina_hw_core::bus::SysBusError;
use machina_hw_core::irq::{InterruptSource, IrqLine, IrqSink};
use machina_hw_intc::eiointc::{Eiointc, EiointcIrqSink};
use machina_hw_intc::pch_pic::{PchPic, PchPicIrqSink};

pub const LOONGARCH_DEVICE_HWI: u8 = 1;
pub const LOONGARCH_UART_PCH_IRQ: u32 = 2;
pub const LOONGARCH_VIRTIO_PCH_IRQ_BASE: u32 = 3;

const PCH_INT_MASK: u64 = 0x020;
const PCH_HTMSI_VEC: u64 = 0x200;
const EIO_IPMAP: u64 = 0x0c0;
const EIO_ENABLE: u64 = 0x200;
const EIO_CORE_ISR: u64 = 0x400;
const EIO_COREMAP: u64 = 0x800;

pub struct LoongArchInterruptCascade {
    pch_pic: Arc<PchPic>,
    eiointc: Arc<Eiointc>,
}

impl LoongArchInterruptCascade {
    #[must_use]
    pub fn new(local_id: &str, num_cpus: u32) -> Self {
        Self {
            pch_pic: Arc::new(PchPic::new_named(
                &format!("{local_id}-pch-pic"),
                32,
            )),
            eiointc: Arc::new(Eiointc::new_named(
                &format!("{local_id}-eiointc"),
                num_cpus,
            )),
        }
    }

    #[must_use]
    pub fn from_devices(pch_pic: Arc<PchPic>, eiointc: Arc<Eiointc>) -> Self {
        Self { pch_pic, eiointc }
    }

    #[must_use]
    pub fn pch_pic(&self) -> Arc<PchPic> {
        Arc::clone(&self.pch_pic)
    }

    #[must_use]
    pub fn eiointc(&self) -> Arc<Eiointc> {
        Arc::clone(&self.eiointc)
    }

    pub fn connect_cpu_hwi(
        &self,
        cpu_id: u32,
        hwi: u8,
        cpu: Arc<Mutex<LoongArchCpu>>,
    ) {
        self.eiointc.connect_hwi_output(
            cpu_id,
            hwi,
            InterruptSource::new(
                Arc::new(LoongArchCpuHwiSink { cpu }) as Arc<dyn IrqSink>,
                u32::from(hwi),
            ),
        );
    }

    pub fn route_pch_irq_to_cpu_hwi(
        &self,
        pch_irq: u32,
        eio_irq: u32,
        cpu_id: u32,
        hwi: u8,
    ) {
        self.pch_pic.connect_output(
            eio_irq,
            InterruptSource::new(
                Arc::new(EiointcIrqSink(Arc::clone(&self.eiointc)))
                    as Arc<dyn IrqSink>,
                eio_irq,
            ),
        );
        self.pch_pic.mmio_write_sized(
            PCH_HTMSI_VEC + u64::from(pch_irq),
            1,
            u64::from(eio_irq),
        );
        let pch_mask = self.pch_pic.mmio_read_sized(PCH_INT_MASK, 8);
        self.pch_pic.mmio_write_sized(
            PCH_INT_MASK,
            8,
            pch_mask & !(1u64 << pch_irq),
        );
        self.eiointc.mmio_write_sized(
            cpu_id,
            EIO_IPMAP + u64::from(eio_irq / 32),
            1,
            1u64 << hwi,
        );
        self.eiointc.mmio_write_sized(
            cpu_id,
            EIO_COREMAP + u64::from(eio_irq),
            1,
            coremap_byte(cpu_id),
        );
        let enable_offset = EIO_ENABLE + u64::from((eio_irq / 32) * 4);
        let enable = self.eiointc.mmio_read_sized(cpu_id, enable_offset, 4);
        self.eiointc.mmio_write_sized(
            cpu_id,
            enable_offset,
            4,
            enable | (1u64 << (eio_irq % 32)),
        );
    }

    pub fn attach_uart(
        &self,
        uart: &Arc<Uart16550>,
    ) -> Result<(), SysBusError> {
        uart.attach_irq(IrqLine::new(
            Arc::new(PchPicIrqSink(Arc::clone(&self.pch_pic)))
                as Arc<dyn IrqSink>,
            LOONGARCH_UART_PCH_IRQ,
        ))
    }

    #[must_use]
    pub fn virtio_irq_line(&self, index: u32) -> IrqLine {
        IrqLine::new(
            Arc::new(PchPicIrqSink(Arc::clone(&self.pch_pic)))
                as Arc<dyn IrqSink>,
            LOONGARCH_VIRTIO_PCH_IRQ_BASE + index,
        )
    }

    pub fn ack_eiointc(&self, cpu_id: u32, eio_irq: u32) {
        self.eiointc.mmio_write_sized(
            cpu_id,
            EIO_CORE_ISR + u64::from((eio_irq / 32) * 4),
            4,
            1u64 << (eio_irq % 32),
        );
    }
}

struct LoongArchCpuHwiSink {
    cpu: Arc<Mutex<LoongArchCpu>>,
}

impl IrqSink for LoongArchCpuHwiSink {
    fn set_irq(&self, irq: u32, level: bool) {
        self.cpu
            .lock()
            .unwrap()
            .set_hwi_interrupt_pending(irq as u8, level);
    }
}

fn coremap_byte(cpu_id: u32) -> u64 {
    let node = cpu_id / 4;
    let core = cpu_id % 4;
    u64::from(((node << 4) | (1 << core)) as u8)
}
