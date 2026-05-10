# Machina 快速入门

> 目标读者：希望构建、运行和引导客户软件的开发者。

## 新贡献者上手流程

下面这条路径仅依赖仓库自带的 `tests/firmware/` 固件，因此 fresh
clone 之后无需准备外部内核、磁盘镜像或 RISC-V 交叉工具链就能跑通。
按顺序执行；任意一步失败请参见末尾的「常见故障」表。

### 1. 安装宿主机依赖

- 当前 stable Rust 工具链（`rustc 1.80+`）以及 `cargo`，建议通过
  [`rustup`](https://rustup.rs/) 安装。
- GNU `make`。
- 一个可用的宿主链接器（`cc`），release 构建时 cargo 会调用它。

### 2. 克隆与构建

```bash
git clone https://github.com/gevico/machina.git
cd machina
make release
```

首次构建会拉取工作区依赖（在主流笔记本上约 1–3 分钟）。可执行
文件位于 `./target/release/machina`。

### 3. 用自带固件做最短的端到端冒烟

最便宜的全流程检查 —— 启动自带的裸金属 PASS 内核，跑完后退出码 0：

```bash
./target/release/machina -M riscv64-ref -m 128 -bios none \
    -kernel tests/firmware/sifive_pass.bin -nographic
```

预期输出（`shutdown (pass)` 这一行就是成功标志）：

```
machina: riscv64-ref, 128 MiB RAM
machina: entering execution loop
machina: shutdown (pass)
```

如果想顺带验证捆绑的 RustSBI 路径：

```bash
./target/release/machina -M riscv64-ref -m 128 \
    -kernel tests/firmware/sbi_smoke.bin -nographic
```

输出应以 RustSBI banner 开头，并以 `MACHINA_SBI_OK` 结束，随后
Machina 干净退出。

### 4. 跑你正在改动那一块的窄测试

迭代时永远先用 filter 跑窄测试，整套 `make test` 比较慢。示例：

```bash
# 启动工具自身的冒烟。
cargo test -p machina-tests tools::

# 反汇编器回归。
cargo test -p machina-tests disas

# 内存区域 / FlatView。
cargo test -p machina-tests memory_region

# RISC-V CSR 语义。
cargo test -p machina-tests riscv_csr
```

### 5. 提 PR 之前的检查

提 PR 之前，按 CI 同样的检查跑一遍：

```bash
make fmt-check    # rustfmt diff 必须为空
make clippy       # 0 个 clippy 警告
make test         # 全量测试套件
```

如果改动了 `.agents/`，再跑一次 `make check-agent-skills`。

### 常见故障

| 现象 | 大概率原因 / 修法 |
|------|-------------------|
| `make release` 提示 `cc not found` | 装宿主机的编译工具：Debian/Ubuntu 用 `build-essential`，macOS 装 Xcode CLT，Fedora/Arch 用 `gcc`。 |
| `tests/firmware/*.bin` 不存在 | 仓库自带预编译二进制。若被删掉，可在装好 `riscv64-elf-` 交叉工具链后执行 `cd tests/firmware && ./build.sh` 重建。 |
| 冒烟命令卡住 | 检查参数是否漏了 `-bios none`。`sifive_pass.bin` 是裸金属内核，不带 `-bios none` 会被 RustSBI 接管而不退出。 |
| `make test` 跑得很久 | 这是正常的——测试套件很大。日常迭代请用 `cargo test -p machina-tests <filter>` 跑窄测试，只在 push 之前跑一次 `make test`。 |
| `info registers` 提示 "VM must be paused" | 当前虚拟机仍在运行。先发 `stop`（HMP）或 `{"execute":"stop"}`（QMP）暂停，再查询寄存器。 |

## 快速开始

### 构建

```bash
git clone https://github.com/gevico/machina.git
cd machina
make release
```

### 运行

```bash
# 引导内核
./target/release/machina -nographic -bios none \
    -kernel path/to/kernel.elf

# 带 VirtIO 块设备
./target/release/machina -nographic \
    -drive file=path/to/disk.img \
    -kernel path/to/kernel.elf

# 带 Monitor 控制台（QMP over TCP）
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

## 机器类型

Machina 当前暴露三个用户可见机器：

| 机器 | 客户 ISA | 用途 |
|------|----------|------|
| `riscv64-ref` | RISC-V 64-bit | RISC-V 参考平台，包含 SBI、PLIC、ACLINT、UART 和 VirtIO MMIO |
| `k230` | RISC-V 64-bit | Kendryte K230 SDK 兼容平台，包含 C908 CPU profile、PLIC、ACLINT、UART、WDT、带 `-dtb` 的 Linux 直接启动和 SDK U-Boot loader 启动 |
| `loongarch64-ref` | LoongArch64 | LoongArch64 参考平台，包含 Linux 直接启动、IOCSR、IPI、EIOINTC、PCH-PIC、UART 和 VirtIO block |

列出支持的机器：

```bash
./target/release/machina -M ?
```

## 启动 K230 SDK Linux

`k230` machine 对齐 QEMU 的 K230 SDK 兼容启动约定：Machina 不生成 K230
设备树。Linux direct boot 需要传入 SDK DTB，固件用它把板级拓扑交给
Linux，Machina 也会基于它更新 `/chosen` 里的 `-append` 和 `-initrd`
信息。

请把 SDK Linux 编译成标准 RISC-V PTE。当前 Machina K230 路径暂不建模
T-HEAD MAEE 页属性位，这一点和作为 oracle 的 QEMU K230 路径保持一致。
在 Kendryte SDK 源码树里，重编 little-core Linux 时传入
`-DQEMU_NO_THEAD_MAEE`：

```bash
cd ~/k230_sdk
make CONF=k230_canmv_defconfig linux-clean
make CONF=k230_canmv_defconfig \
    KCFLAGS="-DDBGLV=0 -DQEMU_NO_THEAD_MAEE" \
    linux-rebuild
cp output/k230_canmv_defconfig/little/linux/arch/riscv/boot/Image \
   output/k230_canmv_defconfig/images/little-core/Image
```

Linux direct boot 使用 SDK 的 `Image`、`k230.dtb` 和 initramfs：

```bash
SDK=~/k230_sdk/output/k230_canmv_defconfig
./target/release/machina -M k230 \
    -kernel "$SDK/images/little-core/Image" \
    -dtb "$SDK/images/little-core/k230.dtb" \
    -initrd "$SDK/images/little-core/rootfs.cpio.gz" \
    -append "console=ttyS0,115200 earlycon=sbi cma=0" \
    -nographic
```

SDK U-Boot 流程用 `-bios` 从 M-mode 启动 U-Boot。在 SDK 存储路径建模前，
先通过 loader device 把 OpenSBI、Linux、initrd 和 DTB 放入内存，然后在
U-Boot 里手动执行 `bootm`：

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

## 在 Machina 上启动 RISC-V Linux 内核

本文档介绍如何在 machina `riscv64-ref` 平台上启动标准
RISC-V Linux 内核。

### 环境要求

| 组件 | 版本 | 说明 |
|------|------|------|
| Rust 工具链 | stable 1.80+ | `cargo build --release` |
| RISC-V Linux 内核 | 6.12+ | flat `Image` 格式 |
| SBI 固件 | OpenSBI 1.4+ 或内嵌 RustSBI | 见下文 |
| 根文件系统 | initramfs (cpio.gz) | 推荐 busybox |
| 交叉编译工具链 | riscv64-linux-gnu-gcc | 编译内核用 |

### 快速开始

```bash
# 1. 编译 machina
cargo build --release

# 2. 使用系统 OpenSBI + initramfs 启动
./target/release/machina \
    -nographic -m 256 \
    -bios /usr/share/qemu/opensbi-riscv64-generic-fw_dynamic.bin \
    -kernel /path/to/Image \
    -initrd /path/to/rootfs.cpio.gz \
    -append "earlycon=ns16550a,mmio,0x10000000 console=ttyS0 root=/dev/ram rdinit=/sbin/init"
```

预期输出（节选）：

```
OpenSBI v1.5.1
...
Boot HART Base ISA        : rv64imafdc
...
Linux version 6.12.51 ...
...
Please press Enter to activate this console.
```

### 启动模式

#### 模式一：OpenSBI（推荐）

使用外部 OpenSBI `fw_dynamic.bin`：

```bash
./target/release/machina \
    -nographic -m 256 \
    -bios /usr/share/qemu/opensbi-riscv64-generic-fw_dynamic.bin \
    -kernel Image \
    -initrd rootfs.cpio.gz \
    -append "earlycon=ns16550a,mmio,0x10000000 console=ttyS0 root=/dev/ram rdinit=/sbin/init"
```

OpenSBI 获取方式：
- **Ubuntu/Debian**：`apt install qemu-system-misc` 会安装
  `/usr/share/qemu/opensbi-riscv64-generic-fw_dynamic.bin`
- **Buildroot**：编译产物在 `output/host/share/qemu/` 下
- **手动编译**：https://github.com/riscv-software-src/opensbi

#### 模式二：内嵌 RustSBI

省略 `-bios` 参数即使用内置的 RustSBI v0.4.0：

```bash
./target/release/machina \
    -nographic -m 256 \
    -kernel Image \
    -initrd rootfs.cpio.gz \
    -append "earlycon=ns16550a,mmio,0x10000000 console=ttyS0 root=/dev/ram rdinit=/sbin/init"
```

#### 模式三：裸机模式（无 SBI）

适用于裸机固件或 riscv-tests：

```bash
./target/release/machina \
    -nographic -m 128 \
    -bios none \
    -kernel firmware.bin
```

二进制文件加载到 `0x80000000`，以 M-mode 启动。

### 命令行参数

| 参数 | 说明 |
|------|------|
| `-m SIZE` | 内存大小（MiB，默认 128） |
| `-bios PATH` | SBI 固件（`none` = 跳过，省略 = RustSBI） |
| `-kernel PATH` | 内核镜像（flat binary 或 ELF） |
| `-initrd PATH` | initramfs 根文件系统（cpio.gz） |
| `-append STR` | 内核启动命令行 |
| `-nographic` | 禁用图形输出，串口重定向到 stdio |
| `-drive file=PATH` | 挂载 VirtIO 块设备 |
| `-s` | 在 `tcp::1234` 启动 GDB 服务器 |
| `-S` | 启动时冻结 CPU（配合 GDB 使用） |

### 内核命令行参数

推荐参数：

```
earlycon=ns16550a,mmio,0x10000000 console=ttyS0 root=/dev/ram rdinit=/sbin/init
```

| 参数 | 作用 |
|------|------|
| `earlycon=ns16550a,mmio,0x10000000` | 通过 UART MMIO 启用早期控制台 |
| `console=ttyS0` | 运行时控制台使用第一个串口 |
| `root=/dev/ram` | 根文件系统为 initramfs |
| `rdinit=/sbin/init` | initramfs 中 init 进程路径 |

### 编译内核

最小内核配置（无模块、无网络、使用 initramfs）：

```bash
# 交叉编译 RISC-V 内核
export ARCH=riscv
export CROSS_COMPILE=riscv64-linux-gnu-

# 从 defconfig 开始，精简配置
make defconfig
# 禁用模块，启用 initramfs
scripts/config --disable MODULES
scripts/config --enable BLK_DEV_INITRD

make -j$(nproc) Image
```

产出的 `Image` 文件在 `arch/riscv/boot/Image`。

### 制作根文件系统

使用 busybox 制作最小 initramfs：

```bash
# 编译静态链接的 RISC-V busybox
wget https://busybox.net/downloads/busybox-1.37.0.tar.bz2
tar xf busybox-1.37.0.tar.bz2
cd busybox-1.37.0
make ARCH=riscv CROSS_COMPILE=riscv64-linux-gnu- defconfig
sed -i 's/# CONFIG_STATIC is not set/CONFIG_STATIC=y/' .config
make ARCH=riscv CROSS_COMPILE=riscv64-linux-gnu- -j$(nproc)
make ARCH=riscv CROSS_COMPILE=riscv64-linux-gnu- install

# 打包 initramfs
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

### 平台硬件信息

`riscv64-ref` 模拟的设备：

| 设备 | 地址 | 中断号 |
|------|------|--------|
| MROM（复位向量） | `0x0000_1000` | — |
| SiFive Test（关机） | `0x0010_0000` | — |
| ACLINT（定时器+IPI） | `0x0200_0000` | MTI/MSI |
| PLIC（中断控制器） | `0x0C00_0000` | MEI/SEI |
| UART 16550A | `0x1000_0000` | 10 |
| VirtIO MMIO 插槽 0 | `0x1000_1000` | 1 |
| DRAM | `0x8000_0000` | — |

指令集：`rv64imafdc_zba_zbb_zbc_zbs_zicsr_zifencei`

## 在 Machina 上启动 K230 SDK Linux

`k230` machine 对齐 QEMU 的 K230 SDK 兼容板级模型，使用 T-HEAD
C908 CPU profile，并且不生成 K230 device tree。Linux 直接启动时需要
通过 `-dtb` 传入 SDK DTB，Machina 会把它交给 OpenSBI，并根据
`-append` 与 `-initrd` 更新 `/chosen`。裸机、RTOS 或自带 DTB 的固件可
以省略 `-dtb`。

### K230 SDK Linux 直接启动

```bash
SDK=k230_sdk/output/k230_canmv_defconfig
./target/release/machina -M k230 \
    -kernel "$SDK/images/little-core/Image" \
    -dtb "$SDK/images/little-core/k230.dtb" \
    -initrd "$SDK/images/little-core/rootfs.cpio.gz" \
    -append "console=ttyS0,115200 earlycon=sbi cma=0" \
    -nographic
```

### K230 SDK U-Boot 启动

此流程通过 `-bios` 从 M-mode 启动 SDK U-Boot。在 SDK 存储路径建模之前，
需要用 loader device 把 OpenSBI、Linux、initrd 和 DTB 放入 RAM，再手动
执行 `bootm`。Linux Image 需要用标准 RISC-V PTE bit 重新构建后再在
Machina 下运行。

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

## 在 Machina 上启动 LoongArch64 Linux

`loongarch64-ref` 是 Machina 的 LoongArch64 参考平台。它实现
当前 Linux 直接启动路径需要的 QEMU 兼容子集，但用户可见的
machine 类型按 Machina reference machine 命名，而不是 QEMU
`virt`。

### LoongArch64 快速启动

```bash
./target/release/machina \
    -M loongarch64-ref \
    -nographic -m 256 \
    -kernel /path/to/vmlinuz.efi \
    -initrd /path/to/rootfs.cpio.gz \
    -append "console=ttyS0 earlycon=uart8250,mmio,0x1fe001e0 rdinit=/init"
```

加载器支持 LoongArch64 ELF kernel、标准 LoongArch Linux
`Image`/EFI 风格镜像，以及 raw fallback 镜像。直接启动 ABI
设置 `a0=efi_boot`、`a1=cmdline`、`a2=system_table`；生成的
EFI config table 提供 Linux 使用的 FDT 与 initrd 元数据。

### LoongArch64 平台硬件信息

| 设备 | 地址 | 中断/路由 |
|------|------|-----------|
| IPI | `0x0100_0000` | CPU IPI |
| EIOINTC | `0x0200_0000` | CPU HWI |
| PCH-PIC | `0x1000_0000` | 经 EIOINTC 路由 |
| VirtIO MMIO 插槽 0 | `0x1000_8000` | PCH-PIC / EIOINTC |
| UART 16550A | `0x1fe0_01e0` | PCH-PIC / EIOINTC |
| DRAM | 低物理内存，FDT memory 中排除 MMIO hole | — |

当前 `loongarch64-ref` 限制：

- 此机器会拒绝 `-S` 和 `-gdb`。
- 此机器会拒绝 `-monitor`。
- 支持通过 `-drive file=...` 使用 VirtIO block；会拒绝
  `virtio-net-device` 和 `-netdev`。

### 常见问题

**没有控制台输出**：确认 `-append` 中包含
`earlycon=ns16550a,mmio,0x10000000`。

**内核崩溃 / 非法指令**：内核必须为 `rv64imafdc`（RV64GC）
编译。需要 Zfh、Zbkb 或 Vector 扩展的内核不受支持。

**卡在 DMA 初始化**：请更新到最新版 machina
（PR #23 中的 neg_align 修复解决了此问题）。

## 快捷键

在 `-nographic` 模式下，转义前缀为 `Ctrl+A`：

| 按键 | 功能 |
|------|------|
| Ctrl+A, X | 退出模拟器 |
| Ctrl+A, C | 切换 Monitor 控制台 |
| Ctrl+A, H | 显示帮助 |
