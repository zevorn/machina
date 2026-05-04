use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_core::machine::{Machine, MachineOpts, MachineState};
use machina_guest_loongarch::loongarch::cpu::{
    LoongArchCpu, LoongArchCpuInterruptState,
};
use machina_guest_loongarch::loongarch::csr::{CRMD_DA, CSR_CRMD};
use machina_hw_char::uart::{Uart16550, Uart16550Mmio};
use machina_hw_core::bus::{SysBus, SysBusError};
use machina_hw_core::chardev::{CharFrontend, StdioChardev};
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_intc::eiointc::{Eiointc, EiointcMmio};
use machina_hw_intc::ipi::{LoongArchIpi, LoongArchIpiMmio};
use machina_hw_intc::pch_pic::{PchPic, PchPicMmio};
use machina_hw_virtio::mmio::VirtioMmio;
use machina_memory::address_space::AddressSpace;
use machina_memory::ram::RamBlock;
use machina_memory::region::MemoryRegion;

use crate::boot;
use crate::interrupt::{
    LoongArchInterruptCascade, LOONGARCH_DEVICE_HWI, LOONGARCH_UART_PCH_IRQ,
    LOONGARCH_VIRTIO_PCH_IRQ_BASE,
};
use crate::iocsr::VirtIocsrBus;

type UartRxCallback = Arc<Mutex<dyn FnMut(u8) + Send>>;

pub const VIRT_UART_BASE: u64 = 0x1FE0_01E0;
pub const VIRT_UART_SIZE: u64 = 0x8;
pub const VIRT_IPI_BASE: u64 = 0x0100_0000;
pub const VIRT_IPI_SIZE: u64 = 0x100;
pub const VIRT_EIOINTC_BASE: u64 = 0x0200_0000;
pub const VIRT_EIOINTC_SIZE: u64 = 0x1_0000;
pub const VIRT_PCH_PIC_BASE: u64 = 0x1000_0000;
pub const VIRT_PCH_PIC_SIZE: u64 = 0x400;
pub const VIRT_VIRTIO_BASE: u64 = 0x1000_8000;
pub const VIRT_VIRTIO_SIZE: u64 = 0x1000;
pub const VIRT_RAM_BASE: u64 = 0x9000_0000_0000_0000;
pub const VIRT_RAM_SIZE_DEFAULT: u64 = 256 * 1024 * 1024;

pub const VIRT_CPUCFG_PRID: u32 = 0x0014_C010;

pub struct VirtMachineConfig {
    pub ram_size: u64,
    pub kernel_path: Option<String>,
}

impl Default for VirtMachineConfig {
    fn default() -> Self {
        Self {
            ram_size: VIRT_RAM_SIZE_DEFAULT,
            kernel_path: None,
        }
    }
}

pub struct LoongArchVirtMachine {
    name: String,
    machine_state: MachineState,
    ram_size: u64,
    cpu: Option<Arc<Mutex<LoongArchCpu>>>,
    address_space: Option<AddressSpace>,
    sysbus: Option<SysBus>,
    ram_block: Option<Arc<RamBlock>>,
    uart: Option<Arc<Uart16550>>,
    ipi: Option<Arc<LoongArchIpi>>,
    interrupt_cascade: Option<LoongArchInterruptCascade>,
    iocsr_bus: Option<Arc<VirtIocsrBus>>,
    virtio_mmio: Option<VirtioMmio>,
    kernel_path: Option<PathBuf>,
    initrd_path: Option<PathBuf>,
    kernel_cmdline: Option<String>,
    uart_chardev: Option<CharFrontend>,
}

