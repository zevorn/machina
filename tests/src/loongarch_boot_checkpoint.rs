use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use flate2::write::GzEncoder;
use flate2::Compression;
use machina_accel::exec::{ExecEnv, ExitReason};
use machina_accel::ir::tb::EXCP_LOONGARCH_WFI;
use machina_accel::{ArchExitAction, GuestCpu, X86_64CodeGen};
use machina_core::address::GPA;
use machina_core::machine::{Machine, MachineOpts};
use machina_guest_loongarch::loongarch::cpu::{
    LoongArchCpu, GUEST_BASE_CPU_OFFSET, NEG_ALIGN_CPU_OFFSET,
};
use machina_guest_loongarch::loongarch::csr::{
    CRMD_DA, CRMD_IE, CSR_CRMD, CSR_ECFG, CSR_EENTRY, CSR_ESTAT, CSR_TCFG,
    CSR_TICLR,
};
use machina_hw_core::chardev::{CharFrontend, Chardev};
use machina_hw_loongarch::boot::KERNEL_ENTRY_DEFAULT;
use machina_hw_loongarch::virt_machine::{
    LoongArchVirtMachine, VIRT_UART_BASE,
};
use machina_system::loongarch_cpu::{
    loongarch_soft_mmu_config, LoongArchFullSystemCpu,
};
use machina_system::CpuManager;

const IDLE_OP: u32 = 0b00000110010010001;
const OP_LD_D: u32 = 0b0010100011;
const TIMER_INTERRUPT: u64 = 1 << 11;
const TASK48_CP4_BOOT_TIMEOUT: &str = "20s";
const TASK48_CP5_BOOT_TIMEOUT: &str = "120s";
const TASK48_CP6_BOOT_TIMEOUT: &str = "120s";

