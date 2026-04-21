use std::sync::Arc;

use machina_hw_virtio::net::{
    parse_mac, NetBackend, PipeBackend, TapBackend, VirtioNet, DEFAULT_MAC,
    VIRTIO_NET_HDR_SIZE_BASE, VIRTIO_NET_HDR_SIZE_MRG,
};
use machina_hw_virtio::VirtioDevice;

// ── TapBackend error ─────────────────────────────────

#[test]
fn test_tap_backend_invalid_ifname() {
    let result = TapBackend::new("nonexistent_xyz99");
    assert!(result.is_err());
}

// ── parse_mac ─────────────────────────────────────────

#[test]
fn test_parse_mac_valid() {
    let mac = parse_mac("52:54:00:12:34:56").unwrap();
    assert_eq!(mac, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
}

#[test]
fn test_parse_mac_all_ff() {
    let mac = parse_mac("ff:ff:ff:ff:ff:ff").unwrap();
    assert_eq!(mac, [0xff; 6]);
}

#[test]
fn test_parse_mac_empty() {
    assert!(parse_mac("").is_err());
}

#[test]
fn test_parse_mac_too_few() {
    assert!(parse_mac("52:54:00:12:34").is_err());
}

#[test]
fn test_parse_mac_too_many() {
    assert!(parse_mac("52:54:00:12:34:56:78").is_err());
}

#[test]
fn test_parse_mac_bad_hex() {
    assert!(parse_mac("ZZ:54:00:12:34:56").is_err());
}

// ── VirtioDevice trait ────────────────────────────────

fn make_net() -> VirtioNet {
    let pipe = PipeBackend::new().expect("pipe backend");
    VirtioNet::new_default(Arc::new(pipe))
}

#[test]
fn test_net_device_id() {
    let net = make_net();
    assert_eq!(net.device_id(), 1);
}

#[test]
fn test_net_num_queues() {
    let net = make_net();
    assert_eq!(net.num_queues(), 2);
}

#[test]
fn test_net_features() {
    let net = make_net();
    let f = net.features();
    assert_ne!(f & (1 << 32), 0); // VERSION_1
    assert_ne!(f & (1 << 5), 0); // MAC
    assert_ne!(f & (1 << 16), 0); // STATUS
    assert_ne!(f & (1 << 15), 0); // MRG_RXBUF
}

// ── Config space ──────────────────────────────────────

#[test]
fn test_net_config_mac() {
    let net = make_net();
    for i in 0..6u64 {
        let byte = net.config_read(i, 1) as u8;
        assert_eq!(byte, DEFAULT_MAC[i as usize]);
    }
}

#[test]
fn test_net_config_status() {
    let net = make_net();
    let status = net.config_read(6, 2) as u16;
    assert_eq!(status, 1); // link up
}

#[test]
fn test_net_config_max_vq_pairs() {
    let net = make_net();
    let pairs = net.config_read(8, 2) as u16;
    assert_eq!(pairs, 1);
}

#[test]
fn test_net_config_out_of_range() {
    let net = make_net();
    assert_eq!(net.config_read(100, 1), 0);
}

// ── Feature negotiation ──────────────────────────────

#[test]
fn test_net_hdr_size_base() {
    let mut net = make_net();
    // Ack features without MRG_RXBUF.
    net.ack_features((1u64 << 32) | (1 << 5));
    assert_eq!(net.hdr_size(), VIRTIO_NET_HDR_SIZE_BASE);
}

#[test]
fn test_net_hdr_size_mrg() {
    let mut net = make_net();
    // Ack features with MRG_RXBUF.
    net.ack_features((1u64 << 32) | (1 << 5) | (1 << 15));
    assert_eq!(net.hdr_size(), VIRTIO_NET_HDR_SIZE_MRG);
}

#[test]
fn test_net_reset_clears_features() {
    let mut net = make_net();
    net.ack_features(0xFFFF_FFFF);
    net.reset();
    assert_eq!(net.acked_features, 0);
}

// ── TX path (via MMIO transport) ─────────────────────

use std::sync::atomic::{AtomicBool, Ordering};

use machina_hw_core::irq::{IrqLine, IrqSink};
use machina_hw_virtio::mmio::VirtioMmio;
use machina_memory::region::MmioOps;

struct DummySink {
    level: AtomicBool,
}

impl IrqSink for DummySink {
    fn set_irq(&self, _irq: u32, level: bool) {
        self.level.store(level, Ordering::SeqCst);
    }
}

fn make_net_mmio() -> (VirtioMmio, Arc<DummySink>) {
    let pipe = PipeBackend::new().unwrap();
    let net = VirtioNet::new_default(Arc::new(pipe));
    let sink = Arc::new(DummySink {
        level: AtomicBool::new(false),
    });
    let irq = IrqLine::new(sink.clone() as Arc<dyn IrqSink>, 1);
    let mmio = VirtioMmio::new(
        Box::new(net),
        irq,
        std::ptr::null_mut(),
        0x8000_0000,
        128 * 1024 * 1024,
    );
    (mmio, sink)
}

#[test]
fn test_net_queue0_notify_is_noop() {
    let (dev, sink) = make_net_mmio();
    dev.write(0x070, 4, 0x0f); // DRIVER_OK
    dev.write(0x050, 4, 0); // QUEUE_NOTIFY queue 0 (RX)
    assert!(!sink.level.load(Ordering::SeqCst));
}

#[test]
fn test_net_rx_worker_starts_on_create() {
    let pipe = PipeBackend::new().unwrap();
    let net = VirtioNet::new_default(Arc::new(pipe));
    let sink = Arc::new(DummySink {
        level: AtomicBool::new(false),
    });
    let irq = IrqLine::new(sink.clone() as Arc<dyn IrqSink>, 1);
    let mmio = VirtioMmio::new(
        Box::new(net),
        irq,
        std::ptr::null_mut(),
        0x8000_0000,
        128 * 1024 * 1024,
    );
    // The RX worker should be running. Dropping
    // the VirtioMmio should join the worker thread
    // without hanging.
    std::thread::sleep(std::time::Duration::from_millis(50));
    drop(mmio);
}

#[test]
fn test_net_drop_joins_rx_thread() {
    let pipe = PipeBackend::new().unwrap();
    let net = VirtioNet::new_default(Arc::new(pipe));
    let sink = Arc::new(DummySink {
        level: AtomicBool::new(false),
    });
    let irq = IrqLine::new(sink.clone() as Arc<dyn IrqSink>, 1);
    let mmio = VirtioMmio::new(
        Box::new(net),
        irq,
        std::ptr::null_mut(),
        0x8000_0000,
        128 * 1024 * 1024,
    );
    let start = std::time::Instant::now();
    drop(mmio);
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "drop took too long: {:?}",
        elapsed
    );
}

