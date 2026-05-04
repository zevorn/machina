# QEMU 硬件 → Machina Rust 1:1 翻译设计

## 范围

将 `~/qemu/hw` 下所有简单硬件设备翻译为 Machina Rust 代码，1:1 行为对
标，补充 mtest 框架完整覆盖。目标为 RISC-V、LoongArch 及架构无关设备。

## 核心原则

- **结构级 1:1**：寄存器偏移、位定义、MMIO 行为、IRQ 语义与 QEMU 完全一致。
  代码结构对齐 QEMU 函数/类型命名，便于追溯对比。
- **优先用 B，缺失的用 A**：优先使用 machina 已有机制（Arc、DeviceRefCell、
  SysBus、MmioOps）。当 machina 缺少等价物时（如 SPI bus、I2C bus、ptimer），
  先自行实现。
- **自底向上**：先构建基础设施框架，再翻译依赖它们的设备。

## 生命周期映射

QEMU C QOM → QEMU Rust QOM → Machina MOM：

```
QEMU C                    QEMU Rust QOM          Machina MOM
──────────────────────────────────────────────────────────────
instance_init             INSTANCE_INIT           new() / new_named()
class_init                CLASS_INIT              define_property()
instance_post_init        INSTANCE_POST_INIT      (合并到 board wiring)
property set              (字段注解)               set_property()
realize                   REALIZE                 Device::realize()
sysbus_init_mmio          init_mmio (post_init)   register_mmio()
+ mmio_map                + mmio_map              + realize_onto()
reset                     ResettablePhasesImpl    Device::reset()
                           (HOLD phase)            (reset_runtime)
unrealize                 (无显式 trait)           Device::unrealize()
                                                   + unrealize_from()
instance_finalize         Drop                    Drop (RAII)
```

关键规则：
- `Device::realize()`：处理设备专属的、可失败的初始化（chardev 连接、定时器
  创建、子设备）。返回 `Result<(), MDeviceError>`。在 MMIO 映射 BEFORE。
- `SysBusDeviceState::realize_onto()`：验证属性、标记 realized、将 MMIO 区域
  映射到地址空间。
- `Device::reset()`：仅触碰运行时状态（寄存器、定时器、IRQ 电平），不重建拓
  扑。

## 批次划分

共 54 项，分 6 个批次。

### Batch 0 — 基础设施（6 个框架）

| 组件 | C 源文件 | 描述 |
|------|---------|------|
| SPI bus (`hw/ssi/src/bus.rs`) | `ssi/ssi.c`（186 行） | `SpiBus` + `SpiSlave` trait |
| I2C bus (`hw/i2c/src/bus.rs`) | `i2c/core.c`（429 行） | `I2cBus` + `I2cSlave` trait |
| SD bus (`hw/sd/src/bus.rs`) | `sd/core.c`（273 行） | `SdBus` + `SdCard` trait |
| Ptimer (`hw/timer/src/ptimer.rs`) | `core/ptimer.c`（484 行） | 通用周期/单次定时器 |
| fw_cfg 接口 (`hw/firmware/src/`) | `fw_cfg-interface.c`（23 行）| 固件配置接口 |
|（GPIO） | 已有：`irq.rs` | IrqLine、IrqSink 已覆盖 |

### Batch 1 — 叶子层极简设备（8 个，每个 <200 行 C）

`sifive_e_prci`（124）、`sifive_u_prci`（169）、`gpio_key`（109）、
`gpio_pwr`（70）、`pvpanic`（77 + mmio 60）、`unimp`（98）、`led`（160）、
`virt_ctrl`（150）

### Batch 2 — 中断控制器（9 个）

`loongarch_dintc`（213）、`loongarch_pch_msi`（114）、
`loongson_ipi`（129 + common 362）、`loongson_liointc`（249）、
`riscv_aplic`（1106）、`riscv_imsic`（491）、`riscv_cmgcr`（243）、
`riscv_cpc`（273）

### Batch 3 — 外设层（17 个）

- **UART**：`pl011`（729）、`sifive_uart`（410）、`riscv_htif`（367）
- **RTC**：`pl031`（335）、`ls7a_rtc`（488）、`goldfish_rtc`（298）、
  `ds1338`（244）
- **Timer**：`sifive_pwm`（467）、`sse-timer`（470）、`sse-counter`（473）
- **GPIO**：`pl061`（593）、`sifive_gpio`（396）
- **SPI**：`pl022`（316）、`sifive_spi`（356）
- **其他**：`pl050`（278）、`sifive_e_aon`（326）、`sifive_u_otp`（293）

### Batch 4 — 存储与固件层（12 个）

- **Flash**：`m25p80`（1909）、`pflash_cfi01`（1038）、`pflash_cfi02`（1030）
- **SD/MMC**：`sd`（3257）、`sdhci`（2010）、`ssi-sd`（342）、`pl181`（544）
- **I2C 设备**：`eeprom_at24c`（266）、`smbus_eeprom`（296）
- **固件**：`fw_cfg`（1304）、`loongarch/boot`（449）

### Batch 5 — 杂项（5 个）

`sifive_pdma`（489）、`pl080`（471）、`tmp105`（341）、`tmp421`（391）、
`sbsa_gwdt`（299）

## 设备翻译模式

每个 QEMU C 结构体映射为遵循 machina 现有模式的 Rust 结构体：

```rust
pub struct DeviceX {
    // 设置阶段状态（用 Mutex 实现 Arc<Self> 共享访问）
    state: parking_lot::Mutex<SysBusDeviceState>,
    // 运行时寄存器（内部可变性）
    regs: DeviceRefCell<DeviceXRegs>,
    // IRQ 输出线（仅初始化期写入，运行时无锁读取）
    irqs: parking_lot::Mutex<Vec<Option<IrqLine>>>,
}

impl DeviceX {
    pub fn new() -> Self                 // instance_init → 分配+默认值
    pub fn attach_to_bus(&self, ...)     // 挂载到 SysBus
    pub fn register_mmio(&self, ...)     // 声明 MMIO 区域
    pub fn realize_onto(&self, ...)      // realize + MMIO 映射进地址空间
    pub fn reset_runtime(&self)          // reset（等价 HOLD phase）
    pub fn unrealize_from(&self, ...)    // unrealize + 解除 MMIO 映射
    pub fn read(&self, offset, size)     // MMIO 读
    pub fn write(&self, offset, size, v) // MMIO 写
}

pub struct DeviceXMmio(pub Arc<DeviceX>);
impl MmioOps for DeviceXMmio { ... }
```

## 测试策略

每个设备在 `tests/src/hw_<device>.rs` 中编写测试文件，覆盖级别为完整覆盖：

- Reset 后所有寄存器默认值
- 所有 MMIO 读写路径
- 只读/只写寄存器语义
- IRQ 产生和清除的所有路径
- 边界条件（越界 offset、无效值、溢出）
- 设备专属功能特性（loopback、FIFO、DMA 等）

框架组件（SPI bus、I2C bus、Ptimer）验证传输协议和定时器到期行为。

## 已实现设备（跳过）

PLIC、ACLINT、SiFive Test、UART 16550、LoongArch ExtIOI/IPI/PCH_PIC、
VirtIO block/net/mmio。
