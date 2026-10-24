//! QR Drop -- send files to a phone by scanning a QR code.
//!
//! - [`args`]: what a start asks for, debounced multi-selections
//! - [`batch`] / [`handoff`]: turning OS requests into sessions
//! - [`sessions`] / [`ui`]: starting sessions and their windows
//! - [`app_core`]: shared state and the window view model
//! - [`commands`] / [`update`]: the frontend's IPC surface
//! - [`share`] / [`server`] / [`net`] / [`qr`]: the transfer itself

mod app_core;
mod args;
mod batch;
mod commands;
pub mod disk;
mod handoff;
mod integration;
pub mod net;
pub mod qr;
pub mod server;
mod sessions;
mod settings;
pub mod share;
mod startup_error;
mod ui;
mod update;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

use app_core::AppCore;
use args::Request;
use share::{EventSink, ShareConfig, ShareEvent, ShareManager, TextSink};

/// How long a start without arguments waits before showing the drop zone:
/// on macOS the files of `open -a` arrive as an event shortly after launch.
const OPEN_EVENT_GRACE: Duration = if cfg!(target_os = "macos") {
    Duration::from_millis(700)
} else {
    Duration::ZERO
};

/// When a request arrived; only matters when it carries nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// This process was started with the request.
    Startup,
    /// The running app got it (second instance, macOS open event).
    Running,
}

fn event_name(event: &ShareEvent) -> &'static str {
    match event {
        ShareEvent::Connected { .. } => "share://connected",
        ShareEvent::Progress { .. } => "share://progress",
        ShareEvent::Interrupted { .. } => "share://interrupted",
        ShareEvent::Done { .. } => "share://done",
        ShareEvent::Expired { .. } => "share://expired",
        ShareEvent::Upload { .. } => "share://upload",
        ShareEvent::Received { .. } => "share://received",
        ShareEvent::TextReceived { .. } => "share://text",
    }
}

/// Sends a session event to the window of its session only.
fn emit_to_session(app: &AppHandle, event: &ShareEvent) {
    let label = ui::session_label(event.id());
    if let Err(e) = app.emit_to(label, event_name(event), event) {
        log::debug!("Event nicht zugestellt: {e}");
    }
}

/// Remembers files saved by a receive session for "Ordner öffnen".
fn record_received_file(app: &AppHandle, event: &ShareEvent) {
    if let ShareEvent::Received { id, path, .. } = event {
        if let Some(core) = app_core::get(app) {
            core.record_received(id, path.clone());
        }
    }
}

/// Runs `f` on the async runtime instead of the calling thread: creating
/// windows from an event-loop callback can deadlock on Windows.
fn run_detached(app: &AppHandle, f: fn(&AppHandle)) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { f(&app) });
}

/// Carries out a request of a start, a second instance or the OS.
fn dispatch(app: &AppHandle, request: Request, origin: Origin) {
    match request {
        Request::Clipboard => run_detached(app, sessions::share_clipboard),
        Request::Receive => run_detached(app, sessions::start_receive),
        Request::Paths(paths) => batch::queue_paths(app, paths),
        Request::Idle => show_idle_window(app, origin),
    }
}

/// Nothing to share: show the welcome (first run on macOS) or the drop
/// zone. A fresh start first waits for files that macOS delivers as an
/// event, and stays quiet if they came.
fn show_idle_window(app: &AppHandle, origin: Origin) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if origin == Origin::Startup {
            tokio::time::sleep(OPEN_EVENT_GRACE).await;
            let busy = app_core::get(&app)
                .is_some_and(|core| core.collector.has_pending() || core.has_sessions());
            if busy || !app.webview_windows().is_empty() {
                return;
            }
        }
        let result = if needs_welcome(&app) {
            ui::open_welcome(&app)
        } else {
            ui::open_dropzone(&app)
        };
        if let Err(e) = result {
            log::error!("Fenster konnte nicht geöffnet werden: {e}");
            ui::exit_if_idle(&app);
        }
    });
}

/// macOS shows a welcome window until it was confirmed once.
fn needs_welcome(app: &AppHandle) -> bool {
    cfg!(target_os = "macos")
        && app
            .path()
            .app_config_dir()
            .is_ok_and(|dir| !settings::load(&dir).welcomed)
}

