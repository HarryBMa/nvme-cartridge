//! The launcher shell: a tray-resident process that watches for cartridges and
//! raises a window when one is inserted.

mod cartridge;
mod eject;
mod launch;
mod manifest;
mod settings;
mod trust;
mod volumes;

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use cartridge::{Cartridge, Session};
use settings::Settings;
use volumes::VolumeEvent;

const MAIN_WINDOW: &str = "main";
const EVENT_INSERTED: &str = "cartridge://inserted";
const EVENT_REMOVED: &str = "cartridge://removed";
const EVENT_STATUS: &str = "cartridge://status";

#[derive(Default)]
struct AppState {
    /// Live cartridges, keyed by mount path.
    sessions: Mutex<HashMap<String, Session>>,
    /// The one the window is currently showing.
    showing: Mutex<Option<String>>,
}

/// What the frontend gets back from an action.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionResult {
    ok: bool,
    message: String,
}

impl ActionResult {
    fn ok(message: impl Into<String>) -> Self {
        ActionResult {
            ok: true,
            message: message.into(),
        }
    }
    fn err(message: impl Into<String>) -> Self {
        ActionResult {
            ok: false,
            message: message.into(),
        }
    }
}

#[tauri::command]
fn current_cartridge(state: State<'_, AppState>) -> Option<Cartridge> {
    let showing = state.showing.lock().ok()?.clone()?;
    let sessions = state.sessions.lock().ok()?;
    let session = sessions.get(&showing)?;
    cartridge::inspect(&session.volume, &Settings::load()).ok()
}

#[tauri::command]
fn play(id: String, state: State<'_, AppState>, window: WebviewWindow) -> ActionResult {
    let session = {
        let sessions = match state.sessions.lock() {
            Ok(s) => s,
            Err(_) => return ActionResult::err("launcher state is poisoned"),
        };
        match sessions.get(&id) {
            Some(s) => s.clone(),
            None => return ActionResult::err("that cartridge is no longer connected"),
        }
    };

    // Re-check trust at the moment of launch rather than trusting the payload
    // the window was drawn from; the script may have changed since.
    let fresh = match cartridge::session(&session.volume) {
        Ok(s) => s,
        Err(e) => return ActionResult::err(format!("could not re-read the cartridge: {e}")),
    };

    match launch::launch(&fresh.target, &fresh.volume.mount, &fresh.trust) {
        Ok(()) => {
            let _ = window.hide();
            ActionResult::ok("Launching")
        }
        Err(e) => ActionResult::err(e.to_string()),
    }
}

#[tauri::command]
fn eject_cartridge(id: String, state: State<'_, AppState>, window: WebviewWindow) -> ActionResult {
    let session = {
        let sessions = match state.sessions.lock() {
            Ok(s) => s,
            Err(_) => return ActionResult::err("launcher state is poisoned"),
        };
        match sessions.get(&id) {
            Some(s) => s.clone(),
            None => return ActionResult::err("that cartridge is no longer connected"),
        }
    };

    match eject::eject(&session.volume) {
        Ok(()) => {
            let _ = window.hide();
            ActionResult::ok("Safe to remove")
        }
        Err(e) => {
            // An eject that unmounted but could not power off still leaves the
            // volume gone, which is what the user actually cares about.
            if !eject::is_still_mounted(&session.volume.mount) {
                let _ = window.hide();
                return ActionResult::ok("Safe to remove");
            }
            ActionResult::err(e.to_string())
        }
    }
}