impl LoongArchVirtMachine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            name: "loongarch64-ref".to_string(),
            machine_state: MachineState::new_root("machine"),
            ram_size: 0,
            cpu: None,
            address_space: None,
            sysbus: None,
            ram_block: None,
            uart: None,
            ipi: None,
            interrupt_cascade: None,
            iocsr_bus: None,
            virtio_mmio: None,
            kernel_path: None,
            initrd_path: None,
            kernel_cmdline: None,
            uart_chardev: None,
        }
    }

    pub fn address_space(&self) -> &AddressSpace {
        self.address_space
            .as_ref()
            .expect("machine not initialized")
    }

    pub fn sysbus(&self) -> &SysBus {
        self.sysbus.as_ref().expect("machine not initialized")
    }

    #[must_use]
    pub fn cpu(&self) -> Arc<Mutex<LoongArchCpu>> {
        Arc::clone(self.cpu.as_ref().expect("machine not initialized"))
    }

    #[must_use]
    pub fn ram_block(&self) -> &Arc<RamBlock> {
        self.ram_block.as_ref().expect("machine not initialized")
    }

    #[must_use]
    pub fn uart(&self) -> Arc<Uart16550> {
        Arc::clone(self.uart.as_ref().expect("machine not initialized"))
    }

    pub fn take_runtime_cpu_state(
        &mut self,
    ) -> Result<
        (LoongArchCpu, Arc<LoongArchCpuInterruptState>),
        Box<dyn std::error::Error>,
    > {
        let interrupts = Arc::new(LoongArchCpuInterruptState::default());
        self.ipi()
            .connect_output(0, runtime_ipi_source(Arc::clone(&interrupts)));
        self.interrupt_cascade
            .as_ref()
            .expect("machine not initialized")
            .connect_cpu_hwi_async(
                0,
                LOONGARCH_DEVICE_HWI,
                Arc::clone(&interrupts),
            );

        let cpu_arc = self.cpu();
        let mut guard = cpu_arc.lock().unwrap();
        let cpu = std::mem::replace(&mut *guard, LoongArchCpu::new());
        drop(guard);

        Ok((cpu, interrupts))
    }

    #[must_use]
    pub fn ipi(&self) -> Arc<LoongArchIpi> {
        Arc::clone(self.ipi.as_ref().expect("machine not initialized"))
    }

    #[must_use]
    pub fn eiointc(&self) -> Arc<Eiointc> {
        self.interrupt_cascade
            .as_ref()
            .expect("machine not initialized")
            .eiointc()
    }

    #[must_use]
    pub fn pch_pic(&self) -> Arc<PchPic> {
        self.interrupt_cascade
            .as_ref()
            .expect("machine not initialized")
            .pch_pic()
    }

    #[must_use]
    pub fn iocsr_bus(&self) -> Arc<VirtIocsrBus> {
        Arc::clone(self.iocsr_bus.as_ref().expect("machine not initialized"))
    }

    #[must_use]
    pub fn interrupt_cascade(&self) -> &LoongArchInterruptCascade {
        self.interrupt_cascade
            .as_ref()
            .expect("machine not initialized")
    }

    pub fn set_uart_chardev(
        &mut self,
        frontend: CharFrontend,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.uart.is_some() {
            return Err(
                "loongarch64-ref UART chardev must be set before init".into()
            );
        }
        self.uart_chardev = Some(frontend);
        Ok(())
    }

    #[must_use]
    pub fn virtio_mmio(&self) -> Option<&VirtioMmio> {
        self.virtio_mmio.as_ref()
    }

    fn attach_interrupt_devices(
        sysbus: &mut SysBus,
        ipi: &Arc<LoongArchIpi>,
        eiointc: &Arc<Eiointc>,
        pch_pic: &Arc<PchPic>,
    ) -> Result<(), SysBusError> {
        ipi.attach_to_bus(sysbus)?;
        ipi.register_mmio(
            MemoryRegion::io(
                "ipi0-mmio",
                VIRT_IPI_SIZE,
                Arc::new(LoongArchIpiMmio(Arc::clone(ipi), 0)),
            ),
            GPA::new(VIRT_IPI_BASE),
        )?;

        eiointc.attach_to_bus(sysbus)?;
        eiointc.register_mmio(
            MemoryRegion::io(
                "eiointc0-mmio",
                VIRT_EIOINTC_SIZE,
                Arc::new(EiointcMmio(Arc::clone(eiointc))),
            ),
            GPA::new(VIRT_EIOINTC_BASE),
        )?;

        pch_pic.attach_to_bus(sysbus)?;
        pch_pic.register_mmio(
            MemoryRegion::io(
                "pch-pic0-mmio",
                VIRT_PCH_PIC_SIZE,
                Arc::new(PchPicMmio(Arc::clone(pch_pic))),
            ),
            GPA::new(VIRT_PCH_PIC_BASE),
        )
    }

    fn realize_interrupt_devices(
        sysbus: &mut SysBus,
        address_space: &mut AddressSpace,
        ipi: &Arc<LoongArchIpi>,
        eiointc: &Arc<Eiointc>,
        pch_pic: &Arc<PchPic>,
    ) -> Result<(), SysBusError> {
        ipi.realize_onto(sysbus, address_space)?;
        eiointc.realize_onto(sysbus, address_space)?;
        pch_pic.realize_onto(sysbus, address_space)
    }
}