struct CaptureChardev {
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl Chardev for CaptureChardev {
    fn read(&mut self) -> Option<u8> {
        None
    }

    fn write(&mut self, data: u8) {
        self.bytes.lock().unwrap().push(data);
    }

    fn can_read(&self) -> bool {
        false
    }
}

fn default_opts() -> MachineOpts {
    MachineOpts {
        ram_size: 64 * 1024 * 1024,
        cpu_count: 1,
        kernel: None,
        bios: None,
        bios_builtin: false,
        append: None,
        nographic: false,
        drive: None,
        initrd: None,
        netdev: None,
    }
}

fn write_raw_kernel(insns: &[u32]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for insn in insns {
        file.write_all(&insn.to_le_bytes()).unwrap();
    }
    file.flush().unwrap();
    file
}

fn r2_si12(op: u32, si12: i16, rj: u32, rd: u32) -> u32 {
    (op << 22) | ((si12 as u16 as u32 & 0x0FFF) << 10) | (rj << 5) | rd
}

fn code15_insn(op: u32, code: u32) -> u32 {
    (op << 15) | (code & 0x7FFF)
}

fn write_image(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
    file
}

fn build_linux_image(entry: u64, load_offset: u64, payload: &[u8]) -> Vec<u8> {
    let mut image = vec![0u8; 64];
    image[0..2].copy_from_slice(&0x5a4du16.to_le_bytes());
    image[8..16].copy_from_slice(&entry.to_le_bytes());
    image[16..24].copy_from_slice(&((64 + payload.len()) as u64).to_le_bytes());
    image[24..32].copy_from_slice(&load_offset.to_le_bytes());
    image[56..60].copy_from_slice(&0x8182_23cdu32.to_le_bytes());
    image.extend_from_slice(payload);
    image
}

fn build_efi_zboot_image(payload: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(payload).unwrap();
    let compressed = encoder.finish().unwrap();
    let payload_offset = 64_u32;

    let mut image = vec![0u8; payload_offset as usize];
    image[0..2].copy_from_slice(b"MZ");
    image[4..8].copy_from_slice(b"zimg");
    image[8..12].copy_from_slice(&payload_offset.to_le_bytes());
    image[12..16].copy_from_slice(&(compressed.len() as u32).to_le_bytes());
    image[24..28].copy_from_slice(b"gzip");
    image[56..60].copy_from_slice(&0x8182_23cdu32.to_le_bytes());
    image[60..64].copy_from_slice(&64_u32.to_le_bytes());
    image.extend_from_slice(&compressed);
    image
}

#[test]
fn task47_direct_boot_starts_kernel_entry_in_da_mode_for_cp1() {
    let kernel = write_raw_kernel(&[IDLE_OP]);
    let mut opts = default_opts();
    opts.kernel = Some(kernel.path().to_path_buf());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(&opts).unwrap();
    machine.boot().unwrap();

    let cpu = machine.cpu();
    let cpu = cpu.lock().unwrap();
    assert_eq!(cpu.pc(), KERNEL_ENTRY_DEFAULT);
    assert_eq!(cpu.crmd() & CRMD_DA, CRMD_DA);
    assert_eq!(cpu.crmd() & CRMD_IE, 0);
}

#[test]
fn task47_direct_boot_accepts_efi_zboot_linux_image_for_cp1() {
    let payload =
        [IDLE_OP.to_le_bytes(), 0x0340_0000u32.to_le_bytes()].concat();
    let load_offset = 0x20_0000;
    let entry = KERNEL_ENTRY_DEFAULT + 64;
    let linux_image = build_linux_image(entry, load_offset, &payload);
    let zboot = build_efi_zboot_image(&linux_image);
    let kernel = write_image(&zboot);

    let mut opts = default_opts();
    opts.kernel = Some(kernel.path().to_path_buf());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(&opts).unwrap();
    machine.boot().unwrap();

    let cpu = machine.cpu();
    let cpu = cpu.lock().unwrap();
    assert_eq!(cpu.pc(), entry);
    drop(cpu);

    let loaded = machine.address_space().read(GPA::new(entry), 4);
    assert_eq!(loaded as u32, IDLE_OP);
}

#[test]
fn task47_loongarch_virt_uart_can_capture_console_output() {
    let output = Arc::new(Mutex::new(Vec::new()));
    let mut machine = LoongArchVirtMachine::new();
    machine
        .set_uart_chardev(CharFrontend::new(Box::new(CaptureChardev {
            bytes: Arc::clone(&output),
        })))
        .unwrap();
    machine.init(&default_opts()).unwrap();

    machine
        .address_space()
        .write(GPA::new(VIRT_UART_BASE), 1, u64::from(b'L'));
    machine.address_space().write(
        GPA::new(VIRT_UART_BASE),
        1,
        u64::from(b'\n'),
    );

    assert_eq!(&*output.lock().unwrap(), b"L\n");
}

#[test]
fn task47_loongarch_wfi_expires_enabled_timer_for_cp3() {
    let ram = [0u32; 4];
    let mut cpu = LoongArchCpu::new();
    cpu.set_pc(KERNEL_ENTRY_DEFAULT);
    cpu.csr_write(CSR_CRMD, CRMD_DA | CRMD_IE);
    cpu.csr_write(CSR_ECFG, TIMER_INTERRUPT);
    cpu.csr_write(CSR_EENTRY, KERNEL_ENTRY_DEFAULT + 0x1000);
    cpu.csr_write(CSR_TCFG, 0x0011);

    let mut sys = unsafe {
        LoongArchFullSystemCpu::new(
            cpu,
            ram.as_ptr().cast::<u8>(),
            0,
            (ram.len() * 4) as u64,
            0,
            Arc::new(AtomicBool::new(true)),
        )
    };

    let action = sys.handle_arch_exit(EXCP_LOONGARCH_WFI);

    assert_eq!(action, ArchExitAction::Continue);
    assert_eq!(
        sys.cpu.csr_read(CSR_ESTAT) & TIMER_INTERRUPT,
        TIMER_INTERRUPT
    );
    assert_eq!(sys.cpu.pc(), KERNEL_ENTRY_DEFAULT + 0x1000);
    assert_eq!(sys.cpu.crmd() & CRMD_IE, 0);
}

#[test]
fn task47_loongarch_runtime_tb_tick_expires_enabled_timer_for_cp3() {
    let ram = [0u32; 4];
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_TCFG, 0x0011);

