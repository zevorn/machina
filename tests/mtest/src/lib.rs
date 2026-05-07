#[cfg(test)]
mod tests {
    use machina_core::machine::{LoaderSpec, Machine, MachineOpts};
    use machina_hw_riscv::k230::K230Machine;

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
    fn k230_dtb_fixup_disables_sdk_sdhci_nodes() {
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
            Some("disabled".to_string()),
        );
        assert_eq!(
            machina_hw_riscv::k230_dtb::dtb_node_status(
                &blob,
                "/soc/sdhci1@91581000"
            )
            .unwrap(),
            Some("disabled".to_string()),
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