impl Default for LoongArchVirtMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine for LoongArchVirtMachine {
    fn name(&self) -> &str {
        &self.name
    }

    fn machine_state(&self) -> &MachineState {
        &self.machine_state
    }

    fn machine_state_mut(&mut self) -> &mut MachineState {
        &mut self.machine_state
    }

    fn init(
        &mut self,
        opts: &MachineOpts,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if opts.ram_size == 0 {
            return Err("ram_size must be greater than 0".into());
        }
        if opts.cpu_count != 1 {
            return Err("loongarch64-ref currently supports one CPU".into());
        }
        if opts.netdev.is_some() {
            return Err(
                "loongarch64-ref does not support virtio-net-device/-netdev"
                    .into(),
            );
        }

        self.ram_size = opts.ram_size;
        self.kernel_path = opts.kernel.clone();
        self.initrd_path = opts.initrd.clone();
        self.kernel_cmdline = opts.append.clone();

        let mut sysbus = SysBus::new("sysbus0");
        let mut root = MemoryRegion::container("system", u64::MAX);

        let (ram_region, ram_block) = MemoryRegion::ram("ram", opts.ram_size);
        root.add_subregion(ram_region, GPA::new(VIRT_RAM_BASE));

        let cpu = Arc::new(Mutex::new(LoongArchCpu::new()));
        {
            let ram_ptr = ram_block.as_ptr() as usize;
            let mut cpu_guard = cpu.lock().unwrap();
            cpu_guard.set_cpuid(0);
            cpu_guard.set_guest_base(
                ram_ptr.wrapping_sub(VIRT_RAM_BASE as usize) as u64,
            );
            cpu_guard.set_ram_base(VIRT_RAM_BASE);
            cpu_guard.set_ram_end(VIRT_RAM_BASE + opts.ram_size);
        }

        let ipi = Arc::new(LoongArchIpi::new_named("ipi0", opts.cpu_count));
        ipi.connect_output(
            0,
            InterruptSource::new(
                Arc::new(LoongArchCpuIpiSink {
                    cpu: Arc::clone(&cpu),
                }) as Arc<dyn IrqSink>,
                0,
            ),
        );

        let eiointc = Arc::new(Eiointc::new_named("eiointc0", opts.cpu_count));
        let pch_pic = Arc::new(PchPic::new_named("pch-pic0", 32));
        let cascade = LoongArchInterruptCascade::from_devices(
            Arc::clone(&pch_pic),
            Arc::clone(&eiointc),
        );
        cascade.connect_cpu_hwi(0, LOONGARCH_DEVICE_HWI, Arc::clone(&cpu));

        let iocsr_bus =
            VirtIocsrBus::new(Arc::clone(&ipi), Arc::clone(&eiointc));
        iocsr_bus.install_on(&mut cpu.lock().unwrap());

        Self::attach_interrupt_devices(&mut sysbus, &ipi, &eiointc, &pch_pic)?;

        let uart = Arc::new(Uart16550::new_named("uart0"));
        uart.attach_to_bus(&mut sysbus)?;
        uart.register_mmio(
            MemoryRegion::io(
                "uart0",
                VIRT_UART_SIZE,
                Arc::new(Uart16550Mmio(Arc::clone(&uart))),
            ),
            GPA::new(VIRT_UART_BASE),
        )?;
        cascade.attach_uart(&uart)?;
        cascade.route_pch_irq_to_cpu_hwi(
            LOONGARCH_UART_PCH_IRQ,
            LOONGARCH_UART_PCH_IRQ,
            0,
            LOONGARCH_DEVICE_HWI,
        );
        if let Some(frontend) = self.uart_chardev.take() {
            uart.attach_chardev(frontend)?;
        } else if opts.nographic {
            uart.attach_chardev(CharFrontend::new(Box::new(
                StdioChardev::new(),
            )))?;
        }

        let mut virtio_mmio = if let Some(drive_path) = &opts.drive {
            use machina_hw_virtio::block::VirtioBlk;

            let blk = VirtioBlk::open(drive_path)?;
            cascade.route_pch_irq_to_cpu_hwi(
                LOONGARCH_VIRTIO_PCH_IRQ_BASE,
                LOONGARCH_VIRTIO_PCH_IRQ_BASE,
                0,
                LOONGARCH_DEVICE_HWI,
            );
            let mut mmio = VirtioMmio::new_named(
                "virtio-mmio0",
                Box::new(blk),
                cascade.virtio_irq_line(0),
                ram_block.as_ptr(),
                // Linux programs virtio descriptors with low physical
                // addresses from the memory@0 FDT node, not VIRT_RAM_BASE.
                0,
                opts.ram_size,
            );
            mmio.attach_to_bus(&mut sysbus)?;
            let region =
                mmio.make_mmio_region("virtio-mmio0", VIRT_VIRTIO_SIZE);
            mmio.register_mmio(region, GPA::new(VIRT_VIRTIO_BASE))?;
            Some(mmio)
        } else {
            None
        };

        let mut address_space = AddressSpace::new(root);
        Self::realize_interrupt_devices(
            &mut sysbus,
            &mut address_space,
            &ipi,
            &eiointc,
            &pch_pic,
        )?;

        if let Some(mmio) = virtio_mmio.as_mut() {
            mmio.realize_onto(&mut sysbus, &mut address_space)?;
        }

        let uart_for_rx = Arc::clone(&uart);
        let rx_cb: UartRxCallback = Arc::new(Mutex::new(move |byte: u8| {
            uart_for_rx.receive(byte);
        }));
        uart.realize_onto(&mut sysbus, &mut address_space, rx_cb)?;

        self.cpu = Some(cpu);
        self.address_space = Some(address_space);
        self.sysbus = Some(sysbus);
        self.ram_block = Some(ram_block);
        self.uart = Some(uart);
        self.ipi = Some(ipi);
        self.interrupt_cascade = Some(cascade);
        self.iocsr_bus = Some(iocsr_bus);
        self.virtio_mmio = virtio_mmio;

        Ok(())
    }