    let mut sys = unsafe {
        LoongArchFullSystemCpu::new(
            cpu,
            ram.as_ptr().cast::<u8>(),
            0,
            (ram.len() * 4) as u64,
            0,
            Arc::new(AtomicBool::new(true)),
        )
    };

    assert!(!sys.check_mem_fault());
    assert_eq!(
        sys.cpu.csr_read(CSR_ESTAT) & TIMER_INTERRUPT,
        TIMER_INTERRUPT
    );
}

#[test]
fn task48_periodic_timer_does_not_refire_immediately_after_ticlr() {
    let ram = [0u32; 4];
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_TCFG, 0x0103);
    cpu.timer_tick(0x100);
    assert_eq!(cpu.csr_read(CSR_ESTAT) & TIMER_INTERRUPT, TIMER_INTERRUPT);
    cpu.csr_write(CSR_TICLR, 1);
    assert_eq!(cpu.csr_read(CSR_ESTAT) & TIMER_INTERRUPT, 0);

    let mut sys = unsafe {
        LoongArchFullSystemCpu::new(
            cpu,
            ram.as_ptr().cast::<u8>(),
            0,
            (ram.len() * 4) as u64,
            0,
            Arc::new(AtomicBool::new(true)),
        )
    };

    assert!(!sys.check_mem_fault());
    assert_eq!(
        sys.cpu.csr_read(CSR_ESTAT) & TIMER_INTERRUPT,
        0,
        "one runtime TB tick must not immediately retrigger a periodic timer"
    );

    for _ in 0..3 {
        assert!(!sys.check_mem_fault());
    }
    assert_eq!(
        sys.cpu.csr_read(CSR_ESTAT) & TIMER_INTERRUPT,
        TIMER_INTERRUPT,
        "periodic timer must refire after enough virtual cycles elapse"
    );
}

#[test]
fn task48_cp6_checkpoint_contract_requires_init_process_and_shell_prompt() {
    let needles = checkpoint_needles("cp6");
    assert!(needles.contains(&"Run /init as init process"));
    assert!(cp6_prompt_after_console(
        "Please press Enter to activate this console\n# "
    ));
    assert!(cp6_prompt_after_console(
        "Please press Enter to activate this console\n$ "
    ));
    assert!(!cp6_prompt_after_console(
        "# prompt before console\nPlease press Enter to activate this console\n"
    ));
    assert!(!cp6_prompt_after_console(
        "Please press Enter to activate this console\n[ log line ]"
    ));
}

#[test]
fn task47_loongarch_counting_timer_breaks_direct_chains_for_cp3() {
    let ram = [0u32; 4];
    let mut cpu = LoongArchCpu::new();
    cpu.csr_write(CSR_TCFG, 0x0011);

    let sys = unsafe {
        LoongArchFullSystemCpu::new(
            cpu,
            ram.as_ptr().cast::<u8>(),
            0,
            (ram.len() * 4) as u64,
            0,
            Arc::new(AtomicBool::new(true)),
        )
    };

    assert!(!sys.pending_interrupt());
    assert!(sys.has_pending_irq());
}

