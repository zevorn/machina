use machina_hw_intc::eiointc::Eiointc;

#[test]
fn enable_write_read_at_0x200() {
    let mut e = Eiointc::new();
    e.mmio_write(0x200, 0xDEAD_BEEF);
    assert_eq!(e.mmio_read(0x200), 0xDEAD_BEEF);
}

#[test]
fn enable_0x204_independent_of_0x200() {
    let mut e = Eiointc::new();
    e.mmio_write(0x200, 0xAAAA_AAAA);
    e.mmio_write(0x204, 0xBBBB_BBBB);
    assert_eq!(e.mmio_read(0x200), 0xAAAA_AAAA);
    assert_eq!(e.mmio_read(0x204), 0xBBBB_BBBB);
}

#[test]
fn set_irq_pending_when_enabled() {
    let mut e = Eiointc::new();
    e.mmio_write(0x200, 1);
    e.mmio_write(0x0C0, 1);
    e.set_irq(0, true);
    assert_ne!(e.pending_for_cpu(0) & (1 << 1), 0);
}

#[test]
fn set_irq_masked_when_disabled() {
    let mut e = Eiointc::new();
    e.set_irq(0, true);
    assert_eq!(e.pending_for_cpu(0), 0);
}

#[test]
fn ack_clears_isr_via_core_isr() {
    let mut e = Eiointc::new();
    e.mmio_write(0x200, 1);
    e.set_irq(0, true);
    assert_ne!(e.mmio_read(0x400) & 1, 0);
    e.mmio_write(0x400, 1);
    assert_eq!(e.mmio_read(0x400) & 1, 0);
}

#[test]
fn core_isr_read_returns_enabled_pending() {
    let mut e = Eiointc::new();
    e.mmio_write(0x200, 0x5);
    e.set_irq(0, true);
    e.set_irq(2, true);
    assert_eq!(e.mmio_read(0x400), 0x5);
}

#[test]
fn coremap_routes_to_specific_cpu() {
    let mut e = Eiointc::new();
    e.mmio_write(0x200, 1);
    e.mmio_write(0x0C0, 1);
    e.mmio_write(0x800, 1);
    e.set_irq(0, true);
    assert_eq!(e.pending_for_cpu(0), 0);
    assert_ne!(e.pending_for_cpu(1) & (1 << 1), 0);
}
