use std::sync::{Arc, Mutex};

use machina_accel::code_buffer::CodeBuffer;
use machina_accel::ir::Context;
use machina_accel::translate::translate_and_execute;
use machina_accel::{HostCodeGen, X86_64CodeGen};
use machina_guest_loongarch::loongarch::cpu::{
    LoongArchCpu, GUEST_BASE_CPU_OFFSET,
};
use machina_guest_loongarch::loongarch::csr::{
    CRMD_DA, CRMD_IE, CSR_CRMD, CSR_ECFG, CSR_ESTAT,
};
use machina_guest_loongarch::loongarch::ext::LoongArchCfg;
use machina_guest_loongarch::loongarch::trans::{
    LoongArchDisasContext, LoongArchTranslator,
};
use machina_guest_loongarch::translator_loop;
use machina_hw_core::irq::{InterruptSource, IrqSink};
use machina_hw_intc::eiointc::Eiointc;
use machina_hw_intc::ipi::LoongArchIpi;
use machina_hw_loongarch::iocsr::VirtIocsrBus;

const IPI_LINE: u64 = 1 << 12;
const OP_IOCSRRD_B: u32 = 0b0000011001001000000000;
const OP_IOCSRRD_H: u32 = 0b0000011001001000000001;
const OP_IOCSRRD_W: u32 = 0b0000011001001000000010;
const OP_IOCSRRD_D: u32 = 0b0000011001001000000011;
const OP_IOCSRWR_B: u32 = 0b0000011001001000000100;
const OP_IOCSRWR_H: u32 = 0b0000011001001000000101;
const OP_IOCSRWR_W: u32 = 0b0000011001001000000110;
const OP_IOCSRWR_D: u32 = 0b0000011001001000000111;

struct CpuIpiSink {
    cpu: Arc<Mutex<LoongArchCpu>>,
}

impl IrqSink for CpuIpiSink {
    fn set_irq(&self, _irq: u32, level: bool) {
        self.cpu.lock().unwrap().set_ipi_interrupt_pending(level);
    }
}

#[derive(Default)]
struct RecordingSink {
    lines: Mutex<Vec<bool>>,
}

impl RecordingSink {
    fn line(&self, irq: u32) -> bool {
        self.lines
            .lock()
            .unwrap()
            .get(irq as usize)
            .copied()
            .unwrap_or(false)
    }
}

impl IrqSink for RecordingSink {
    fn set_irq(&self, irq: u32, level: bool) {
        let mut lines = self.lines.lock().unwrap();
        while lines.len() <= irq as usize {
            lines.push(false);
        }
        lines[irq as usize] = level;
    }
}

fn r2_insn(op: u32, rj: u32, rd: u32) -> u32 {
    (op << 10) | (rj << 5) | rd
}

fn run_la(cpu: &mut LoongArchCpu, insns: &[u32]) -> usize {
    let mut code = vec![0_u8; insns.len() * 4];
    for (idx, insn) in insns.iter().enumerate() {
        let off = idx * 4;
        code[off..off + 4].copy_from_slice(&insn.to_le_bytes());
    }

    let mut backend = X86_64CodeGen::new();
    backend.set_guest_base_offset(GUEST_BASE_CPU_OFFSET);
    let mut buf = CodeBuffer::new(4096).unwrap();
    backend.emit_prologue(&mut buf);
    backend.emit_epilogue(&mut buf);

    let mut ir = Context::new();
    backend.init_context(&mut ir);

    let mut ctx =
        LoongArchDisasContext::new(0, code.as_ptr(), LoongArchCfg::default());
    ctx.base.max_insns = insns.len() as u32;
    translator_loop::<LoongArchTranslator>(&mut ctx, &mut ir);

    unsafe { translate_and_execute(&mut ir, &backend, &mut buf, cpu.env_ptr()) }
}

fn cpu_with_id(cpu_id: u32) -> LoongArchCpu {
    let mut cpu = LoongArchCpu::new();
    cpu.set_cpuid(u64::from(cpu_id));
    cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    cpu.csr_write(CSR_ECFG, IPI_LINE);
    cpu
}

fn iocsr_read(cpu: &mut LoongArchCpu, op: u32, addr: u32) -> u64 {
    cpu.write_gpr(2, u64::from(addr));
    run_la(cpu, &[r2_insn(op, 2, 5)]);
    cpu.read_gpr(5)
}

fn iocsr_write(cpu: &mut LoongArchCpu, op: u32, addr: u32, val: u64) {
    cpu.write_gpr(2, u64::from(addr));
    cpu.write_gpr(3, val);
    run_la(cpu, &[r2_insn(op, 2, 3)]);
}

fn any_send_val(target_cpu: u32, dest: u32, data: u32, byte_mask: u32) -> u64 {
    (1_u64 << 31)
        | u64::from(dest)
        | (u64::from(target_cpu) << 16)
        | (u64::from(byte_mask) << 27)
        | (u64::from(data) << 32)
}

