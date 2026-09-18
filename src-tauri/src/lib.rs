//! Spectrum — the Tauri 2 control-panel shell.
//!
//! The engine (poll → classify → route → drip) runs in a background thread with its
//! own Tokio runtime, exclusively owning an `Engine`. After each cycle it publishes a
//! cheap snapshot (status + recent log) into shared state. The webview's commands only
//! READ that snapshot and push control flags (start/stop/dry/tuning) — they never block
//! on the engine, so a long poll never freezes the UI. Mirrors v4's Arc<Mutex<>> pattern.
//!
//! CR-1 chunk 0 closes the two places this shell could fail in silence.
//! * **The disk log opens in [`run`]**, before the engine thread exists, so a failure
//!   before `Engine::new` still leaves a record. The release build hides the console
//!   (`main.rs:4`), so a `println!` or an `eprintln!` here reaches nobody.
//! * **A poisoned mutex is recovered, not swallowed.** It used to answer with an empty
//!   snapshot for ever, and the dashboard simply stopped moving. See [`guard`].

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use spectrum_engine::config::Config;
use spectrum_engine::engine::{Engine, Status};
use spectrum_engine::log;

struct Shared {
    running: AtomicBool,
    dry: AtomicBool,
    config_dirty: AtomicBool,
    status: Mutex<Option<Status>>,
    log: Mutex<Vec<String>>,
    cfg_path: String,
    /// One `panel.mutex.poisoned` record per mutex, not one per tick.
    status_poisoned: AtomicBool,
    log_poisoned: AtomicBool,
}

/// Take a shared lock without ever freezing the panel on it.
///
/// `Mutex::lock` returns `Err` once any thread has panicked while holding it. This shell
/// used to answer that with `unwrap_or(None)` and `unwrap_or_default()`, so one panic in
/// the engine thread left the dashboard showing empty values for ever, with no line in
/// the log, the ring or the console. The guard is recovered instead — the value behind it
/// is the last snapshot that was published — and the poisoning is recorded once per
/// mutex. Mirrors the log sink's own recovery (`crates\engine\src\log\sink.rs:45-49`).
/// [CR-1 chunk 0 · DoD item 2]
fn guard<'a, T>(
    mutex: &'a Mutex<T>,
    reported: &AtomicBool,
    name: &str,
    shows: &str,
) -> MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(g) => g,
        Err(poisoned) => {
            if !reported.swap(true, Ordering::Relaxed) {
                log::error("panel.mutex.poisoned")
                    .field("mutex", name)
                    .field("shows", shows)
                    .not_doing(
                        "trust this snapshot as complete",
                        "a thread panicked while holding this lock, so its update never finished; the panel recovers the guard and keeps serving what was published before the panic",
                    )
                    .emit();
            }
            poisoned.into_inner()
        }
    }
}

#[derive(serde::Serialize)]
struct StatusView {
    running: bool,
    engine: Option<Status>,
}

#[derive(serde::Deserialize)]
struct Tuning {
    min_severity: u64,
    poll_minutes: u64,
    drip_seconds: u64,
}

#[tauri::command]
fn status(shared: tauri::State<'_, Arc<Shared>>) -> StatusView {
    StatusView {
        running: shared.running.load(Ordering::Relaxed),
        engine: status_guard(&shared).clone(),
    }
}

#[tauri::command]
fn recent_log(shared: tauri::State<'_, Arc<Shared>>) -> Vec<String> {
    log_guard(&shared).clone()
}

/// The status snapshot the dashboard tiles read.
fn status_guard(shared: &Arc<Shared>) -> MutexGuard<'_, Option<Status>> {
    guard(
        &shared.status,
        &shared.status_poisoned,
        "status",
        "the dashboard tiles: seen, queued, posted and dropped",
    )
}

/// The recent-activity lines the panel's log pane reads.
fn log_guard(shared: &Arc<Shared>) -> MutexGuard<'_, Vec<String>> {
    guard(&shared.log, &shared.log_poisoned, "log", "the panel's recent-activity pane")
}

