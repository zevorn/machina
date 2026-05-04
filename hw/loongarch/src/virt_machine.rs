pub const VIRT_UART_BASE: u64 = 0x1FE0_01E0;
pub const VIRT_UART_SIZE: u64 = 0x8;
pub const VIRT_IPI_BASE: u64 = 0x0100_0000;
pub const VIRT_IPI_SIZE: u64 = 0x100;
pub const VIRT_EIOINTC_BASE: u64 = 0x0200_0000;
pub const VIRT_EIOINTC_SIZE: u64 = 0x1_0000;
pub const VIRT_PCH_PIC_BASE: u64 = 0x1000_0000;
pub const VIRT_PCH_PIC_SIZE: u64 = 0x100;
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
