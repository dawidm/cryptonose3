//! Experimental "close to tray": a tray icon with a menu, and closing the window hides it instead
//! of quitting. The icon only exists while the setting is on.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, WindowEvent};

use crate::AppState;

const TRAY_ID: &str = "main";

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn toggle_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            show_main_window(app);
        }
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let t = app.state::<AppState>().settings.lock().unwrap().toggles.clone();
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "show", "Show / hide window", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &CheckMenuItem::with_id(app, "sound", "Sound", true, t.sound, None::<&str>)?,
            &CheckMenuItem::with_id(app, "browser", "Browser", true, t.browser, None::<&str>)?,
            &CheckMenuItem::with_id(app, "notification", "Notify", true, t.notification, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit Cryptonose 3", true, None::<&str>)?,
        ],
    )
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "show" => toggle_main_window(app),
        "quit" => app.exit(0),
        "sound" | "browser" | "notification" => {
            let state = app.state::<AppState>();
            let settings = {
                let mut s = state.settings.lock().unwrap();
                let t = &mut s.toggles;
                let v = match id {
                    "sound" => &mut t.sound,
                    "browser" => &mut t.browser,
                    _ => &mut t.notification,
                };
                *v = !*v;
                s.clone()
            };
            state.save();
            let _ = app.emit("cn:settings", &settings);
        }
        _ => {}
    }
}

/// Create or remove the tray icon to match the setting, and keep its menu in sync with the toggles.
pub fn sync(app: &AppHandle) {
    let enabled = app.state::<AppState>().settings.lock().unwrap().general.close_to_tray;
    match (enabled, app.tray_by_id(TRAY_ID)) {
        (true, Some(tray)) => {
            if let Ok(menu) = build_menu(app) {
                let _ = tray.set_menu(Some(menu));
            }
        }
        (true, None) => {
            if let Err(e) = create(app) {
                tracing::warn!("creating tray icon failed: {e}");
            }
        }
        (false, Some(_)) => {
            let _ = app.remove_tray_by_id(TRAY_ID);
        }
        (false, None) => {}
    }
}

fn create(app: &AppHandle) -> tauri::Result<TrayIcon> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Cryptonose 3")
        .menu(&build_menu(app)?)
        // on Linux only the menu works, so a left click opens it everywhere for consistency
        .show_menu_on_left_click(cfg!(target_os = "linux"))
        .on_menu_event(|app, e| on_menu(app, e.id.as_ref()))
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                toggle_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)
}

/// Window close: hide to the tray when enabled, otherwise quit as usual.
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        let app = window.app_handle();
        let to_tray = app.state::<AppState>().settings.lock().unwrap().general.close_to_tray && app.tray_by_id(TRAY_ID).is_some();
        if to_tray {
            api.prevent_close();
            let _ = window.hide();
        }
    }
}