fn make_bus(
    num_cpus: u32,
) -> (Arc<LoongArchIpi>, Arc<Eiointc>, Arc<VirtIocsrBus>) {
    let ipi = Arc::new(LoongArchIpi::new_named("ipi0", num_cpus));
    let eiointc = Arc::new(Eiointc::new_named("eiointc0", num_cpus));
    let bus = VirtIocsrBus::new(Arc::clone(&ipi), Arc::clone(&eiointc));
    (ipi, eiointc, bus)
}

#[test]
fn task39_translated_iocsr_any_send_routes_eiointc_aliases() {
    let (_ipi, eiointc, bus) = make_bus(2);
    let mut cpu0 = cpu_with_id(0);
    let mut cpu1 = cpu_with_id(1);
    bus.install_on(&mut cpu0);
    bus.install_on(&mut cpu1);
    iocsr_write(&mut cpu0, OP_IOCSRWR_W, 0x14c0, 0x0202_0202);
    iocsr_write(&mut cpu0, OP_IOCSRWR_W, 0x1c00, 0x0101_0101);

    iocsr_write(
        &mut cpu0,
        OP_IOCSRWR_D,
        0x1158,
        any_send_val(0, 0x1c00, 0x0000_0200, 0b1101),
    );
    assert_eq!(
        eiointc.mmio_read_sized(0, 0x800, 4),
        0x0101_0201,
        "ANY_SEND must merge route bytes before dispatching to EIOINTC"
    );

    iocsr_write(
        &mut cpu0,
        OP_IOCSRWR_D,
        0x1158,
        any_send_val(0, 0x1600, 1 << 1, 0),
    );
    eiointc.set_irq(1, true);

    assert_eq!(iocsr_read(&mut cpu0, OP_IOCSRRD_W, 0x1800), 0);
    assert_eq!(iocsr_read(&mut cpu1, OP_IOCSRRD_W, 0x1800), 1 << 1);

    iocsr_write(
        &mut cpu0,
        OP_IOCSRWR_D,
        0x1158,
        any_send_val(1, 0x1800, 1 << 1, 0),
    );
    assert_eq!(
        iocsr_read(&mut cpu1, OP_IOCSRRD_W, 0x1800),
        0,
        "ANY_SEND must use the encoded target CPU for core-ISR ack"
    );
}

#[test]
fn task39_translated_iocsr_routes_ipi_send_to_nonzero_target() {
    let (ipi, eiointc, bus) = make_bus(2);
    let mut requester = cpu_with_id(0);
    let target = Arc::new(Mutex::new(cpu_with_id(1)));
    bus.install_on(&mut requester);
    ipi.connect_output(
        1,
        InterruptSource::new(
            Arc::new(CpuIpiSink {
                cpu: Arc::clone(&target),
            }) as Arc<dyn IrqSink>,
            0,
        ),
    );
    ipi.mmio_write_sized(1, 0x1004, 4, 1 << 3);

    iocsr_write(&mut requester, OP_IOCSRWR_W, 0x1040, (1 << 16) | 3);

    assert_eq!(ipi.mmio_read_sized(1, 0x1000, 4), 1 << 3);
    assert_eq!(
        target.lock().unwrap().csr_read(CSR_ESTAT) & IPI_LINE,
        IPI_LINE
    );
    assert_eq!(eiointc.pending_for_cpu(0), 0);
}

#[test]
fn task39_translated_iocsr_routes_ipi_enable_mask_and_clear() {
    let (ipi, _eiointc, bus) = make_bus(2);
    let sink = Arc::new(RecordingSink::default());
    let mut cpu0 = cpu_with_id(0);
    let mut cpu1 = cpu_with_id(1);
    bus.install_on(&mut cpu0);
    bus.install_on(&mut cpu1);
    ipi.connect_output(
        1,
        InterruptSource::new(Arc::clone(&sink) as Arc<dyn IrqSink>, 0),
    );

    iocsr_write(&mut cpu1, OP_IOCSRWR_W, 0x1004, 0);
    iocsr_write(&mut cpu0, OP_IOCSRWR_W, 0x1040, (1 << 16) | 4);
    assert_eq!(ipi.mmio_read_sized(1, 0x1000, 4), 1 << 4);
    assert!(!sink.line(0), "masked status must not raise CPU1 IPI line");

    iocsr_write(&mut cpu1, OP_IOCSRWR_W, 0x1004, 1 << 4);
    assert!(
        sink.line(0),
        "enabling a pending vector must raise CPU1 line"
    );

    iocsr_write(&mut cpu1, OP_IOCSRWR_W, 0x100c, 1 << 4);
    assert_eq!(ipi.mmio_read_sized(1, 0x1000, 4), 0);
    assert!(!sink.line(0), "clearing status must lower CPU1 line");
}

