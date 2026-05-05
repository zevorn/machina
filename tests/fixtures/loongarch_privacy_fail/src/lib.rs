use machina_guest_loongarch::loongarch::cpu::LoongArchCpu;

pub fn try_write_gpr() {
    let mut cpu = LoongArchCpu::new();
    cpu.gpr[0] = 42;
}

pub fn try_write_pc() {
    let mut cpu = LoongArchCpu::new();
    cpu.pc = 0x1000;
}

pub fn try_write_crmd() {
    let mut cpu = LoongArchCpu::new();
    cpu.crmd = 0xFF;
}

pub fn try_write_estat() {
    let mut cpu = LoongArchCpu::new();
    cpu.estat = 1;
}

pub fn try_write_as_ptr() {
    let mut cpu = LoongArchCpu::new();
    cpu.as_ptr = 0xDEAD;
}

pub fn try_write_ram_base() {
    let mut cpu = LoongArchCpu::new();
    cpu.ram_base = 0x8000_0000;
}

pub fn try_write_tb_flush() {
    let mut cpu = LoongArchCpu::new();
    cpu.tb_flush_pending = true;
}
