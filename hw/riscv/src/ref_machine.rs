// riscv64-ref machine: virt-compatible reference platform.

use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_core::machine::{Machine, MachineOpts};
use machina_hw_char::uart::Uart16550;
use machina_hw_core::fdt::FdtBuilder;
use machina_hw_intc::aclint::Aclint;
use machina_hw_intc::plic::Plic;
use machina_memory::address_space::AddressSpace;
use machina_memory::ram::RamBlock;
use machina_memory::region::{MemoryRegion, MmioOps};

// QEMU virt memory map base addresses.
const PLIC_BASE: u64 = 0x0C00_0000;
const ACLINT_BASE: u64 = 0x0200_0000;
const UART0_BASE: u64 = 0x1000_0000;
pub const RAM_BASE: u64 = 0x8000_0000;

// Region sizes.
const PLIC_SIZE: u64 = 0x0400_0000;
const ACLINT_SIZE: u64 = 0x0001_0000;
const UART0_SIZE: u64 = 0x100;

// CLINT sub-region boundary: offsets below this are MSWI,
// offsets at or above are MTIMER.
const CLINT_MTIMER_OFFSET: u64 = 0x4000;

const UART_IRQ: u32 = 10;
const PLIC_NUM_SOURCES: u32 = 96;
// M-mode + S-mode per hart.
const PLIC_NUM_CONTEXTS: u32 = 2;

// ---- MMIO adapter: PLIC ----

struct PlicMmio(Arc<Mutex<Plic>>);

impl MmioOps for PlicMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        self.0.lock().unwrap().read(offset, size)
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        self.0.lock().unwrap().write(offset, size, val);
    }
}

// ---- MMIO adapter: ACLINT (CLINT-compatible) ----

struct AclintMmio(Arc<Mutex<Aclint>>);

impl MmioOps for AclintMmio {
    fn read(&self, offset: u64, size: u32) -> u64 {
        let dev = self.0.lock().unwrap();
        if offset < CLINT_MTIMER_OFFSET {
            dev.mswi_read(offset, size)
        } else {
            dev.mtimer_read(offset - CLINT_MTIMER_OFFSET, size)
        }
    }

    fn write(&self, offset: u64, size: u32, val: u64) {
        let mut dev = self.0.lock().unwrap();
        if offset < CLINT_MTIMER_OFFSET {
            dev.mswi_write(offset, size, val);
        } else {
            dev.mtimer_write(offset - CLINT_MTIMER_OFFSET, size, val);
        }
    }
}

// ---- MMIO adapter: UART 16550 ----

struct UartMmio(Arc<Mutex<Uart16550>>);

impl MmioOps for UartMmio {
    fn read(&self, offset: u64, _size: u32) -> u64 {
        self.0.lock().unwrap().read(offset) as u64
    }

    fn write(&self, offset: u64, _size: u32, val: u64) {
        self.0.lock().unwrap().write(offset, val as u8);
    }
}

// ---- RefMachine ----

pub struct RefMachine {
    name: String,
    ram_size: u64,
    cpu_count: u32,
    address_space: Option<AddressSpace>,
    ram_block: Option<Arc<RamBlock>>,
    plic: Option<Arc<Mutex<Plic>>>,
    aclint: Option<Arc<Mutex<Aclint>>>,
    uart: Option<Arc<Mutex<Uart16550>>>,
    fdt_blob: Option<Vec<u8>>,
}

impl RefMachine {
    pub fn new() -> Self {
        Self {
            name: "riscv64-ref".to_string(),
            ram_size: 0,
            cpu_count: 0,
            address_space: None,
            ram_block: None,
            plic: None,
            aclint: None,
            uart: None,
            fdt_blob: None,
        }
    }

    pub fn address_space(&self) -> &AddressSpace {
        self.address_space
            .as_ref()
            .expect("machine not initialized")
    }

