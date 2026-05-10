#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::path::{Path, PathBuf};

    use flate2::read::GzDecoder;
    use machina_core::machine::{LoaderSpec, Machine, MachineOpts};
    use machina_guest_riscv::riscv::cpu::RiscvCpu;
    use machina_guest_riscv::riscv::csr::{CSR_PMPADDR0, CSR_PMPCFG0};
    use machina_hw_riscv::k230::K230Machine;
    use machina_hw_riscv::k230_boot::K230_BOOTROM_BASE;

    #[test]
    fn loader_spec_parses_qemu_loader_syntax() {
        let spec = LoaderSpec::parse(
            "loader,file=/tmp/fw.uImage,addr=0x0c100000,force-raw=on",
        )
        .unwrap();
        assert_eq!(spec.file.to_str(), Some("/tmp/fw.uImage"));
        assert_eq!(spec.addr, 0x0c10_0000);
        assert!(spec.force_raw);
    }

    #[test]
    fn loader_spec_rejects_missing_file() {
        let err =
            LoaderSpec::parse("loader,addr=0x1000,force-raw=on").unwrap_err();
        assert!(err.contains("missing file="));
    }

    fn k230_opts() -> MachineOpts {
        MachineOpts {
            ram_size: 0x8000_0000,
            cpu_count: 1,
            kernel: None,
            bios: Some("none".into()),
            bios_builtin: false,
            append: None,
            nographic: true,
            drive: None,
            initrd: None,
            dtb: None,
            loaders: Vec::new(),
            netdev: None,
        }
    }

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn bundled_k230_linux_dir() -> PathBuf {
        repo_root().join("pc-bios/k230-linux")
    }

    fn read_prefix(path: &Path, len: usize) -> Vec<u8> {
        let mut file = std::fs::File::open(path).unwrap();
        let mut data = vec![0; len];
        file.read_exact(&mut data).unwrap();
        data
    }

    fn decompress_gzip_to(src: &Path, dst: &Path) -> u64 {
        let input = std::fs::File::open(src).unwrap();
        let mut decoder = GzDecoder::new(input);
        let mut output = std::fs::File::create(dst).unwrap();
        std::io::copy(&mut decoder, &mut output).unwrap()
    }

    fn align_up_4k(value: u64) -> u64 {
        (value + 0xfff) & !0xfff
    }

    fn minimal_exec_elf(
        entry: u64,
        p_vaddr: u64,
        p_paddr: u64,
        p_filesz: u64,
        p_memsz: u64,
    ) -> Vec<u8> {
        let mut elf = vec![0u8; 64 + 56];
        elf[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        elf[4] = 2;
        elf[5] = 1;
        elf[6] = 1;
        elf[16..18].copy_from_slice(&2u16.to_le_bytes());
        elf[20..24].copy_from_slice(&1u32.to_le_bytes());
        elf[24..32].copy_from_slice(&entry.to_le_bytes());
        elf[32..40].copy_from_slice(&64u64.to_le_bytes());
        elf[52..54].copy_from_slice(&64u16.to_le_bytes());
        elf[54..56].copy_from_slice(&56u16.to_le_bytes());
        elf[56..58].copy_from_slice(&1u16.to_le_bytes());

        let ph = 64usize;
        elf[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
        let p_offset: u64 = 120;
        elf[ph + 8..ph + 16].copy_from_slice(&p_offset.to_le_bytes());
        elf[ph + 16..ph + 24].copy_from_slice(&p_vaddr.to_le_bytes());
        elf[ph + 24..ph + 32].copy_from_slice(&p_paddr.to_le_bytes());
        elf[ph + 32..ph + 40].copy_from_slice(&p_filesz.to_le_bytes());
        elf[ph + 40..ph + 48].copy_from_slice(&p_memsz.to_le_bytes());
        elf.extend(std::iter::repeat(0x13).take(p_filesz as usize));
        elf
    }

    #[test]
    fn k230_direct_boot_rejects_initrd_without_dtb() {
        let dir = tempfile::tempdir().unwrap();
        let initrd = dir.path().join("rootfs.cpio.gz");
        std::fs::write(&initrd, b"initrd").unwrap();

        let mut machine = K230Machine::new();
        let opts = MachineOpts {
            initrd: Some(initrd),
            ..k230_opts()
        };
        machine.init(&opts).unwrap();

        let err = machine.boot().unwrap_err().to_string();
        assert!(err.contains("-initrd requires -dtb for the k230 machine"));
    }

    #[test]
    fn k230_bundled_linux_artifacts_boot_with_builtin_sbi() {
        let dir = bundled_k230_linux_dir();
        let image = dir.join("Image");
        let dtb = dir.join("k230.dtb");
        let initrd = dir.join("rootfs.cpio.gz");

        assert!(image.is_file(), "missing {}", image.display());
        assert!(dtb.is_file(), "missing {}", dtb.display());
        assert!(initrd.is_file(), "missing {}", initrd.display());

        let dtb_blob = std::fs::read(&dtb).unwrap();
        let linux_mem =
            machina_hw_riscv::k230_dtb::dtb_first_memory_region(&dtb_blob)
                .unwrap()
                .unwrap();
        let kernel_size = std::fs::metadata(&image).unwrap().len();
        let initrd_base = align_up_4k(
            linux_mem
                .base
                .checked_add((linux_mem.size / 2).min(512 * 1024 * 1024))
                .unwrap()
                .max(linux_mem.base + kernel_size)
                .max(linux_mem.base),
        );

        let mut machine = K230Machine::new();
        let opts = MachineOpts {
            kernel: Some(image.clone()),
            dtb: Some(dtb),
            initrd: Some(initrd.clone()),
            bios: None,
            bios_builtin: true,
            append: Some(
                "console=ttyS0,115200 earlycon=sbi cma=0 norandmaps".into(),
            ),
            ..k230_opts()
        };
        machine.init(&opts).unwrap();
        machine.boot().unwrap();

        let cpus = machine.cpus_lock();
        let cpu = cpus[0].as_ref().unwrap();
        assert_eq!(cpu.pc, linux_mem.base);
        assert_eq!(cpu.gpr[10], 0);
        let fdt_addr = cpu.gpr[11];
        assert_ne!(fdt_addr, 0);
        drop(cpus);

        assert_eq!(
            machine.read_ram_bytes(linux_mem.base, 4).unwrap(),
            read_prefix(&image, 4)
        );
        assert_eq!(
            machine.read_ram_bytes(initrd_base, 4).unwrap(),
            read_prefix(&initrd, 4)
        );
        assert_eq!(
            machine.read_ram_bytes(fdt_addr, 4).unwrap(),
            vec![0xd0, 0x0d, 0xfe, 0xed]
        );
    }

    #[test]
    fn k230_bundled_uboot_sd_artifacts_initialize_sdk_boot_path() {
        let dir = bundled_k230_linux_dir();
        let uboot = dir.join("u-boot");
        let sd_gz = dir.join("sysimage-sdcard.img.gz");

        assert!(uboot.is_file(), "missing {}", uboot.display());
        assert!(sd_gz.is_file(), "missing {}", sd_gz.display());

        let temp = tempfile::tempdir().unwrap();
        let sd = temp.path().join("sysimage-sdcard.img");
        let raw_len = decompress_gzip_to(&sd_gz, &sd);
        assert!(raw_len >= 512 * 1024 * 1024);

        let mut machine = K230Machine::new();
        let opts = MachineOpts {
            bios: Some(uboot),
            drive: Some(sd),
            ..k230_opts()
        };
        machine.init(&opts).unwrap();
        machine.boot().unwrap();

        let cpus = machine.cpus_lock();
        let cpu = cpus[0].as_ref().unwrap();
        assert_eq!(cpu.pc, K230_BOOTROM_BASE);
    }

    #[test]
    fn k230_linux_handoff_keeps_locked_sdk_pmp_windows() {
        let mut cpu = RiscvCpu::new();

        cpu.csr_write(CSR_PMPADDR0, 0x2448_4dff);
        cpu.csr_write(CSR_PMPADDR0 + 1, 0x2448_51ff);
        cpu.csr_write(CSR_PMPCFG0, 0x9999);

        cpu.csr_write(CSR_PMPADDR0, 0x003f_ffff_ffff_ffff);

        assert_eq!(cpu.csr_read(CSR_PMPADDR0), 0x2448_4dff);
        assert_eq!(cpu.csr_read(CSR_PMPADDR0 + 1), 0x2448_51ff);
        assert_eq!(cpu.csr_read(CSR_PMPCFG0) & 0xffff, 0x9999);
    }

    #[test]
    fn k230_builtin_boot_places_dtb_below_initrd_when_top_overlaps() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("Image");
        let initrd = dir.path().join("rootfs.cpio.gz");
        let dtb = dir.path().join("k230.dtb");
        let mem_base = 0;
        let mem_size = 0x4_0000;
        let initrd_start = 0x2_0000;
        let initrd_tail = [0xaa, 0xbb, 0xcc, 0xdd];
        let mut initrd_blob = vec![0x5a; mem_size as usize / 2];

        initrd_blob[mem_size as usize / 2 - 4..].copy_from_slice(&initrd_tail);
        std::fs::write(&image, [0x13, 0x00, 0x00, 0x00]).unwrap();
        std::fs::write(&initrd, &initrd_blob).unwrap();
        std::fs::write(
            &dtb,
            machina_hw_riscv::k230_dtb::test_fixture_dtb_with_memory_region(
                mem_base, mem_size,
            ),
        )
        .unwrap();

        let mut machine = K230Machine::new();
        let opts = MachineOpts {
            kernel: Some(image),
            dtb: Some(dtb),
            initrd: Some(initrd),
            bios: None,
            bios_builtin: true,
            ..k230_opts()
        };
        machine.init(&opts).unwrap();
        machine.boot().unwrap();

        let cpus = machine.cpus_lock();
        let fdt_addr = cpus[0].as_ref().unwrap().gpr[11];
        drop(cpus);

        assert!(fdt_addr < initrd_start);
        assert_eq!(
            machine.read_ram_bytes(fdt_addr, 4).unwrap(),
            vec![0xd0, 0x0d, 0xfe, 0xed]
        );
        assert_eq!(
            machine.read_ram_bytes(mem_size - 4, 4).unwrap(),
            initrd_tail
        );
    }

    #[test]
    fn k230_builtin_boot_uses_elf_low_load_addr_for_initrd_layout() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("Image");
        let initrd = dir.path().join("rootfs.cpio.gz");
        let dtb = dir.path().join("k230.dtb");
        let mem_base = 0;
        let mem_size = 0x4_0000;
        let initrd_base = 0x3_2000;
        let initrd_blob = [0xaa, 0xbb, 0xcc, 0xdd];

        std::fs::write(
            &image,
            minimal_exec_elf(0x3_0000, 0x1000, 0x1000, 4, 0x3_0004),
        )
        .unwrap();
        std::fs::write(&initrd, initrd_blob).unwrap();
        std::fs::write(
            &dtb,
            machina_hw_riscv::k230_dtb::test_fixture_dtb_with_memory_region(
                mem_base, mem_size,
            ),
        )
        .unwrap();

        let mut machine = K230Machine::new();
        let opts = MachineOpts {
            kernel: Some(image),
            dtb: Some(dtb),
            initrd: Some(initrd),
            bios: None,
            bios_builtin: true,
            ..k230_opts()
        };
        machine.init(&opts).unwrap();
        machine.boot().unwrap();

        assert_eq!(
            machine
                .read_ram_bytes(initrd_base, initrd_blob.len())
                .unwrap(),
            initrd_blob
        );
    }

    #[test]
    fn k230_dtb_fixup_preserves_sdk_sdhci_nodes() {
        let mut blob =
            machina_hw_riscv::k230_dtb::test_fixture_dtb_with_sdhci_nodes();
        blob = machina_hw_riscv::k230_dtb::fixup_k230_dtb(
            &blob,
            Some((0x0a10_0000, 0x0a20_0000)),
            Some("console=ttyS0,115200 earlycon=sbi cma=0"),
        )
        .unwrap();

        assert_eq!(
            machina_hw_riscv::k230_dtb::dtb_node_status(
                &blob,
                "/soc/sdhci0@91580000"
            )
            .unwrap(),
            Some("okay".to_string()),
        );
        assert_eq!(
            machina_hw_riscv::k230_dtb::dtb_node_status(
                &blob,
                "/soc/sdhci1@91581000"
            )
            .unwrap(),
            Some("okay".to_string()),
        );
        assert!(blob
            .windows(b"console=ttyS0,115200 earlycon=sbi cma=0".len())
            .any(|w| w == b"console=ttyS0,115200 earlycon=sbi cma=0"));
    }

    #[test]
    fn k230_dtb_fixup_preserves_bootargs_without_append() {
        let bootargs = "console=ttyS0 root=/dev/mmcblk0p2";
        let mut blob =
            machina_hw_riscv::k230_dtb::test_fixture_dtb_with_sdhci_nodes_and_bootargs(
                bootargs,
            );
        blob = machina_hw_riscv::k230_dtb::fixup_k230_dtb(
            &blob,
            Some((0x0a10_0000, 0x0a20_0000)),
            None,
        )
        .unwrap();

        assert_eq!(
            machina_hw_riscv::k230_dtb::dtb_chosen_bootargs(&blob).unwrap(),
            Some(bootargs.to_string()),
        );
    }

    #[test]
    fn k230_dtb_fixup_preserves_reservation_map() {
        let reservations = [
            machina_hw_riscv::k230_dtb::FdtReservation {
                address: 0x8000_0000,
                size: 0x20_0000,
            },
            machina_hw_riscv::k230_dtb::FdtReservation {
                address: 0x0a10_0000,
                size: 0x1000,
            },
        ];
        let mut blob =
            machina_hw_riscv::k230_dtb::test_fixture_dtb_with_sdhci_nodes_bootargs_and_reservations(
                "console=ttyS0",
                &reservations,
            );
        blob = machina_hw_riscv::k230_dtb::fixup_k230_dtb(
            &blob,
            Some((0x0a10_0000, 0x0a20_0000)),
            Some("console=ttyS0,115200 earlycon=sbi cma=0"),
        )
        .unwrap();

        assert_eq!(
            machina_hw_riscv::k230_dtb::dtb_mem_reservations(&blob).unwrap(),
            reservations,
        );
    }

    #[test]
    fn k230_loader_boot_places_sdk_payloads() {
        let dir = tempfile::tempdir().unwrap();
        let fw = dir.path().join("fw.uImage");
        let image = dir.path().join("Image");
        let initrd = dir.path().join("rootfs.cpio.gz");
        let dtb = dir.path().join("k230.dtb");
        std::fs::write(&fw, [0x11, 0x22, 0x33, 0x44]).unwrap();
        std::fs::write(&image, [0x55, 0x66, 0x77, 0x88]).unwrap();
        std::fs::write(&initrd, [0xaa, 0xbb, 0xcc, 0xdd]).unwrap();
        std::fs::write(&dtb, [0xde, 0xad, 0xbe, 0xef]).unwrap();

        let mut machine = K230Machine::new();
        let opts = MachineOpts {
            loaders: vec![
                LoaderSpec {
                    file: fw,
                    addr: 0x0c10_0000,
                    force_raw: true,
                },
                LoaderSpec {
                    file: image,
                    addr: 0x0820_0000,
                    force_raw: true,
                },
                LoaderSpec {
                    file: initrd,
                    addr: 0x0a10_0000,
                    force_raw: true,
                },
                LoaderSpec {
                    file: dtb,
                    addr: 0x0a00_0000,
                    force_raw: true,
                },
            ],
            ..k230_opts()
        };
        machine.init(&opts).unwrap();
        machine.boot().unwrap();

        assert_eq!(
            machine.read_ram_bytes(0x0c10_0000, 4).unwrap(),
            vec![0x11, 0x22, 0x33, 0x44]
        );
        assert_eq!(
            machine.read_ram_bytes(0x0820_0000, 4).unwrap(),
            vec![0x55, 0x66, 0x77, 0x88]
        );
        assert_eq!(
            machine.read_ram_bytes(0x0a10_0000, 4).unwrap(),
            vec![0xaa, 0xbb, 0xcc, 0xdd]
        );
        assert_eq!(
            machine.read_ram_bytes(0x0a00_0000, 4).unwrap(),
            vec![0xde, 0xad, 0xbe, 0xef]
        );
    }

    #[test]
    fn k230_loader_boot_rejects_overflowing_payload_range() {
        let dir = tempfile::tempdir().unwrap();
        let fw = dir.path().join("fw.uImage");
        std::fs::write(&fw, [0x11, 0x22, 0x33, 0x44]).unwrap();

        let mut machine = K230Machine::new();
        let opts = MachineOpts {
            loaders: vec![LoaderSpec {
                file: fw,
                addr: u64::MAX - 1,
                force_raw: true,
            }],
            ..k230_opts()
        };
        machine.init(&opts).unwrap();

        let err = machine.boot().unwrap_err().to_string();
        assert!(err.contains("k230 loader range end overflows u64"));
    }

    fn qemu_system_riscv64() -> Option<String> {
        machina_oracle::qemu::find_qemu("riscv64")
    }

    #[test]
    fn qemu_k230_wdt_register_mask_slice_matches_machina() {
        use machina_hw_watchdog::k230::{
            K230Wdt, K230WdtMmio, CR, PROT_LEVEL, TORR,
        };
        use machina_memory::region::MmioOps;
        use machina_oracle::qemu::QemuProbe;

        let Some(qemu) = qemu_system_riscv64() else {
            eprintln!("skip: qemu-system-riscv64 not found");
            return;
        };

        let extra = vec![
            "-accel".to_string(),
            "qtest".to_string(),
            "-bios".to_string(),
            "none".to_string(),
        ];
        let mut probe = match QemuProbe::spawn(&qemu, "k230", &extra) {
            Ok(probe) => probe,
            Err(err) => {
                eprintln!("skip: cannot start QEMU k230 qtest slice: {err}");
                return;
            }
        };

        let base = 0x9110_6000;
        probe.send_write(base + CR, 4, u64::MAX).unwrap();
        probe.send_read(base + CR, 4).unwrap();
        probe.send_write(base + TORR, 4, u64::MAX).unwrap();
        probe.send_read(base + TORR, 4).unwrap();
        probe.send_write(base + PROT_LEVEL, 4, u64::MAX).unwrap();
        probe.send_read(base + PROT_LEVEL, 4).unwrap();

        let qemu_values = match probe.finish() {
            Ok(values) if values.len() == 3 => values,
            Ok(values) => {
                eprintln!(
                    "skip: unexpected QEMU k230 qtest response count: {}",
                    values.len()
                );
                return;
            }
            Err(err) => {
                eprintln!("skip: QEMU k230 qtest slice unavailable: {err}");
                return;
            }
        };

        let wdt = K230Wdt::new_named("k230-wdt0");
        let mmio = K230WdtMmio(wdt);
        mmio.write(CR, 4, u64::MAX);
        let machina_cr = mmio.read(CR, 4);
        mmio.write(TORR, 4, u64::MAX);
        let machina_torr = mmio.read(TORR, 4);
        mmio.write(PROT_LEVEL, 4, u64::MAX);
        let machina_prot = mmio.read(PROT_LEVEL, 4);

        assert_eq!(qemu_values, vec![machina_cr, machina_torr, machina_prot]);
    }

    #[test]
    fn k230_sdk_boot_artifacts_are_discovered_for_opt_in_smoke() {
        let Some(sdk) =
            std::env::var_os("MACHINA_K230_SDK").map(std::path::PathBuf::from)
        else {
            eprintln!("skip: MACHINA_K230_SDK not set");
            return;
        };

        assert!(sdk.join("images/little-core/Image").is_file());
        assert!(sdk.join("images/little-core/k230.dtb").is_file());
        assert!(sdk.join("images/little-core/rootfs.cpio.gz").is_file());
        assert!(sdk.join("images/little-core/fw_jump.bin").is_file());
        assert!(sdk.join("little/uboot/u-boot").is_file());
    }
}