/// Add this cartridge's digest to the shared trust list, after an explicit click.
#[tauri::command]
fn trust_cartridge(id: String, state: State<'_, AppState>) -> ActionResult {
    let session = {
        let sessions = match state.sessions.lock() {
            Ok(s) => s,
            Err(_) => return ActionResult::err("launcher state is poisoned"),
        };
        match sessions.get(&id) {
            Some(s) => s.clone(),
            None => return ActionResult::err("that cartridge is no longer connected"),
        }
    };

    let Some(digest) = session.trust.digest() else {
        return ActionResult::err("there is nothing to trust on this cartridge");
    };
    match trust::trust_digest(digest) {
        Ok(()) => ActionResult::ok("Cartridge trusted"),
        Err(e) => ActionResult::err(format!("could not write the trust list: {e}")),
    }
}

#[tauri::command]
fn dismiss(window: WebviewWindow) {
    let _ = window.hide();
}

/// Bring the window forward for a cartridge.
fn present(app: &AppHandle, payload: &Cartridge) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let _ = window.emit(EVENT_INSERTED, payload);
    let _ = window.show();
    let _ = window.center();
    // Raise above a running game or a full-screen browser, then release the
    // constraint so the user can tab away from the launcher normally.
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();
    let _ = window.set_always_on_top(false);
}

/// Bridge the polling watcher onto the Tauri event loop.
fn spawn_watcher(app: AppHandle) {
    let events = volumes::watch();
    std::thread::Builder::new()
        .name("cartridge-bridge".into())
        .spawn(move || {
            for event in events {
                let state = app.state::<AppState>();
                match event {
                    VolumeEvent::Inserted(volume) => {
                        let settings = Settings::load();
                        // A volume without a valid manifest is just a disk.
                        let Ok(view) = cartridge::inspect(&volume, &settings) else {
                            continue;
                        };
                        let Ok(session) = cartridge::session(&volume) else {
                            continue;
                        };

                        let id = view.id.clone();
                        if let Ok(mut sessions) = state.sessions.lock() {
                            sessions.insert(id.clone(), session);
                        }
                        if let Ok(mut showing) = state.showing.lock() {
                            *showing = Some(id);
                        }

                        if view.autolaunch {
                            // Auto-launch still draws the window first, so the
                            // user can see what fired and eject from the same
                            // place.
                            present(&app, &view);
                            if let Ok(sessions) = state.sessions.lock() {
                                if let Some(session) = sessions.get(&view.id) {
                                    let result = launch::launch(
                                        &session.target,
                                        &session.volume.mount,
                                        &session.trust,
                                    );
                                    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                                        let _ = window.emit(
                                            EVENT_STATUS,
                                            match &result {
                                                Ok(()) => ActionResult::ok("Launching"),
                                                Err(e) => ActionResult::err(e.to_string()),
                                            },
                                        );
                                    }
                                }
                            }
                        } else {
                            present(&app, &view);
                        }
                    }

                    VolumeEvent::Removed(mount) => {
                        let id = mount.to_string_lossy().into_owned();
                        if let Ok(mut sessions) = state.sessions.lock() {
                            sessions.remove(&id);
                        }
                        let was_showing = state
                            .showing
                            .lock()
                            .map(|s| s.as_deref() == Some(id.as_str()))
                            .unwrap_or(false);

                        if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                            let _ = window.emit(EVENT_REMOVED, &id);
                            if was_showing {
                                // The cartridge the user was looking at is gone;
                                // leaving a stale window up would be a lie.
                                let _ = window.hide();
                            }
                        }
                        if was_showing {
                            if let Ok(mut showing) = state.showing.lock() {
                                *showing = None;
                            }
                        }
                    }
                }
            }
        })
        .expect("spawn cartridge bridge");
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show launcher", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::with_id("cartridge-tray")
        .icon(app.default_window_icon().cloned().expect("bundled icon"))
        .tooltip("PC Cartridge System — waiting for a cartridge")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            current_cartridge,
            play,
            eject_cartridge,
            trust_cartridge,
            dismiss
        ])
        .on_window_event(|window, event| {
            // Closing the window parks the daemon in the tray rather than
            // stopping cartridge detection.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            build_tray(app.handle())?;
            spawn_watcher(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the cartridge launcher");
}