#[tauri::command]
fn start(shared: tauri::State<'_, Arc<Shared>>) {
    shared.running.store(true, Ordering::Relaxed);
}

#[tauri::command]
fn stop(shared: tauri::State<'_, Arc<Shared>>) {
    shared.running.store(false, Ordering::Relaxed);
}

#[tauri::command]
fn set_dry(shared: tauri::State<'_, Arc<Shared>>, dry: bool) {
    shared.dry.store(dry, Ordering::Relaxed);
}

#[tauri::command]
fn get_tuning(shared: tauri::State<'_, Arc<Shared>>) -> serde_json::Value {
    let v: serde_json::Value = std::fs::read_to_string(&shared.cfg_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let bands: Vec<String> = v
        .get("webhooks")
        .and_then(|w| w.as_object())
        .map(|w| w.keys().cloned().collect())
        .unwrap_or_default();
    serde_json::json!({
        "min_severity": v.get("min_severity").and_then(|x| x.as_u64()).unwrap_or(2),
        "poll_minutes": v.get("poll_minutes").and_then(|x| x.as_u64()).unwrap_or(10),
        "drip_seconds": v.get("drip_seconds").and_then(|x| x.as_u64()).unwrap_or(90),
        "webhook_bands": bands,
    })
}

#[tauri::command]
fn set_tuning(shared: tauri::State<'_, Arc<Shared>>, tuning: Tuning) -> Result<(), String> {
    let mut v: serde_json::Value = std::fs::read_to_string(&shared.cfg_path)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))?;
    if let Some(obj) = v.as_object_mut() {
        obj.insert("min_severity".into(), serde_json::json!(tuning.min_severity.clamp(1, 4)));
        obj.insert("poll_minutes".into(), serde_json::json!(tuning.poll_minutes.max(1)));
        obj.insert("drip_seconds".into(), serde_json::json!(tuning.drip_seconds.max(5)));
    }
    std::fs::write(
        &shared.cfg_path,
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    shared.config_dirty.store(true, Ordering::Relaxed);
    Ok(())
}

fn publish(shared: &Arc<Shared>, engine: &Engine) {
    *status_guard(shared) = Some(engine.status());
    *log_guard(shared) = engine.recent_log(60);
}