fn setup(app: &mut tauri::App, started: Instant) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let config_dir = app.path().app_config_dir()?;
    let mut prefs = settings::load(&config_dir);

    let manager = Arc::new(ShareManager::new(ShareConfig::default()));
    let sink: EventSink = {
        let handle = handle.clone();
        Arc::new(move |event| {
            record_received_file(&handle, &event);
            emit_to_session(&handle, &event);
        })
    };
    let text_sink: TextSink = {
        let handle = handle.clone();
        Arc::new(move |text: &str| {
            use tauri_plugin_clipboard_manager::ClipboardExt;
            handle.clipboard().write_text(text.to_string()).is_ok()
        })
    };
    let uploads = Arc::new(server::receive::Uploads::default());
    let state = server::AppState {
        manager: manager.clone(),
        sink,
        text_sink,
        uploads: uploads.clone(),
    };
    let server = tauri::async_runtime::block_on(server::start(state, prefs.port))?;
    settings::remember_port(&config_dir, &mut prefs, server.port);
    let core = Arc::new(AppCore::new(manager, server, uploads));
    core.note_request(started);
    app.manage(core.clone());
    app.manage(update::FoundUpdate::default());

    spawn_expiry_loop(handle.clone(), core);
    integration::on_startup(&handle);

    let cwd = std::env::current_dir().ok();
    let request = args::parse_request(std::env::args_os().skip(1), cwd.as_deref());
    dispatch(&handle, request, Origin::Startup);
    Ok(())
}

/// Every second: invalidates expired tokens, tells their windows, and drops
/// unfinished uploads of sessions that are gone.
fn spawn_expiry_loop(app: AppHandle, core: Arc<AppCore>) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            for id in core.manager.take_expired(Instant::now()) {
                emit_to_session(&app, &ShareEvent::Expired { id });
            }
            core.uploads.sweep(&core.manager);
        }
    });
}

fn on_second_instance(app: &AppHandle, argv: Vec<String>, cwd: String) {
    if let Some(core) = app_core::get(app) {
        core.note_request(Instant::now());
    }
    let request = args::parse_request(argv.iter().skip(1), Some(Path::new(&cwd)));
    dispatch(app, request, Origin::Running);
}

fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::Destroyed = event {
        if let Some(id) = window.label().strip_prefix(ui::SESSION_PREFIX) {
            if let Some(core) = app_core::get(window.app_handle()) {
                core.forget(id);
            }
        }
    }
}

fn on_run_event(app: &AppHandle, event: RunEvent) {
    match event {
        // Finder "Öffnen mit", Dock drop, Quick Action (`open -b`).
        #[cfg(target_os = "macos")]
        RunEvent::Opened { urls } => {
            let paths: Vec<std::path::PathBuf> = urls
                .iter()
                .filter_map(|u| u.to_file_path().ok())
                .filter(|p| p.exists())
                .collect();
            if !paths.is_empty() {
                if let Some(core) = app_core::get(app) {
                    core.note_request(Instant::now());
                }
                dispatch(app, Request::Paths(paths), Origin::Running);
            }
        }
        RunEvent::Exit => {
            if let Some(core) = app_core::get(app) {
                core.shutdown();
            }
        }
        _ => {}
    }
}

fn log_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    let mut builder = tauri_plugin_log::Builder::new()
        .clear_targets()
        .target(Target::new(TargetKind::LogDir { file_name: None }))
        .rotation_strategy(RotationStrategy::KeepSome(3))
        .max_file_size(1_000_000)
        .level(log::LevelFilter::Info);
    if cfg!(debug_assertions) {
        builder = builder
            .target(Target::new(TargetKind::Stdout))
            .level(log::LevelFilter::Debug);
    }
    builder.build()
}

pub fn run() {
    let started = Instant::now();
    let app = tauri::Builder::default()
        // Must be the first plugin (see plugin docs).
        .plugin(tauri_plugin_single_instance::init(on_second_instance))
        .plugin(log_plugin())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Keep in sync with the command list in build.rs.
        .invoke_handler(tauri::generate_handler![
            commands::get_share,
            commands::select_network,
            commands::renew_share,
            commands::cancel_share,
            commands::reveal_received,
            commands::share_paths,
            commands::share_clipboard,
            commands::receive_from_phone,
            commands::pick_files,
            commands::check_update,
            commands::install_update,
            commands::finish_welcome,
        ])
        .setup(move |app| setup(app, started))
        .on_window_event(on_window_event)
        .build(tauri::generate_context!());

    match app {
        Ok(app) => app.run(on_run_event),
        Err(e) => {
            startup_error::report(&e);
            std::process::exit(1);
        }
    }
}