#[test]
fn task39_translated_iocsr_preserves_ipi_mailbox_widths() {
    let (ipi, _eiointc, bus) = make_bus(2);
    let mut cpu1 = cpu_with_id(1);
    bus.install_on(&mut cpu1);

    iocsr_write(&mut cpu1, OP_IOCSRWR_D, 0x1020, 0x1122_3344_5566_7788);
    assert_eq!(
        iocsr_read(&mut cpu1, OP_IOCSRRD_D, 0x1020),
        0x1122_3344_5566_7788
    );
    assert_eq!(iocsr_read(&mut cpu1, OP_IOCSRRD_B, 0x1021), 0x77);
    assert_eq!(iocsr_read(&mut cpu1, OP_IOCSRRD_H, 0x1022), 0x5566);
    assert_eq!(iocsr_read(&mut cpu1, OP_IOCSRRD_W, 0x1024), 0x1122_3344);

    iocsr_write(&mut cpu1, OP_IOCSRWR_B, 0x1021, 0xaa);
    iocsr_write(&mut cpu1, OP_IOCSRWR_H, 0x1022, 0xbbcc);
    iocsr_write(&mut cpu1, OP_IOCSRWR_W, 0x1024, 0xdead_beef);

    assert_eq!(
        iocsr_read(&mut cpu1, OP_IOCSRRD_D, 0x1020),
        0xdead_beef_bbcc_aa88
    );
    assert_eq!(ipi.mmio_read_sized(1, 0x1020, 8), 0xdead_beef_bbcc_aa88);
}

#[test]
fn task39_translated_iocsr_routes_mail_send_to_target_mailbox() {
    let (ipi, _eiointc, bus) = make_bus(2);
    let mut cpu0 = cpu_with_id(0);
    bus.install_on(&mut cpu0);
    ipi.mmio_write_sized(1, 0x1020, 4, 0xaaaa_bbbb);

    let data = 0x1122_3344u64;
    let byte_mask = 0x5u64;
    let val = (data << 32) | (byte_mask << 27) | (1 << 16);
    iocsr_write(&mut cpu0, OP_IOCSRWR_D, 0x1048, val);

    assert_eq!(ipi.mmio_read_sized(1, 0x1020, 4), 0x11aa_33bb);
}

#[test]
fn task39_translated_iocsr_routes_eiointc_programming_and_cpu_ack() {
    let (_ipi, eiointc, bus) = make_bus(2);
    let mut cpu0 = cpu_with_id(0);
    let mut cpu1 = cpu_with_id(1);
    bus.install_on(&mut cpu0);
    bus.install_on(&mut cpu1);

    iocsr_write(&mut cpu0, OP_IOCSRWR_W, 0x14c0, 0x0202_0202);
    iocsr_write(&mut cpu0, OP_IOCSRWR_W, 0x1c00, 0x0202_0202);
    iocsr_write(&mut cpu0, OP_IOCSRWR_W, 0x1600, 1 << 1);
    eiointc.set_irq(1, true);

    assert_eq!(iocsr_read(&mut cpu0, OP_IOCSRRD_W, 0x1800), 0);
    assert_eq!(iocsr_read(&mut cpu1, OP_IOCSRRD_W, 0x1800), 1 << 1);

    iocsr_write(&mut cpu0, OP_IOCSRWR_W, 0x1800, 1 << 1);
    assert_eq!(
        iocsr_read(&mut cpu1, OP_IOCSRRD_W, 0x1800),
        1 << 1,
        "CPU0 ack must not clear a CPU1-routed EIOINTC source"
    );

    iocsr_write(&mut cpu1, OP_IOCSRWR_W, 0x1800, 1 << 1);
    assert_eq!(iocsr_read(&mut cpu1, OP_IOCSRRD_W, 0x1800), 0);
    assert_eq!(eiointc.mmio_read_sized(0, 0x300, 4) & (1 << 1), 0);
}

#[test]
fn task39_translated_iocsr_without_dispatcher_keeps_cpu_local_fallback() {
    let mut cpu = cpu_with_id(0);

    iocsr_write(&mut cpu, OP_IOCSRWR_W, 0x1004, 1);
    iocsr_write(&mut cpu, OP_IOCSRWR_W, 0x1008, 1);
    assert_eq!(iocsr_read(&mut cpu, OP_IOCSRRD_W, 0x1000), 1);
    assert_eq!(cpu.csr_read(CSR_ESTAT) & IPI_LINE, IPI_LINE);

    iocsr_write(&mut cpu, OP_IOCSRWR_W, 0x100c, 1);
    iocsr_write(&mut cpu, OP_IOCSRWR_W, 0x1040, 1 << 16);
    assert_eq!(
        iocsr_read(&mut cpu, OP_IOCSRRD_W, 0x1000),
        0,
        "without a dispatcher, nonzero targets stay outside the CPU-local path"
    );
}