/// The engine background thread: own a Tokio runtime, drive the cycle, publish snapshots.
fn drive(shared: Arc<Shared>) {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            // This used to be an eprintln! and a return. The release panel hides the
            // console, and `log::init` had not run yet, so the window then sat there for
            // ever with an empty dashboard and no record anywhere. [CR-1 chunk 0]
            log::error("panel.runtime.failed")
                .err(e)
                .not_doing(
                    "poll, classify or post anything for as long as this panel runs",
                    "the engine thread could not build its Tokio runtime; the window stays open with an empty dashboard until it is restarted",
                )
                .emit();
            return;
        }
    };
    rt.block_on(async move {
        let mut engine = match Engine::new(&shared.cfg_path) {
            Ok(e) => e,
            Err(e) => {
                // `Engine::new` has already written the reason to the log; this line is
                // the same fact in the window, where the operator is looking.
                *log_guard(&shared) = vec![format!("engine init failed: {e}")];
                return;
            }
        };
        if engine.seen_empty() {
            engine.seed().await;
        }
        publish(&shared, &engine);

        let mut last_poll = Instant::now();
        let mut first = true;
        loop {
            if shared.config_dirty.swap(false, Ordering::Relaxed) {
                engine.reload_config();
            }
            if shared.running.load(Ordering::Relaxed) {
                let poll_secs = engine.cfg.poll_minutes.max(1) * 60;
                if first || last_poll.elapsed().as_secs() >= poll_secs {
                    engine.poll().await;
                    last_poll = Instant::now();
                    first = false;
                }
                let dry = shared.dry.load(Ordering::Relaxed);
                engine.drip(dry).await;
                publish(&shared, &engine);
                tokio::time::sleep(Duration::from_secs(engine.cfg.drip_seconds.max(1))).await;
            } else {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    });
}

/// Open this run's log file, before anything can fail without one.
///
/// `Engine::new` opens it as well, but that happens inside the engine thread — so every
/// failure before it (a runtime that will not build, a config that will not load) had
/// nowhere to go, because the release build hides the console. `log::init` pins one file
/// per resolved path, so the engine thread's later call is a no-op. [CR-1 chunk 0]
fn open_log(cfg_path: &str) {
    let base = Path::new(cfg_path).parent().unwrap_or_else(|| Path::new("."));
    // A config that will not load is exactly the kind of failure the log exists to
    // record, so the file opens either way — with the defaults when the config is gone.
    let loaded = Config::load(cfg_path);
    let logging = loaded.as_ref().map(|c| c.logging.clone()).unwrap_or_default();
    if let Err(e) = log::init(&logging, base) {
        log::error("log.init.failed")
            .field("dir", &logging.dir)
            .err(e)
            .not_doing(
                "write a log file for this run",
                "the release panel hides the console, so nothing this run does is recorded anywhere",
            )
            .emit();
    }
    if let Err(e) = loaded {
        log::warn("panel.config.unreadable")
            .field("path", cfg_path)
            .err(e)
            .not_doing(
                "apply the configured logging settings",
                "this run logs with the defaults; the engine thread reports the config failure itself and then stops",
            )
            .emit();
    }
}

pub fn run() {
    let cfg_path =
        std::env::var("SPECTRUM_CONFIG").unwrap_or_else(|_| "config.local.json".to_string());
    open_log(&cfg_path);
    log::info("panel.started")
        .field("config", &cfg_path)
        .flag("autostart", std::env::var("SPECTRUM_AUTOSTART").is_ok())
        .emit();
    let shared = Arc::new(Shared {
        running: AtomicBool::new(std::env::var("SPECTRUM_AUTOSTART").is_ok()),
        dry: AtomicBool::new(false),
        config_dirty: AtomicBool::new(false),
        status: Mutex::new(None),
        log: Mutex::new(Vec::new()),
        cfg_path,
        status_poisoned: AtomicBool::new(false),
        log_poisoned: AtomicBool::new(false),
    });
    {
        let s = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("spectrum-engine".into())
            .spawn(move || drive(s))
            .expect("failed to spawn engine thread");
    }
    tauri::Builder::default()
        // A second launch just focuses the existing window — it never starts a second
        // engine (which would double-post). Single-instance MUST be the first plugin.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            use tauri::Manager;
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .manage(shared)
        .setup(|app| {
            use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
            use tauri::tray::TrayIconBuilder;
            use tauri::Manager;

            let handle = app.handle();
            let show = MenuItem::with_id(handle, "show", "Show Spectrum", true, None::<&str>)?;
            let start = MenuItem::with_id(handle, "start", "Start", true, None::<&str>)?;
            let stop = MenuItem::with_id(handle, "stop", "Stop", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(handle)?;
            let quit = MenuItem::with_id(handle, "quit", "Quit Spectrum", true, None::<&str>)?;
            let menu = Menu::with_items(handle, &[&show, &start, &stop, &sep, &quit])?;
            let icon = app.default_window_icon().cloned().expect("bundled window icon");

            TrayIconBuilder::with_id("spectrum-tray")
                .icon(icon)
                .tooltip("Spectrum — news watcher")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    let shared = app.state::<Arc<Shared>>();
                    match event.id.as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "start" => shared.running.store(true, Ordering::Relaxed),
                        "stop" => shared.running.store(false, Ordering::Relaxed),
                        "quit" => app.exit(0),
                        _ => {}
                    }
                })
                .build(handle)?;

            // Autostart (Riley's boot): the engine already runs (SPECTRUM_AUTOSTART);
            // start hidden so it boots straight into the tray rather than popping a window.
            if std::env::var("SPECTRUM_AUTOSTART").is_ok() {
                if let Some(w) = handle.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window hides it to the tray instead of quitting —
            // "Quit Spectrum" in the tray menu is the real exit.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            status, recent_log, start, stop, set_dry, get_tuning, set_tuning
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Spectrum panel");
}