#[test]
fn test_net_reset_does_not_deadlock() {
    let pipe = PipeBackend::new().unwrap();
    let backend = Arc::new(pipe);
    let net_backend: Arc<dyn machina_hw_virtio::net::NetBackend> =
        Arc::clone(&backend) as _;
    let net = VirtioNet::new_default(net_backend);
    let sink = Arc::new(DummySink {
        level: AtomicBool::new(false),
    });
    let irq = IrqLine::new(sink.clone() as Arc<dyn IrqSink>, 1);
    let mmio = VirtioMmio::new(
        Box::new(net),
        irq,
        std::ptr::null_mut(),
        0x8000_0000,
        128 * 1024 * 1024,
    );
    // Inject a packet so the RX worker has data.
    backend.inject_packet(b"hello").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    // Reset via MMIO STATUS write (holds MMIO lock
    // during device.reset()). Must not deadlock.
    let start = std::time::Instant::now();
    mmio.write(0x070, 4, 0); // STATUS = 0 → reset
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_millis(300),
        "reset deadlocked: {:?}",
        elapsed
    );
    drop(mmio);
}

// ── AC-3: deterministic lifecycle ────────────────────

#[test]
fn test_net_stop_within_200ms() {
    let (mmio, _) = make_net_mmio();
    let start = std::time::Instant::now();
    drop(mmio);
    assert!(start.elapsed() < std::time::Duration::from_millis(200),);
}

#[test]
fn test_net_reset_contention_via_shared_state() {
    let pipe = PipeBackend::new().unwrap();
    let backend = Arc::new(pipe);
    let nb: Arc<dyn machina_hw_virtio::net::NetBackend> =
        Arc::clone(&backend) as _;
    let net = VirtioNet::new_default(nb);
    let sink = Arc::new(DummySink {
        level: AtomicBool::new(false),
    });
    let irq = IrqLine::new(sink.clone() as Arc<dyn IrqSink>, 1);
    let mmio = VirtioMmio::new(
        Box::new(net),
        irq,
        std::ptr::null_mut(),
        0x8000_0000,
        128 * 1024 * 1024,
    );

    // Hold the MMIO lock to force worker contention.
    let ss = mmio.shared_state();
    let _guard = ss.lock().unwrap();

    // Inject packet while lock is held — worker will
    // try_lock and skip.
    backend.inject_packet(b"pkt1").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));

    // Release lock, then reset from the transport.
    drop(_guard);
    let start = std::time::Instant::now();
    mmio.write(0x070, 4, 0); // reset
    assert!(
        start.elapsed() < std::time::Duration::from_millis(200),
        "reset contention deadlocked"
    );

    drop(mmio);
}

// ── AC-4: distinct IRQ assertion ─────────────────────

