//! Thread-local event tracer for debugging and analysis.
//!
//! ## Multithreading model
//!
//! - The "tracing is active" gate is a single global atomic
//!   ([`ENABLED`]); this gives a fast path for trace points on
//!   threads that have not opted in.
//! - The trace *destination* is per-thread: every public
//!   `trace_*` function writes to the calling thread's
//!   [`TRACE_FILE`]. A thread that has not called
//!   [`init_trace`] has no destination, so its trace calls are
//!   silently dropped even when [`trace_enabled`] reports true
//!   because some *other* thread enabled tracing.
//! - As a consequence, [`trace_enabled`] only tells you whether
//!   *some* thread has initialised tracing; it does not imply
//!   that the calling thread will produce output. To trace
//!   events from multiple threads, every participating thread
//!   must call [`init_trace`] with its own path.

use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static TRACE_FILE: RefCell<Option<File>> =
        const { RefCell::new(None) };
}

/// Open a trace file for the **current thread** and enable
/// tracing globally.
///
/// Each thread that should produce output must call this on
/// itself with its own path; the trace destination is a
/// thread-local. Calling `init_trace` on the main thread does
/// not retroactively configure child threads.
///
/// # Errors
///
/// Returns an error if the file cannot be created.
pub fn init_trace(path: &str) -> std::io::Result<()> {
    let file = File::create(path)?;
    TRACE_FILE.with(|f| {
        *f.borrow_mut() = Some(file);
    });
    ENABLED.store(true, Ordering::Relaxed);
    Ok(())
}

/// Fast check whether *some* thread has enabled tracing.
///
/// Returns `false` when no thread has ever called
/// [`init_trace`], making the cost of an unconfigured trace
/// point a single relaxed atomic load. A `true` result does
/// **not** mean the calling thread has a trace file open: a
/// thread that has not called [`init_trace`] still drops its
/// own trace events even when this returns `true`.
#[inline]
pub fn trace_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Record a CSR write.
pub fn trace_csr(name: &str, val: u64) {
    if !trace_enabled() {
        return;
    }
    TRACE_FILE.with(|f| {
        if let Some(file) = f.borrow_mut().as_mut() {
            let _ = writeln!(file, "CSR {name} <- 0x{val:016x}");
        }
    });
}

/// Record an exception event.
pub fn trace_exception(cause: u64, pc: u64) {
    if !trace_enabled() {
        return;
    }
    TRACE_FILE.with(|f| {
        if let Some(file) = f.borrow_mut().as_mut() {
            let _ = writeln!(file, "EXC cause={cause} pc=0x{pc:016x}");
        }
    });
}

/// Record translation of a guest TB.
pub fn trace_tb(pc: u64, flags: u32) {
    if !trace_enabled() {
        return;
    }
    TRACE_FILE.with(|f| {
        if let Some(file) = f.borrow_mut().as_mut() {
            let _ = writeln!(file, "TB pc=0x{pc:016x} flags=0x{flags:08x}");
        }
    });
}

/// Record an MMIO access.
pub fn trace_mmio(addr: u64, size: u32, val: u64, is_write: bool) {
    if !trace_enabled() {
        return;
    }
    let dir = if is_write { 'W' } else { 'R' };
    TRACE_FILE.with(|f| {
        if let Some(file) = f.borrow_mut().as_mut() {
            let _ = writeln!(
                file,
                "MMIO {dir} addr=0x{addr:016x} \
                 size={size} val=0x{val:016x}"
            );
        }
    });
}
