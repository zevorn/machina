use std::sync::{Arc, Mutex};

use machina_core::monitor::{MonitorState, VmState};
use machina_monitor::hmp;
use machina_monitor::mmp;
use machina_monitor::service::MonitorService;

fn make_svc() -> Arc<Mutex<MonitorService>> {
    let state = Arc::new(MonitorState::new());
    Arc::new(Mutex::new(MonitorService::new(state)))
}

// ── MonitorState tests ──────────────────────────────

#[test]
fn test_monitor_state_initial() {
    let ms = MonitorState::new();
    assert_eq!(ms.vm_state(), VmState::Running);
    assert!(!ms.is_quit_requested());
}

#[test]
fn test_monitor_state_stop_resume() {
    let ms = Arc::new(MonitorState::new());
    let ms2 = Arc::clone(&ms);

    // Spawn a thread that parks when pause requested.
    let handle = std::thread::spawn(move || {
        ms2.check_pause(); // blocks if PauseRequested
    });

    // Request stop — should block until parked.
    ms.request_stop();
    assert_eq!(ms.vm_state(), VmState::Paused);

    // Resume.
    ms.request_cont();
    handle.join().unwrap();
    assert_eq!(ms.vm_state(), VmState::Running);
}

#[test]
fn test_monitor_state_stop_idempotent() {
    let ms = Arc::new(MonitorState::new());
    let ms2 = Arc::clone(&ms);

    let handle = std::thread::spawn(move || {
        ms2.check_pause();
    });

    ms.request_stop();
    // Second stop when already paused is idempotent.
    ms.request_stop();
    assert_eq!(ms.vm_state(), VmState::Paused);

    ms.request_cont();
    handle.join().unwrap();
}

#[test]
fn test_monitor_state_cont_when_running() {
    let ms = MonitorState::new();
    // cont when already running is a no-op.
    ms.request_cont();
    assert_eq!(ms.vm_state(), VmState::Running);
}

#[test]
fn test_monitor_state_quit() {
    let ms = MonitorState::new();
    assert!(!ms.is_quit_requested());
    ms.request_quit();
    assert!(ms.is_quit_requested());
}

// ── MMP dispatch tests ──────────────────────────────

#[test]
fn test_mmp_qmp_capabilities() {
    let svc = make_svc();
    let resp = mmp::dispatch("qmp_capabilities", &svc);
    assert_eq!(resp["return"], serde_json::json!({}));
}

#[test]
fn test_mmp_query_status_running() {
    let svc = make_svc();
    let resp = mmp::dispatch("query-status", &svc);
    assert_eq!(resp["return"]["running"], true);
}

#[test]
fn test_mmp_unknown_command() {
    let svc = make_svc();
    let resp = mmp::dispatch("nonexistent", &svc);
    assert_eq!(
        resp["error"]["class"],
        "CommandNotFound"
    );
}

#[test]
fn test_mmp_query_cpus_fast() {
    let svc = make_svc();
    let resp = mmp::dispatch("query-cpus-fast", &svc);
    let arr = resp["return"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["cpu-index"], 0);
    assert_eq!(arr[0]["arch"], "riscv64");
}

#[test]
fn test_mmp_quit() {
    let svc = make_svc();
    let resp = mmp::dispatch("quit", &svc);
    assert_eq!(resp["return"], serde_json::json!({}));
    assert!(
        svc.lock().unwrap().state.is_quit_requested()
    );
}

// ── HMP tests ───────────────────────────────────────

#[test]
fn test_hmp_info_status() {
    let svc = make_svc();
    let out = hmp::handle_line("info status", &svc);
    assert_eq!(
        out,
        Some("VM status: running\n".to_string())
    );
}

#[test]
fn test_hmp_info_registers_requires_pause() {
    let svc = make_svc();
    let out = hmp::handle_line("info registers", &svc);
    assert!(
        out.as_ref()
            .unwrap()
            .contains("must be paused")
    );
}

#[test]
fn test_hmp_help() {
    let svc = make_svc();
    let out = hmp::handle_line("help", &svc);
    assert!(out.as_ref().unwrap().contains("info status"));
    assert!(out.as_ref().unwrap().contains("quit"));
}

#[test]
fn test_hmp_unknown_command() {
    let svc = make_svc();
    let out = hmp::handle_line("foobar", &svc);
    assert!(
        out.as_ref()
            .unwrap()
            .contains("unknown command")
    );
}

#[test]
fn test_hmp_quit_returns_none() {
    let svc = make_svc();
    let out = hmp::handle_line("quit", &svc);
    assert!(out.is_none()); // signals exit
}

#[test]
fn test_hmp_empty_line() {
    let svc = make_svc();
    let out = hmp::handle_line("", &svc);
    assert_eq!(out, Some(String::new()));
}

#[test]
fn test_hmp_info_cpus() {
    let svc = make_svc();
    let out = hmp::handle_line("info cpus", &svc);
    assert!(out.as_ref().unwrap().contains("CPU #0"));
}
