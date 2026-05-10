# Getting Started with Machina

> Target audience: developers who want to build, run, and boot
> guest software on machina.

## First-time Contributor Walk-through

This path uses only the firmware payloads bundled in
`tests/firmware/`, so it works on a fresh clone with no external
kernel, disk image, or RISC-V cross toolchain. Run each step in
order; if a step fails see [Troubleshooting](#troubleshooting).

### 1. Install host dependencies

- A current stable Rust toolchain (`rustc 1.80+`) plus `cargo`,
  installed via [`rustup`](https://rustup.rs/).
- GNU `make`.
- A working host C linker (`cc`) — Cargo invokes it for the
  release build.

### 2. Clone and build

```bash
git clone https://github.com/gevico/machina.git
cd machina
make release
```

The first build also pulls workspace dependencies (~1–3 minutes
on a recent laptop). The output binary is at
`./target/release/machina`.

### 3. Smoke-boot a bundled payload

The cheapest end-to-end check — boots the bundled bare-metal
"PASS" kernel against `riscv64-ref` and exits with code 0:

```bash
./target/release/machina -M riscv64-ref -m 128 -bios none \
    -kernel tests/firmware/sifive_pass.bin -nographic
```

Expected output (the `shutdown (pass)` line is the success
marker):

```
machina: riscv64-ref, 128 MiB RAM
machina: entering execution loop
machina: shutdown (pass)
```

For a longer smoke test that exercises the bundled RustSBI
firmware path, run:

```bash
./target/release/machina -M riscv64-ref -m 128 \
    -kernel tests/firmware/sbi_smoke.bin -nographic
```

Expected output begins with the RustSBI banner and ends with
`MACHINA_SBI_OK`, after which Machina exits cleanly.

### 4. Run the narrow tests for the area you are touching

Always start with a focused filter so iteration is fast — `make
test` runs the full suite (slow). Examples:

```bash
# Smoke tests for the boot tooling itself.
cargo test -p machina-tests tools::

# Disassembler regressions.
cargo test -p machina-tests disas

# Memory-region / FlatView tests.
cargo test -p machina-tests memory_region

# RISC-V CSR semantics.
cargo test -p machina-tests riscv_csr
```

### 5. Pre-PR checks

Before opening a pull request, the same checks CI runs:

```bash
make fmt-check    # rustfmt diff must be empty
make clippy       # zero clippy warnings
make test         # full test suite
```

If `.agents/` was modified, also run `make check-agent-skills`.

### Troubleshooting

| Symptom | Likely cause / fix |
|---------|--------------------|
| `make release` fails on `cc not found` | Install the host build essentials (`build-essential` on Debian/Ubuntu, Xcode CLT on macOS, `gcc` on Fedora/Arch). |
| `tests/firmware/*.bin` missing | The repo ships pre-built binaries. If you removed them, rebuild with `cd tests/firmware && ./build.sh`, which needs a `riscv64-elf-` cross toolchain. |
| Smoke command hangs | Make sure you used `tests/firmware/sifive_pass.bin` with `-bios none`. The plain `sifive_pass` payload is a bare-metal kernel and will not work without `-bios none`. |
| `make test` takes a long time | This is expected — the suite is large. While iterating, prefer `cargo test -p machina-tests <filter>` and only run `make test` before pushing. |
| `info registers` says "VM must be paused" | You are connected to a running guest. Send `stop` (HMP) or `{"execute":"stop"}` (QMP) first, then retry the query. |

## Quick Start

### Build

```bash
git clone https://github.com/gevico/machina.git
cd machina
make release
```

### Run

```bash
# Boot a kernel
./target/release/machina -nographic -bios none \
    -kernel path/to/kernel.elf

# With VirtIO block device
./target/release/machina -nographic \
    -drive file=path/to/disk.img \
    -kernel path/to/kernel.elf

# With monitor console (QMP over TCP)
./target/release/machina -nographic \
    -monitor tcp:127.0.0.1:4444 \
    -bios none -kernel path/to/kernel.elf
```

## Build Commands

| Command | Description |
|---------|-------------|
| `make build` | Build all crates (debug) |
| `make release` | Build all crates (release) |
| `make test` | Run all tests |
| `make clippy` | Lint with `-D warnings` |
| `make fmt` | Auto-format all code |

## Machine Types

Machina currently exposes three user-facing machines:

| Machine | Guest ISA | Purpose |
|---------|-----------|---------|
| `riscv64-ref` | RISC-V 64-bit | RISC-V reference platform with SBI, PLIC, ACLINT, UART, and VirtIO MMIO |
| `k230` | RISC-V 64-bit | Kendryte K230 SDK-compatible platform with C908 CPU profile, PLIC, ACLINT, UARTs, WDTs, direct Linux boot with `-dtb`, and SDK U-Boot loader boot |
| `loongarch64-ref` | LoongArch64 | LoongArch64 reference platform with direct Linux boot, IOCSR, IPI, EIOINTC, PCH-PIC, UART, and VirtIO block |

List supported machines with:

```bash
./target/release/machina -M ?
```

## Booting K230 SDK Linux

The `k230` machine follows QEMU's SDK-compatible K230 boot contract: Machina
does not synthesize a K230 device tree. Linux direct boot requires the SDK DTB
so firmware can pass the board topology to Linux and Machina can update
`/chosen` for `-append` and `-initrd`.

Build the SDK Linux kernel with standard RISC-V PTE bits. Current Machina K230
support intentionally does not model T-HEAD MAEE page attributes yet, matching
the QEMU K230 path used as the oracle. In a Kendryte SDK tree, pass
`-DQEMU_NO_THEAD_MAEE` when rebuilding the little-core Linux kernel:

```bash
cd ~/k230_sdk
make CONF=k230_canmv_defconfig linux-clean
make CONF=k230_canmv_defconfig \
    KCFLAGS="-DDBGLV=0 -DQEMU_NO_THEAD_MAEE" \
    linux-rebuild
cp output/k230_canmv_defconfig/little/linux/arch/riscv/boot/Image \
   output/k230_canmv_defconfig/images/little-core/Image
```

Direct Linux boot uses the SDK `Image`, `k230.dtb`, and initramfs:

```bash
SDK=~/k230_sdk/output/k230_canmv_defconfig
./target/release/machina -M k230 \
    -kernel "$SDK/images/little-core/Image" \
    -dtb "$SDK/images/little-core/k230.dtb" \
    -initrd "$SDK/images/little-core/rootfs.cpio.gz" \
    -append "console=ttyS0,115200 earlycon=sbi cma=0" \
    -nographic
```

The SDK U-Boot flow starts U-Boot in M-mode with `-bios`. Until the SDK
storage path is modeled, place OpenSBI, Linux, initrd, and DTB in RAM with
loader devices and run `bootm` manually from U-Boot:

```bash
SDK=~/k230_sdk/output/k230_canmv_defconfig
IMAGE=$SDK/images/little-core/Image
INITRD=$SDK/images/little-core/rootfs.cpio.gz
DTB=$SDK/images/little-core/k230.dtb
FWJUMP_UIMAGE=/tmp/k230-fw-jump.uImage

"$SDK/little/buildroot-ext/host/bin/mkimage" \
    -A riscv -O linux -T kernel -C none \
    -a 0x08000000 -e 0x08000000 -n opensbi \
    -d "$SDK/images/little-core/fw_jump.bin" "$FWJUMP_UIMAGE"

./target/release/machina -M k230 \
    -bios "$SDK/little/uboot/u-boot" \
    -device loader,file="$FWJUMP_UIMAGE",addr=0x0c100000,force-raw=on \
    -device loader,file="$IMAGE",addr=0x08200000,force-raw=on \
    -device loader,file="$INITRD",addr=0x0a100000,force-raw=on \
    -device loader,file="$DTB",addr=0x0a000000,force-raw=on \
    -nographic
```

## Booting RISC-V Linux on Machina

This section describes how to boot a standard RISC-V Linux
kernel on the machina `riscv64-ref` platform.

### Prerequisites

| Component | Version | Notes |
|-----------|---------|-------|
| Rust toolchain | stable 1.80+ | `cargo build --release` |
| RISC-V Linux kernel | 6.12+ | flat `Image` format |
| SBI firmware | OpenSBI 1.4+ **or** embedded RustSBI | see below |
| Root filesystem | initramfs (cpio.gz) | busybox recommended |
| Cross toolchain | riscv64-linux-gnu-gcc | for building kernel |

### Linux Boot Quick Start

```bash
# 1. Build machina
cargo build --release

# 2. Boot with system OpenSBI + initramfs
./target/release/machina \
    -nographic -m 256 \
    -bios /usr/share/qemu/opensbi-riscv64-generic-fw_dynamic.bin \
    -kernel /path/to/Image \
    -initrd /path/to/rootfs.cpio.gz \
    -append "earlycon=ns16550a,mmio,0x10000000 console=ttyS0 root=/dev/ram rdinit=/sbin/init"
```

Expected output (abbreviated):

```
OpenSBI v1.5.1
...
Boot HART Base ISA        : rv64imafdc
...
Linux version 6.12.51 ...
...
Please press Enter to activate this console.
```

### Boot Modes

#### Mode 1: OpenSBI (Recommended)

Use an external OpenSBI `fw_dynamic.bin`:

```bash
./target/release/machina \
    -nographic -m 256 \
    -bios /usr/share/qemu/opensbi-riscv64-generic-fw_dynamic.bin \
    -kernel Image \
    -initrd rootfs.cpio.gz \
    -append "earlycon=ns16550a,mmio,0x10000000 console=ttyS0 root=/dev/ram rdinit=/sbin/init"
```

OpenSBI sources:
- **Ubuntu/Debian**: `apt install qemu-system-misc` installs
  `/usr/share/qemu/opensbi-riscv64-generic-fw_dynamic.bin`
- **Buildroot**: built under `output/host/share/qemu/`
- **Manual build**: https://github.com/riscv-software-src/opensbi

#### Mode 2: Embedded RustSBI

Omit the `-bios` flag to use the built-in RustSBI v0.4.0:

```bash
./target/release/machina \
    -nographic -m 256 \
    -kernel Image \
    -initrd rootfs.cpio.gz \
    -append "earlycon=ns16550a,mmio,0x10000000 console=ttyS0 root=/dev/ram rdinit=/sbin/init"
```

#### Mode 3: Bare-metal (No SBI)

For firmware or bare-metal binaries without SBI:

```bash
./target/release/machina \
    -nographic -m 128 \
    -bios none \
    -kernel firmware.bin
```

The binary loads at `0x80000000` and starts in M-mode.

### CLI Options

| Flag | Description |
|------|-------------|
| `-m SIZE` | RAM in MiB (default: 128) |
| `-bios PATH` | SBI firmware (`none` = skip, omit = RustSBI) |
| `-kernel PATH` | Kernel image (flat binary or ELF) |
| `-initrd PATH` | Initramfs (cpio.gz) |
| `-append STR` | Kernel command line |
| `-nographic` | Disable graphical output, serial on stdio |
| `-drive file=PATH` | Attach VirtIO block device |
| `-s` | GDB server on `tcp::1234` |
| `-S` | Freeze CPU at startup (use with GDB) |

### Kernel Command Line

Recommended parameters:

```
earlycon=ns16550a,mmio,0x10000000 console=ttyS0 root=/dev/ram rdinit=/sbin/init
```

| Parameter | Purpose |
|-----------|---------|
| `earlycon=ns16550a,mmio,0x10000000` | Early console via UART MMIO |
| `console=ttyS0` | Runtime console on first serial port |
| `root=/dev/ram` | Root filesystem is initramfs |
| `rdinit=/sbin/init` | Init process path in initramfs |

### Building the Kernel

A minimal kernel config for machina (no modules, no
network, initramfs):

```bash
# Cross-compile for RISC-V
export ARCH=riscv
export CROSS_COMPILE=riscv64-linux-gnu-

# Start from defconfig and trim
make defconfig
# Disable modules, enable initramfs
scripts/config --disable MODULES
scripts/config --enable BLK_DEV_INITRD

make -j$(nproc) Image
```

The `Image` file is in `arch/riscv/boot/Image`.

### Building the Root Filesystem

Using busybox for a minimal initramfs:

```bash
# Build static busybox for RISC-V
wget https://busybox.net/downloads/busybox-1.37.0.tar.bz2
tar xf busybox-1.37.0.tar.bz2
cd busybox-1.37.0
make ARCH=riscv CROSS_COMPILE=riscv64-linux-gnu- defconfig
sed -i 's/# CONFIG_STATIC is not set/CONFIG_STATIC=y/' .config
make ARCH=riscv CROSS_COMPILE=riscv64-linux-gnu- -j$(nproc)
make ARCH=riscv CROSS_COMPILE=riscv64-linux-gnu- install

# Create initramfs
cd _install
mkdir -p proc sys dev etc/init.d
cat > etc/init.d/rcS << 'INIT'
#!/bin/sh
mount -t proc none /proc
mount -t sysfs none /sys
INIT
chmod +x etc/init.d/rcS
cat > init << 'INIT'
#!/bin/sh
exec /sbin/init
INIT
chmod +x init
find . | cpio -o --format=newc | gzip > ../rootfs.cpio.gz
```

### Platform Details

The `riscv64-ref` machine emulates:

| Device | Address | IRQ |
|--------|---------|-----|
| MROM (reset vector) | `0x0000_1000` | -- |
| SiFive Test (shutdown) | `0x0010_0000` | -- |
| ACLINT (timer+IPI) | `0x0200_0000` | MTI/MSI |
| PLIC (interrupt controller) | `0x0C00_0000` | MEI/SEI |
| UART 16550A | `0x1000_0000` | 10 |
| VirtIO MMIO slot 0 | `0x1000_1000` | 1 |
| DRAM | `0x8000_0000` | -- |

ISA: `rv64imafdc_zba_zbb_zbc_zbs_zicsr_zifencei`

## Booting K230 SDK Linux on Machina

The `k230` machine follows the QEMU K230 SDK-compatible board model. It uses a
T-HEAD C908 CPU profile and does not generate a K230 device tree. For Linux
direct boot, pass the SDK DTB with `-dtb` so Machina can hand it to OpenSBI and
update `/chosen` for `-append` and `-initrd`. Bare-metal payloads, RTOS images,
or firmware with an embedded DTB may omit `-dtb`.

### K230 SDK Linux Direct Boot

```bash
SDK=k230_sdk/output/k230_canmv_defconfig
./target/release/machina -M k230 \
    -kernel "$SDK/images/little-core/Image" \
    -dtb "$SDK/images/little-core/k230.dtb" \
    -initrd "$SDK/images/little-core/rootfs.cpio.gz" \
    -append "console=ttyS0,115200 earlycon=sbi cma=0" \
    -nographic
```

### K230 SDK U-Boot Boot

This flow starts SDK U-Boot in M-mode with `-bios`. Until the SDK storage path
is modeled, place OpenSBI, Linux, initrd, and DTB in RAM with loader devices and
run `bootm` manually. The Linux Image must be rebuilt with standard RISC-V PTE
bits before running under Machina.

```bash
SDK=k230_sdk/output/k230_canmv_defconfig
IMAGE=$SDK/images/little-core/Image
INITRD=$SDK/images/little-core/rootfs.cpio.gz
DTB=$SDK/images/little-core/k230.dtb
FWJUMP_UIMAGE=/tmp/k230-fw-jump.uImage
"$SDK/little/buildroot-ext/host/bin/mkimage" \
    -A riscv -O linux -T kernel -C none \
    -a 0x08000000 -e 0x08000000 -n opensbi \
    -d "$SDK/images/little-core/fw_jump.bin" "$FWJUMP_UIMAGE"
./target/release/machina -M k230 \
    -bios "$SDK/little/uboot/u-boot" \
    -device loader,file="$FWJUMP_UIMAGE",addr=0x0c100000,force-raw=on \
    -device loader,file="$IMAGE",addr=0x08200000,force-raw=on \
    -device loader,file="$INITRD",addr=0x0a100000,force-raw=on \
    -device loader,file="$DTB",addr=0x0a000000,force-raw=on \
    -nographic
```

## Booting LoongArch64 Linux on Machina

The `loongarch64-ref` machine is Machina's LoongArch64 reference
platform. It models the QEMU-compatible subset needed by the current
direct-boot Linux path without exposing it as a QEMU `virt` machine
type.

### LoongArch64 Boot Quick Start

```bash
./target/release/machina \
    -M loongarch64-ref \
    -nographic -m 256 \
    -kernel /path/to/vmlinuz.efi \
    -initrd /path/to/rootfs.cpio.gz \
    -append "console=ttyS0 earlycon=uart8250,mmio,0x1fe001e0 rdinit=/init"
```

The loader accepts LoongArch64 ELF kernels, standard LoongArch Linux
`Image`/EFI-style images, and raw fallback images. The direct-boot ABI
sets `a0=efi_boot`, `a1=cmdline`, and `a2=system_table`; the generated
EFI config tables provide the FDT and initrd metadata consumed by Linux.

### LoongArch64 Platform Details

| Device | Address | IRQ / Route |
|--------|---------|-------------|
| IPI | `0x0100_0000` | CPU IPI |
| EIOINTC | `0x0200_0000` | CPU HWI |
| PCH-PIC | `0x1000_0000` | Routed through EIOINTC |
| VirtIO MMIO slot 0 | `0x1000_8000` | PCH-PIC / EIOINTC |
| UART 16550A | `0x1fe0_01e0` | PCH-PIC / EIOINTC |
| DRAM | low physical RAM with MMIO holes excluded from FDT memory | -- |

Current `loongarch64-ref` limitations:

- `-S` and `-gdb` are rejected for this machine.
- `-monitor` is rejected for this machine.
- VirtIO block via `-drive file=...` is supported; `virtio-net-device`
  and `-netdev` are rejected.

### Troubleshooting

**No console output**: Ensure `-append` includes
`earlycon=ns16550a,mmio,0x10000000`.

**Kernel panic / illegal instruction**: The kernel must
be built for `rv64imafdc` (RV64GC). Kernels requiring
Zfh, Zbkb, or Vector extensions are not supported.

**Stuck at DMA init**: Upgrade to the latest machina
(the neg_align fix in PR #23 resolves this).

## Keyboard Shortcuts

In `-nographic` mode, the escape prefix is `Ctrl+A`:

| Key | Action |
|-----|--------|
| Ctrl+A, X | Exit emulator |
| Ctrl+A, C | Toggle monitor console |
| Ctrl+A, H | Show help |