    fn reset(&mut self) {
        if let Some(uart) = &self.uart {
            uart.reset_runtime();
        }
        if let Some(virtio_mmio) = &mut self.virtio_mmio {
            virtio_mmio.reset_runtime();
        }
    }

    fn pause(&mut self) {}

    fn resume(&mut self) {}

    fn shutdown(&mut self) {}

    fn boot(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let kernel_path = self
            .kernel_path
            .as_ref()
            .ok_or("loongarch64-ref boot requires a kernel image")?;
        let boot_config = boot::DirectKernelBootConfig {
            cmdline: self.kernel_cmdline.as_deref(),
            initrd_path: self.initrd_path.as_deref(),
            has_virtio_mmio: self.virtio_mmio.is_some(),
        };
        let boot_info = boot::load_direct_kernel(
            kernel_path,
            &boot_config,
            self.ram_size,
            self.address_space(),
        )?;

        let cpu = self.cpu();
        let mut cpu = cpu.lock().unwrap();
        cpu.set_pc(boot_info.entry);
        cpu.csr_write(CSR_CRMD, CRMD_DA);
        cpu.write_gpr(4, boot_info.efi_boot);
        cpu.write_gpr(5, boot_info.cmdline_addr);
        cpu.write_gpr(6, boot_info.system_table_addr);
        Ok(())
    }

    fn cpu_count(&self) -> usize {
        usize::from(self.cpu.is_some())
    }

    fn ram_size(&self) -> u64 {
        self.ram_size
    }
}

struct LoongArchCpuIpiSink {
    cpu: Arc<Mutex<LoongArchCpu>>,
}

impl IrqSink for LoongArchCpuIpiSink {
    fn set_irq(&self, _irq: u32, level: bool) {
        self.cpu.lock().unwrap().set_ipi_interrupt_pending(level);
    }
}

struct LoongArchCpuAsyncIpiSink {
    interrupts: Arc<LoongArchCpuInterruptState>,
}

impl IrqSink for LoongArchCpuAsyncIpiSink {
    fn set_irq(&self, _irq: u32, level: bool) {
        self.interrupts.set_ipi_interrupt_pending(level);
    }
}

fn runtime_ipi_source(
    interrupts: Arc<LoongArchCpuInterruptState>,
) -> InterruptSource {
    InterruptSource::new(
        Arc::new(LoongArchCpuAsyncIpiSink { interrupts }) as Arc<dyn IrqSink>,
        0,
    )
}
