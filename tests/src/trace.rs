use std::io::Read;

use machina_util::trace;

#[test]
fn test_trace_no_output_without_thread_init() {
    // Without calling init_trace on THIS thread,
    // trace calls should produce no output even if
    // the global ENABLED flag is set by another test.
    let dir = tempfile::tempdir().unwrap();
    let check = dir.path().join("no_output.log");
    // Do NOT call init_trace here.
    trace::trace_csr("test", 0);
    assert!(!check.exists());
}

#[test]
fn test_trace_disabled_produces_no_output() {
    // Calling trace functions when disabled should not
    // panic or produce side effects.
    trace::trace_csr("mstatus", 0x1234);
    trace::trace_exception(2, 0x8000_0000);
    trace::trace_mmio(0x1000_0000, 4, 0x42, true);
}

#[test]
fn test_trace_init_and_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace.log");
    trace::init_trace(path.to_str().unwrap()).unwrap();
    assert!(trace::trace_enabled());

    trace::trace_csr("0x300", 0xABCD);
    trace::trace_exception(5, 0x8000_1000);
    trace::trace_mmio(0x1000_0000, 4, 0xFF, false);

    // Read back and verify structured output.
    let mut content = String::new();
    std::fs::File::open(&path)
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();

    assert!(content.contains("CSR 0x300 <-"));
    assert!(content.contains("EXC cause=5"));
    assert!(content.contains("MMIO R addr="));
}

#[test]
fn test_trace_init_bad_path() {
    let result = trace::init_trace("/nonexistent/dir/trace.log");
    assert!(result.is_err());
}

// ===== Multithreaded behaviour (#66) =====
//
// The trace destination is per-thread; trace_enabled() is a
// global gate. The tests below pin down both halves: an
// uninitialised child thread must never write to whatever file
// the main thread opened, and two threads that init separately
// must each receive their own events.

#[test]
fn test_trace_uninit_child_does_not_write_to_main_thread_file() {
    let dir = tempfile::tempdir().unwrap();
    let main_path = dir.path().join("main.log");

    trace::init_trace(main_path.to_str().unwrap()).unwrap();
    assert!(trace::trace_enabled());

    // Spawn a child that only emits trace events, without
    // calling init_trace itself. The global ENABLED flag is
    // already on, so trace_enabled() is true, but the child's
    // thread-local TRACE_FILE is None — events must drop.
    let main_path_for_child = main_path.clone();
    let handle = std::thread::spawn(move || {
        assert!(trace::trace_enabled());
        trace::trace_csr("child_csr", 0xDEAD_BEEF);
        trace::trace_exception(7, 0x4000_0000);
        trace::trace_mmio(0x9000_0000, 4, 0xCAFE, true);
        // Sanity: the main thread's file is untouched while the
        // child runs (we only wrote child events).
        let mut s = String::new();
        std::fs::File::open(&main_path_for_child)
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert!(
            !s.contains("child_csr"),
            "child trace events leaked into main thread file: {s}",
        );
    });
    handle.join().unwrap();

    // Now exercise main thread's own trace file: should still
    // be functional and not contain any child markers.
    trace::trace_csr("main_csr", 1);
    let mut s = String::new();
    std::fs::File::open(&main_path)
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert!(s.contains("main_csr"));
    assert!(!s.contains("child_csr"));
    assert!(!s.contains("DEAD_BEEF") && !s.contains("dead_beef"));
}

#[test]
fn test_trace_two_threads_write_to_their_own_files() {
    let dir = tempfile::tempdir().unwrap();
    let path_a = dir.path().join("thread_a.log");
    let path_b = dir.path().join("thread_b.log");

    let pa = path_a.clone();
    let ha = std::thread::spawn(move || {
        trace::init_trace(pa.to_str().unwrap()).unwrap();
        trace::trace_csr("aaa", 0x1111);
    });
    let pb = path_b.clone();
    let hb = std::thread::spawn(move || {
        trace::init_trace(pb.to_str().unwrap()).unwrap();
        trace::trace_csr("bbb", 0x2222);
    });
    ha.join().unwrap();
    hb.join().unwrap();

    let mut sa = String::new();
    std::fs::File::open(&path_a)
        .unwrap()
        .read_to_string(&mut sa)
        .unwrap();
    let mut sb = String::new();
    std::fs::File::open(&path_b)
        .unwrap()
        .read_to_string(&mut sb)
        .unwrap();

    assert!(sa.contains("aaa"));
    assert!(!sa.contains("bbb"));
    assert!(sb.contains("bbb"));
    assert!(!sb.contains("aaa"));
}

#[test]
fn test_trace_enabled_global_but_uninit_thread_drops_events() {
    // This test asserts the contract for trace_enabled():
    // returning true does NOT mean the calling thread has a
    // file open. A child thread that never calls init_trace
    // sees trace_enabled() == true (because the main thread or
    // a sibling has enabled tracing) but its own trace_*
    // calls are no-ops.
    let dir = tempfile::tempdir().unwrap();
    let main_path = dir.path().join("global.log");
    trace::init_trace(main_path.to_str().unwrap()).unwrap();

    let h = std::thread::spawn(|| {
        // No init here.
        let was_enabled = trace::trace_enabled();
        trace::trace_csr("ghost", 0xFF);
        was_enabled
    });
    let was_enabled = h.join().unwrap();
    assert!(
        was_enabled,
        "child should have observed the global ENABLED flag",
    );

    let mut s = String::new();
    std::fs::File::open(&main_path)
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert!(!s.contains("ghost"), "uninit child events must not surface");
}