#[test]
fn task47_long_kernel_tb_does_not_overflow_spill_area_for_cp3() {
    let mut insns = vec![r2_si12(OP_LD_D, 0, 0, 1); 600];
    insns.push(code15_insn(IDLE_OP, 0));
    let kernel = write_raw_kernel(&insns);
    let mut opts = default_opts();
    opts.kernel = Some(kernel.path().to_path_buf());

    let mut machine = LoongArchVirtMachine::new();
    machine.init(&opts).unwrap();
    machine.boot().unwrap();

    let mut backend = X86_64CodeGen::new();
    backend.set_guest_base_offset(GUEST_BASE_CPU_OFFSET);
    backend.mmio = Some(loongarch_soft_mmu_config());
    backend.neg_align_off = i32::try_from(NEG_ALIGN_CPU_OFFSET).unwrap();
    let env = ExecEnv::new(backend);
    let shared = env.shared.clone();

    let mut manager = CpuManager::new();
    let stop_flag = manager.running_flag();
    let (cpu_state, interrupts) = machine.take_runtime_cpu_state().unwrap();
    let cpu = unsafe {
        LoongArchFullSystemCpu::new_with_interrupts(
            cpu_state,
            machine.ram_block().as_ptr(),
            0,
            opts.ram_size,
            machine.address_space() as *const _ as u64,
            Arc::clone(&stop_flag),
            interrupts,
        )
    };
    manager.add_loongarch_cpu(cpu);

    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            manager.run(&shared)
        }));

    assert_eq!(result.unwrap(), ExitReason::Halted);
}

#[test]
fn task48_boot_harness_runs_configured_linux_checkpoint() {
    let kernel = std::env::var_os("MACHINA_LOONGARCH_LINUX_IMAGE");
    let initrd = std::env::var_os("MACHINA_LOONGARCH_INITRD");

    if kernel.is_none() || initrd.is_none() {
        eprintln!(
            "task48: set MACHINA_LOONGARCH_LINUX_IMAGE and \
             MACHINA_LOONGARCH_INITRD to run the CP4-CP6 Linux boot harness"
        );
        return;
    }

    let kernel = workspace_path(PathBuf::from(kernel.unwrap()));
    let initrd = workspace_path(PathBuf::from(initrd.unwrap()));
    assert!(
        kernel.is_file(),
        "MACHINA_LOONGARCH_LINUX_IMAGE must point to a file"
    );
    assert!(
        initrd.is_file(),
        "MACHINA_LOONGARCH_INITRD must point to a file"
    );

    let checkpoint = std::env::var("MACHINA_LOONGARCH_BOOT_CHECKPOINT")
        .unwrap_or_else(|_| "cp4".to_string());
    let artifact_dir = std::env::var_os("MACHINA_LOONGARCH_ARTIFACT_DIR")
        .map(|path| workspace_path(PathBuf::from(path)))
        .unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&artifact_dir).unwrap();
    let stdout_path = artifact_dir.join("task48-linux-boot.stdout");
    let stderr_path = artifact_dir.join("task48-linux-boot.stderr");
    let trace_path = artifact_dir.join("task48-linux-boot.trace");
    let status_path = artifact_dir.join("task48-linux-boot.status");
    let command_path = artifact_dir.join("task48-linux-boot.command");
    let timeout = std::env::var("MACHINA_LOONGARCH_BOOT_TIMEOUT")
        .unwrap_or_else(|_| default_boot_timeout(&checkpoint).to_string());
    let ram_mb = std::env::var("MACHINA_LOONGARCH_BOOT_RAM_MB")
        .unwrap_or_else(|_| "256".to_string());
    let mut append =
        "console=ttyS0 earlycon=uart8250,mmio,0x1fe001e0 lpj=1000000 \
         rdinit=/init"
            .to_string();
    if let Ok(extra) = std::env::var("MACHINA_LOONGARCH_BOOT_APPEND_EXTRA") {
        if !extra.trim().is_empty() {
            append.push(' ');
            append.push_str(extra.trim());
        }
    }
    let command_summary = format!(
        "checkpoint={checkpoint}\ntimeout={timeout}\nram_mb={ram_mb}\n\
         kernel={}\ninitrd={}\nappend={append}\nstdout={}\nstderr={}\n\
         trace={}\nstatus={}\n",
        kernel.display(),
        initrd.display(),
        stdout_path.display(),
        stderr_path.display(),
        trace_path.display(),
        status_path.display()
    );
    std::fs::write(&command_path, command_summary).unwrap();

    let output = Command::new("timeout")
        .arg(&timeout)
        .arg(machina_binary())
        .arg("-M")
        .arg("loongarch64-virt")
        .arg("-m")
        .arg(ram_mb)
        .arg("-kernel")
        .arg(&kernel)
        .arg("-initrd")
        .arg(&initrd)
        .arg("-append")
        .arg(&append)
        .arg("-nographic")
        .arg("--trace")
        .arg(&trace_path)
        .output()
        .expect("run machina loongarch boot harness");
    std::fs::write(&stdout_path, &output.stdout).unwrap();
    std::fs::write(&stderr_path, &output.stderr).unwrap();
    std::fs::write(
        &status_path,
        format!(
            "success={}\ncode={:?}\nstatus={:?}\n",
            output.status.success(),
            output.status.code(),
            output.status
        ),
    )
    .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.code() == Some(124) || output.status.success(),
        "boot harness exited with {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        stdout,
        stderr
    );
    assert!(
        !stdout.contains("OF: of_irq_init: children remain, but no parents"),
        "CP4 IRQ topology must not leave orphan interrupt controllers; \
         artifacts: stdout={}, stderr={}, trace={}",
        stdout_path.display(),
        stderr_path.display(),
        trace_path.display()
    );

    assert_checkpoint_stdout(
        &checkpoint,
        &stdout,
        &stdout_path,
        &stderr_path,
        &trace_path,
    );
}