#[test]
fn test_net_irq_distinct_from_blk() {
    use machina_hw_riscv::ref_machine::REF_IRQMAP;

    assert_ne!(
        REF_IRQMAP.virtio_net, REF_IRQMAP.virtio_base,
        "net IRQ must differ from block IRQ"
    );
    assert_eq!(REF_IRQMAP.virtio_net, 12);
}

// ── AC-2: descriptor-backed TX test ──────────────────

fn alloc_guest_ram(size: usize) -> *mut u8 {
    // SAFETY: mmap anonymous pages for test guest RAM.
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert_ne!(ptr, libc::MAP_FAILED, "mmap failed");
    ptr as *mut u8
}

const RAM_BASE: u64 = 0x8000_0000;
const RAM_SIZE: usize = 4 * 1024 * 1024;

fn setup_tx_queue(
    ram: *mut u8,
    desc_off: u64,
    avail_off: u64,
    used_off: u64,
    data_off: u64,
    payload: &[u8],
    hdr_size: usize,
) {
    // Write vnet header (zeros) + payload into data buf.
    let data_ptr = unsafe { ram.add(data_off as usize) };
    unsafe {
        std::ptr::write_bytes(data_ptr, 0, hdr_size);
        std::ptr::copy_nonoverlapping(
            payload.as_ptr(),
            data_ptr.add(hdr_size),
            payload.len(),
        );
    }

    let total = hdr_size + payload.len();
    // Descriptor 0: points to data buffer.
    let dp = unsafe { ram.add(desc_off as usize) };
    unsafe {
        (dp as *mut u64).write_unaligned(RAM_BASE + data_off);
        (dp.add(8) as *mut u32).write_unaligned(total as u32);
        (dp.add(12) as *mut u16).write_unaligned(0);
        (dp.add(14) as *mut u16).write_unaligned(0);
    }

    // Avail ring: flags=0, idx=1, ring[0]=0.
    let ap = unsafe { ram.add(avail_off as usize) };
    unsafe {
        (ap as *mut u16).write_unaligned(0); // flags
        (ap.add(2) as *mut u16).write_unaligned(1); // idx
        (ap.add(4) as *mut u16).write_unaligned(0); // ring[0]
    }

    // Used ring: flags=0, idx=0 (empty).
    let up = unsafe { ram.add(used_off as usize) };
    unsafe {
        (up as *mut u16).write_unaligned(0);
        (up.add(2) as *mut u16).write_unaligned(0);
    }
}

#[test]
fn test_net_tx_strips_header_and_sends() {
    let ram = alloc_guest_ram(RAM_SIZE);
    let pipe = PipeBackend::new().unwrap();
    let pipe_arc = Arc::new(pipe);
    let nb: Arc<dyn machina_hw_virtio::net::NetBackend> =
        Arc::clone(&pipe_arc) as _;
    let net = VirtioNet::new_default(nb);
    let sink = Arc::new(DummySink {
        level: AtomicBool::new(false),
    });
    let irq = IrqLine::new(sink.clone() as Arc<dyn IrqSink>, 1);
    let dev =
        VirtioMmio::new(Box::new(net), irq, ram, RAM_BASE, RAM_SIZE as u64);

    let desc_off: u64 = 0x1000;
    let avail_off: u64 = 0x2000;
    let used_off: u64 = 0x3000;
    let data_off: u64 = 0x4000;
    let payload = b"ETHERNET_FRAME_DATA";
    let hdr_size = VIRTIO_NET_HDR_SIZE_BASE;

    setup_tx_queue(
        ram, desc_off, avail_off, used_off, data_off, payload, hdr_size,
    );

    // Configure TX queue (queue 1).
    dev.write(0x030, 4, 1); // QUEUE_SEL = 1
    dev.write(0x038, 4, 16); // QUEUE_NUM = 16
    dev.write(0x080, 4, (RAM_BASE + desc_off) & 0xFFFF_FFFF);
    dev.write(0x084, 4, (RAM_BASE + desc_off) >> 32);
    dev.write(0x090, 4, (RAM_BASE + avail_off) & 0xFFFF_FFFF);
    dev.write(0x094, 4, (RAM_BASE + avail_off) >> 32);
    dev.write(0x0a0, 4, (RAM_BASE + used_off) & 0xFFFF_FFFF);
    dev.write(0x0a4, 4, (RAM_BASE + used_off) >> 32);
    dev.write(0x044, 4, 1); // QUEUE_READY = 1
    dev.write(0x070, 4, 0x0f); // STATUS = DRIVER_OK

    // Kick TX queue.
    dev.write(0x050, 4, 1); // QUEUE_NOTIFY = 1

    // Read what the backend received.
    let mut recv_buf = [0u8; 256];
    let n = pipe_arc.read_packet(&mut recv_buf).unwrap();
    assert_eq!(
        &recv_buf[..n],
        payload,
        "TX should strip vnet header and send payload"
    );

    drop(dev);
    unsafe {
        libc::munmap(ram as *mut libc::c_void, RAM_SIZE);
    }
}
