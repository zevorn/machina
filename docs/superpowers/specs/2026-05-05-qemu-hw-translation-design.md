# QEMU Hardware → Machina Rust 1:1 Translation Design

## Scope

Translate all simple hardware devices from `~/qemu/hw` into Machina Rust code,
with 1:1 behavioral correspondence and comprehensive mtest coverage.

## Guiding Principles

- **1:1 structural mapping**: Register offsets, bit definitions, MMIO behavior,
  and IRQ semantics match QEMU exactly. Code structure follows QEMU function/type
  naming for traceability.
- **B for infrastructure, A for missing**: Use machina's existing mechanisms
  (Arc, DeviceRefCell, SysBus, MmioOps). When machina lacks an equivalent
  (e.g., SPI bus, I2C bus, ptimer), implement it first.
- **Bottom-up**: Build frameworks before devices that depend on them.

## Lifecycle Mapping

QEMU C QOM → QEMU Rust QOM → Machina MOM:

```
QEMU C                    QEMU Rust QOM          Machina MOM
──────────────────────────────────────────────────────────────
instance_init             INSTANCE_INIT           new() / new_named()
class_init                CLASS_INIT              define_property()
instance_post_init        INSTANCE_POST_INIT      (consolidated in board wiring)
property set              (field annotations)     set_property()
realize                   REALIZE                 Device::realize()
sysbus_init_mmio          init_mmio (post_init)   register_mmio()
+ mmio_map                + mmio_map              + realize_onto()
reset                     ResettablePhasesImpl    Device::reset()
                           (HOLD phase)            (reset_runtime)
unrealize                 (no explicit trait)     Device::unrealize()
                                                   + unrealize_from()
instance_finalize         Drop                    Drop (RAII)
```

Key rules:
- `Device::realize()` handles device-specific, fallible init (chardev, timers).
  Returns `Result<(), MDeviceError>`. Called BEFORE MMIO mapping.
- `SysBusDeviceState::realize_onto()` validates, marks realized, maps MMIO
  into address space.
- `Device::reset()` only touches runtime state (registers, timers, IRQ levels).
  Never rebuilds topology.

## Batch Breakdown

54 items total across 6 batches.

### Batch 0 — Infrastructure (6 frameworks, ~2000 lines total)

| Component | C source | Description |
|-----------|----------|-------------|
| SPI bus (`hw/ssi/src/bus.rs`) | `ssi/ssi.c` (186 lines) | `SpiBus` + `SpiSlave` trait |
| I2C bus (`hw/i2c/src/bus.rs`) | `i2c/core.c` (429 lines) | `I2cBus` + `I2cSlave` trait |
| SD bus (`hw/sd/src/bus.rs`) | `sd/core.c` (273 lines) | `SdBus` + `SdCard` trait |
| Ptimer (`hw/timer/src/ptimer.rs`) | `core/ptimer.c` (484 lines) | Generic periodic/one-shot timer |
| fw_cfg interface (`hw/firmware/src/`) | `fw_cfg-interface.c` (23 lines) | Firmware config interface |
| (GPIO) | Already covered by `irq.rs` | IrqLine, IrqSink |

### Batch 1 — Leaf-level Devices (8 devices, <200 C lines each)

`sifive_e_prci`, `sifive_u_prci`, `gpio_key`, `gpio_pwr`, `pvpanic` (+mmio),
`unimp`, `led`, `virt_ctrl`

### Batch 2 — Interrupt Controllers (9 devices)

`loongarch_dintc` (213), `loongarch_pch_msi` (114), `loongson_ipi` (129+362),
`loongson_liointc` (249), `riscv_aplic` (1106), `riscv_imsic` (491),
`riscv_cmgcr` (243), `riscv_cpc` (273)

### Batch 3 — Peripherals (17 devices)

**UART:** `pl011` (729), `sifive_uart` (410), `riscv_htif` (367)
**RTC:** `pl031` (335), `ls7a_rtc` (488), `goldfish_rtc` (298), `ds1338` (244)
**Timer:** `sifive_pwm` (467), `sse-timer` (470), `sse-counter` (473)
**GPIO:** `pl061` (593), `sifive_gpio` (396)
**SPI:** `pl022` (316), `sifive_spi` (356)
**Other:** `pl050` (278), `sifive_e_aon` (326), `sifive_u_otp` (293)

### Batch 4 — Storage & Firmware (12 devices)

**Flash:** `m25p80` (1909), `pflash_cfi01` (1038), `pflash_cfi02` (1030)
**SD/MMC:** `sd` (3257), `sdhci` (2010), `ssi-sd` (342), `pl181` (544)
**I2C:** `eeprom_at24c` (266), `smbus_eeprom` (296)
**Firmware:** `fw_cfg` (1304), `loongarch/boot` (449)

### Batch 5 — Miscellaneous (5 devices)

`sifive_pdma` (489), `pl080` (471), `tmp105` (341), `tmp421` (391),
`sbsa_gwdt` (299)

## Device Pattern

Each QEMU C struct maps to a Rust struct following the established machina pattern:

```rust
pub struct DeviceX {
    // Setup state (Mutex for &self access)
    state: parking_lot::Mutex<SysBusDeviceState>,
    // Runtime registers (interior mutability)
    regs: DeviceRefCell<DeviceXRegs>,
    // IRQ outputs (written only during init, read lock-free at runtime)
    irqs: parking_lot::Mutex<Vec<Option<IrqLine>>>,
}

impl DeviceX {
    pub fn new() -> Self                 // instance_init
    pub fn attach_to_bus(&self, ...)     // bus wiring
    pub fn register_mmio(&self, ...)     // MMIO declaration
    pub fn realize_onto(&self, ...)       // realize + MMIO mapping
    pub fn reset_runtime(&self)          // reset (HOLD phase only)
    pub fn unrealize_from(&self, ...)    // unrealize + MMIO teardown
    pub fn read(&self, offset, size)     // MMIO read
    pub fn write(&self, offset, size, v) // MMIO write
}

pub struct DeviceXMmio(pub Arc<DeviceX>);
impl MmioOps for DeviceXMmio { ... }
```

## Testing Strategy

Each device gets a `tests/src/hw_<device>.rs` file with complete coverage:

- Reset register defaults
- All MMIO read/write paths
- Read-only / write-only register semantics
- IRQ generation and clearing (all paths)
- Boundary conditions (invalid offsets, overflow, invalid values)
- Device-specific features (loopback, FIFO, DMA, etc.)

Framework components (SPI bus, I2C bus, Ptimer) get protocol-level and
timer-expiry tests.

## Already Implemented (Skip)

PLIC, ACLINT, SiFive Test, UART 16550, LoongArch ExtIOI/IPI/PCH_PIC,
VirtIO block/net/mmio.