fn machina_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_machina") {
        return PathBuf::from(path);
    }
    workspace_path(PathBuf::from("target/debug/machina"))
}

fn workspace_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(path)
    }
}

fn default_boot_timeout(checkpoint: &str) -> &'static str {
    match checkpoint {
        "cp4" => TASK48_CP4_BOOT_TIMEOUT,
        "cp5" => TASK48_CP5_BOOT_TIMEOUT,
        "cp6" => TASK48_CP6_BOOT_TIMEOUT,
        other => panic!("unknown MACHINA_LOONGARCH_BOOT_CHECKPOINT={other}"),
    }
}

fn assert_checkpoint_stdout(
    checkpoint: &str,
    stdout: &str,
    stdout_path: &std::path::Path,
    stderr_path: &std::path::Path,
    trace_path: &std::path::Path,
) {
    for needle in checkpoint_needles(checkpoint) {
        assert!(
            stdout.contains(needle),
            "checkpoint {checkpoint} missing '{needle}'; artifacts: \
             stdout={}, stderr={}, trace={}",
            stdout_path.display(),
            stderr_path.display(),
            trace_path.display()
        );
    }

    if checkpoint == "cp6" {
        assert!(
            cp6_prompt_after_console(stdout),
            "checkpoint cp6 reached console activation but not a shell prompt; \
             artifacts: stdout={}, stderr={}, trace={}",
            stdout_path.display(),
            stderr_path.display(),
            trace_path.display()
        );
    }
}

fn cp6_prompt_after_console(stdout: &str) -> bool {
    let marker = "Please press Enter to activate this console";
    let Some(pos) = stdout.find(marker) else {
        return false;
    };
    let after_console = &stdout[pos + marker.len()..];
    after_console.contains("\n#") || after_console.contains("\n$")
}

fn checkpoint_needles(checkpoint: &str) -> &'static [&'static str] {
    match checkpoint {
        "cp4" => &[
            "Linux version",
            "Kernel command line:",
            "NR_IRQS:",
            "Constant clock source device register",
        ],
        "cp5" => &[
            "Linux version",
            "Kernel command line:",
            "Trying to unpack rootfs image as initramfs",
            "Freeing initrd memory",
        ],
        "cp6" => &[
            "Linux version",
            "Trying to unpack rootfs image as initramfs",
            "Freeing initrd memory",
            "Run /init as init process",
            "Please press Enter to activate this console",
        ],
        other => panic!("unknown MACHINA_LOONGARCH_BOOT_CHECKPOINT={other}"),
    }
}
