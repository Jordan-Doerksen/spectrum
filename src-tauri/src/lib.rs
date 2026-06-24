//! Spectrum — the Tauri 2 control-panel shell.
//!
//! The engine (poll → classify → route → drip) runs in a background thread with its
//! own Tokio runtime, exclusively owning an `Engine`. After each cycle it publishes a
//! cheap snapshot (status + recent log) into shared state. The webview's commands only
//! READ that snapshot and push control flags (start/stop/dry/tuning) — they never block
//! on the engine, so a long poll never freezes the UI. Mirrors v4's Arc<Mutex<>> pattern.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use spectrum_engine::engine::{Engine, Status};

struct Shared {
    running: AtomicBool,
    dry: AtomicBool,
    config_dirty: AtomicBool,
    status: Mutex<Option<Status>>,
    log: Mutex<Vec<String>>,
    cfg_path: String,
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
        engine: shared.status.lock().map(|g| g.clone()).unwrap_or(None),
    }
}

#[tauri::command]
fn recent_log(shared: tauri::State<'_, Arc<Shared>>) -> Vec<String> {
    shared.log.lock().map(|g| g.clone()).unwrap_or_default()
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
    if let Ok(mut s) = shared.status.lock() {
        *s = Some(engine.status());
    }
    if let Ok(mut l) = shared.log.lock() {
        *l = engine.recent_log(60);
    }
}

/// The engine background thread: own a Tokio runtime, drive the cycle, publish snapshots.
fn drive(shared: Arc<Shared>) {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("spectrum: failed to build runtime: {e}");
            return;
        }
    };
    rt.block_on(async move {
        let mut engine = match Engine::new(&shared.cfg_path) {
            Ok(e) => e,
            Err(e) => {
                if let Ok(mut l) = shared.log.lock() {
                    *l = vec![format!("engine init failed: {e}")];
                }
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

pub fn run() {
    let cfg_path =
        std::env::var("SPECTRUM_CONFIG").unwrap_or_else(|_| "config.local.json".to_string());
    let shared = Arc::new(Shared {
        running: AtomicBool::new(std::env::var("SPECTRUM_AUTOSTART").is_ok()),
        dry: AtomicBool::new(false),
        config_dirty: AtomicBool::new(false),
        status: Mutex::new(None),
        log: Mutex::new(Vec::new()),
        cfg_path,
    });
    {
        let s = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("spectrum-engine".into())
            .spawn(move || drive(s))
            .expect("failed to spawn engine thread");
    }
    tauri::Builder::default()
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