    pub fn plic(&self) -> &Arc<Mutex<Plic>> {
        self.plic.as_ref().expect("machine not initialized")
    }

    pub fn aclint(&self) -> &Arc<Mutex<Aclint>> {
        self.aclint.as_ref().expect("machine not initialized")
    }

    pub fn uart(&self) -> &Arc<Mutex<Uart16550>> {
        self.uart.as_ref().expect("machine not initialized")
    }

    pub fn ram_block(&self) -> &Arc<RamBlock> {
        self.ram_block.as_ref().expect("machine not initialized")
    }

    pub fn fdt_blob(&self) -> &[u8] {
        self.fdt_blob.as_ref().expect("machine not initialized")
    }

    /// Write a byte slice into RAM at `offset` bytes from
    /// RAM_BASE.
    pub fn write_ram(
        &self,
        offset: u64,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let block = self.ram_block();
        let end = offset + data.len() as u64;
        if end > block.size() {
            return Err(format!(
                "write_ram: offset {offset:#x} + len {:#x} \
                 exceeds RAM size {:#x}",
                data.len(),
                block.size()
            )
            .into());
        }
        // SAFETY: offset + data.len() is within the
        // mmap'd allocation (checked above).
        unsafe {
            let dst = block.as_ptr().add(offset as usize);
            std::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
        }
        Ok(())
    }

    fn generate_fdt(&self) -> Vec<u8> {
        let mut fdt = FdtBuilder::new();

        // Root node.
        fdt.begin_node("");
        fdt.property_string("compatible", "machina,riscv64-ref");
        fdt.property_u32("#address-cells", 2);
        fdt.property_u32("#size-cells", 2);

        // /cpus
        fdt.begin_node("cpus");
        fdt.property_u32("#address-cells", 1);
        fdt.property_u32("#size-cells", 0);
        fdt.property_u32("timebase-frequency", 10_000_000);
        for i in 0..self.cpu_count {
            let name = format!("cpu@{i}");
            fdt.begin_node(&name);
            fdt.property_string("device_type", "cpu");
            fdt.property_u32("reg", i);
            fdt.property_string("compatible", "riscv");
            fdt.property_string("riscv,isa", "rv64imafdc");
            fdt.property_string("mmu-type", "riscv,sv48");
            fdt.property_string("status", "okay");
            // Interrupt controller sub-node.
            fdt.begin_node("interrupt-controller");
            fdt.property_u32("#interrupt-cells", 1);
            fdt.property_bytes("interrupt-controller", &[]);
            fdt.property_string("compatible", "riscv,cpu-intc");
            fdt.end_node();
            fdt.end_node();
        }
        fdt.end_node(); // /cpus

        // /memory@80000000
        fdt.begin_node("memory@80000000");
        fdt.property_string("device_type", "memory");
        // reg = <0x0 0x80000000 0x0 ram_size>
        fdt.property_u32_list(
            "reg",
            &[
                0,
                RAM_BASE as u32,
                (self.ram_size >> 32) as u32,
                self.ram_size as u32,
            ],
        );
        fdt.end_node();

        // /soc
        fdt.begin_node("soc");
        fdt.property_u32("#address-cells", 2);
        fdt.property_u32("#size-cells", 2);
        fdt.property_string("compatible", "simple-bus");
        fdt.property_bytes("ranges", &[]);

        // /soc/plic@c000000
        fdt.begin_node("plic@c000000");
        fdt.property_string("compatible", "sifive,plic-1.0.0");
        fdt.property_u32("#interrupt-cells", 1);
        fdt.property_bytes("interrupt-controller", &[]);
        fdt.property_u32_list(
            "reg",
            &[0, PLIC_BASE as u32, 0, PLIC_SIZE as u32],
        );
        fdt.property_u32("riscv,ndev", PLIC_NUM_SOURCES - 1);
        fdt.end_node();

        // /soc/clint@2000000
        fdt.begin_node("clint@2000000");
        fdt.property_string("compatible", "riscv,clint0");
        fdt.property_u32_list(
            "reg",
            &[0, ACLINT_BASE as u32, 0, ACLINT_SIZE as u32],
        );
        fdt.end_node();

        // /soc/serial@10000000
        fdt.begin_node("serial@10000000");
        fdt.property_string("compatible", "ns16550a");
        fdt.property_u32_list(
            "reg",
            &[0, UART0_BASE as u32, 0, UART0_SIZE as u32],
        );
        fdt.property_u32("interrupts", UART_IRQ);
        fdt.end_node();

        fdt.end_node(); // /soc

        // /chosen
        fdt.begin_node("chosen");
        fdt.property_string("stdout-path", "/soc/serial@10000000");
        fdt.end_node();

        fdt.end_node(); // root
        fdt.finish()
    }
}

