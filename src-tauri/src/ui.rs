//! Creating, finding and closing windows.

use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Wry};

use crate::app_core;

/// Label prefix of share and receive windows: events are routed by it and
/// capabilities are granted by it (`capabilities/session.json`).
pub const SESSION_PREFIX: &str = "share-";
pub const DROPZONE: &str = "dropzone";
pub const WELCOME: &str = "welcome";

const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 540.0;
/// Offset for stacked session windows (logical pixels).
const STACK_OFFSET: f64 = 28.0;

/// The view a session window loads.
#[derive(Debug, Clone, Copy)]
pub enum SessionView {
    Share,
    Receive,
}

impl SessionView {
    fn name(self) -> &'static str {
        match self {
            Self::Share => "share",
            Self::Receive => "receive",
        }
    }
}

pub fn session_label(id: &str) -> String {
    format!("{SESSION_PREFIX}{id}")
}

pub fn has_session_window(app: &AppHandle, id: &str) -> bool {
    app.get_webview_window(&session_label(id)).is_some()
}

fn effects() -> tauri::utils::config::WindowEffectsConfig {
    #[cfg(target_os = "macos")]
    {
        // Liquid Glass on macOS 26+, HUD vibrancy as fallback on older systems.
        EffectsBuilder::new()
            .effects([Effect::LiquidGlassRegular, Effect::HudWindow])
            .state(EffectState::FollowsWindowActiveState)
            .radius(20.0)
            .build()
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Mica on Windows 11; Windows 10 falls back to CSS glass in the UI.
        EffectsBuilder::new()
            .effect(Effect::Mica)
            .state(EffectState::FollowsWindowActiveState)
            .build()
    }
}

/// The frameless glass window all views share (CLAUDE.md §7.1).
fn glass_window<'a>(
    app: &'a AppHandle,
    label: &str,
    query: &str,
) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
    WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(format!("index.html?{query}").into()),
    )
    .title("QR Drop")
    .inner_size(WIDTH, HEIGHT)
    .resizable(false)
    .maximizable(false)
    .minimizable(true)
    .decorations(false)
    .transparent(true)
    .shadow(true)
    .effects(effects())
}

/// Opens the always-on-top window of a share or receive session, offset from
/// those already open. The position is chosen before the window is built,
/// so it never visibly jumps.
pub fn open_session_window(app: &AppHandle, id: &str, view: SessionView) -> tauri::Result<()> {
    let stacked = app
        .webview_windows()
        .keys()
        .filter(|label| label.starts_with(SESSION_PREFIX))
        .count();
    let builder = glass_window(
        app,
        &session_label(id),
        &format!("view={}&id={id}", view.name()),
    )
    .always_on_top(true);
    let builder = placed(builder, app, stacked);
    let window = builder.build()?;
    if let Err(e) = window.set_focus() {
        log::debug!("Fenster nicht fokussiert: {e}");
    }
    Ok(())
}

/// Logical position for a new window: centred in the work area (without
/// the taskbar) of the monitor under the mouse -- where the user just
/// right-clicked -- moved down and right by `stacked` × [`STACK_OFFSET`].
/// Computed here because `center()` proved unreliable for these undecorated
/// windows on Windows (they landed at the screen edge or on another monitor).
fn window_position(app: &AppHandle, stacked: usize) -> Option<(f64, f64)> {
    let under_mouse = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten());
    let monitor = match under_mouse {
        Some(monitor) => monitor,
        None => app.primary_monitor().ok().flatten()?,
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let origin = area.position.to_logical::<f64>(scale);
    let size = area.size.to_logical::<f64>(scale);
    let offset = STACK_OFFSET * stacked as f64;
    Some((
        origin.x + (size.width - WIDTH) / 2.0 + offset,
        origin.y + (size.height - HEIGHT) / 2.0 + offset,
    ))
}

/// Places a window builder at [`window_position`]; `center()` only as the
/// fallback when no monitor information is available.
fn placed<'a>(
    builder: WebviewWindowBuilder<'a, Wry, AppHandle>,
    app: &AppHandle,
    stacked: usize,
) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
    match window_position(app, stacked) {
        Some((x, y)) => builder.position(x, y),
        None => builder.center(),
    }
}

fn focus_or_create(app: &AppHandle, label: &str) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(label) {
        window.unminimize()?;
        return window.set_focus();
    }
    placed(glass_window(app, label, &format!("view={label}")), app, 0).build()?;
    Ok(())
}

pub fn open_dropzone(app: &AppHandle) -> tauri::Result<()> {
    focus_or_create(app, DROPZONE)
}

pub fn open_welcome(app: &AppHandle) -> tauri::Result<()> {
    focus_or_create(app, WELCOME)
}

/// Closes a window by label, if it is open.
pub fn close(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        if let Err(e) = window.close() {
            log::warn!("Fenster „{label}“ nicht geschlossen: {e}");
        }
    }
}

/// Ends the process when no window is open and none is on its way. Tauri
/// only exits when the last window closes, so a start whose window failed
/// to open would otherwise keep running invisibly.
pub fn exit_if_idle(app: &AppHandle) {
    // On a cold start the first window only appears once the event loop
    // runs -- possibly after the selection's debounce ended. A begun
    // selection or a prepared session therefore counts as "on its way".
    let on_its_way = app_core::get(app).is_some_and(|core| {
        core.collector.has_pending() || core.batches.is_busy() || core.has_sessions()
    });
    if app.webview_windows().is_empty() && !on_its_way {
        log::info!("Kein Fenster offen, QR Drop wird beendet");
        app.exit(0);
    }
}