impl Default for RefMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine for RefMachine {
    fn name(&self) -> &str {
        &self.name
    }

    fn init(
        &mut self,
        opts: &MachineOpts,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if opts.ram_size == 0 {
            return Err("ram_size must be greater than 0".into());
        }
        self.ram_size = opts.ram_size;
        self.cpu_count = opts.cpu_count;

        // Build the address space.
        let mut root = MemoryRegion::container("system", u64::MAX);

        // RAM at 0x8000_0000.
        let (ram_region, ram_block) = MemoryRegion::ram("ram", opts.ram_size);
        self.ram_block = Some(ram_block);
        root.add_subregion(ram_region, GPA::new(RAM_BASE));

        // PLIC.
        let plic = Arc::new(Mutex::new(Plic::new(
            PLIC_NUM_SOURCES,
            PLIC_NUM_CONTEXTS,
        )));
        let plic_mmio = PlicMmio(Arc::clone(&plic));
        let plic_region =
            MemoryRegion::io("plic", PLIC_SIZE, Box::new(plic_mmio));
        root.add_subregion(plic_region, GPA::new(PLIC_BASE));
        self.plic = Some(plic);

        // ACLINT (CLINT-compatible).
        let aclint = Arc::new(Mutex::new(Aclint::new(opts.cpu_count)));
        let aclint_mmio = AclintMmio(Arc::clone(&aclint));
        let aclint_region =
            MemoryRegion::io("clint", ACLINT_SIZE, Box::new(aclint_mmio));
        root.add_subregion(aclint_region, GPA::new(ACLINT_BASE));
        self.aclint = Some(aclint);

        // UART0.
        let uart = Arc::new(Mutex::new(Uart16550::new()));
        let uart_mmio = UartMmio(Arc::clone(&uart));
        let uart_region =
            MemoryRegion::io("uart0", UART0_SIZE, Box::new(uart_mmio));
        root.add_subregion(uart_region, GPA::new(UART0_BASE));
        self.uart = Some(uart);

        self.address_space = Some(AddressSpace::new(root));

        // Generate FDT.
        self.fdt_blob = Some(self.generate_fdt());

        Ok(())
    }

    fn reset(&mut self) {
        // Re-create devices with fresh state.
        if let Some(plic) = &self.plic {
            *plic.lock().unwrap() =
                Plic::new(PLIC_NUM_SOURCES, PLIC_NUM_CONTEXTS);
        }
        if let Some(aclint) = &self.aclint {
            *aclint.lock().unwrap() = Aclint::new(self.cpu_count);
        }
        if let Some(uart) = &self.uart {
            *uart.lock().unwrap() = Uart16550::new();
        }
    }

    fn pause(&mut self) {
        // No-op for now.
    }

    fn resume(&mut self) {
        // No-op for now.
    }

    fn shutdown(&mut self) {
        // No-op for now.
    }

    fn boot(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Actual boot loading is done via boot::setup_boot.
        Ok(())
    }

    fn cpu_count(&self) -> usize {
        self.cpu_count as usize
    }

    fn ram_size(&self) -> u64 {
        self.ram_size
    }
}
