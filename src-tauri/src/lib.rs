pub mod autostart;
pub mod error;
pub mod game_detector;
pub mod hid;

use crate::autostart::{has_minimized_arg, is_autostart_enabled, set_autostart_enabled};
use crate::error::HidError;
use crate::game_detector::{
    default_game_rules, get_foreground_exe_name, list_running_exe_names, load_game_mode_config,
    normalize_exe_name, save_game_mode_config, GameModeConfig, GameModeStatusDto, GameRule,
    GameWatcherState,
};
use crate::hid::{HidDeviceDto, HidManager, TrayStatusDto};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{
    image::Image,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, PhysicalPosition, State, WebviewWindow, WindowEvent,
};

pub struct AppHidState(pub Mutex<HidManager>);
pub struct LightweightModeState(pub AtomicBool);
pub struct AppGameState(pub Mutex<GameWatcherState>);

const TRAY_WINDOW_WIDTH: i32 = 340;
const TRAY_WINDOW_HEIGHT: i32 = 520;
const TRAY_ICON_ID: &str = "moondrop-tray";

#[tauri::command]
fn hid_list_devices(state: State<'_, AppHidState>) -> Result<Vec<HidDeviceDto>, HidError> {
    let mut mgr = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    mgr.list_devices()
}

#[tauri::command]
fn hid_open_device(
    device_key: String,
    window: WebviewWindow,
    state: State<'_, AppHidState>,
) -> Result<HidDeviceDto, HidError> {
    let mut mgr = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    mgr.open_device(&device_key, window)
}

#[tauri::command]
fn hid_close_device(device_key: String, state: State<'_, AppHidState>) -> Result<(), HidError> {
    let mut mgr = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    mgr.close_device(&device_key)
}

#[tauri::command]
fn hid_send_report(
    device_key: String,
    report_id: u8,
    data: Vec<u8>,
    window: WebviewWindow,
    state: State<'_, AppHidState>,
) -> Result<(), HidError> {
    let mgr = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    mgr.send_report(&device_key, report_id, &data, &window)
}

#[tauri::command]
fn hid_send_feature_report(
    device_key: String,
    report_id: u8,
    data: Vec<u8>,
    state: State<'_, AppHidState>,
) -> Result<(), HidError> {
    let mgr = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    mgr.send_feature_report(&device_key, report_id, &data)
}

#[tauri::command]
fn hid_receive_feature_report(
    device_key: String,
    report_id: u8,
    state: State<'_, AppHidState>,
) -> Result<Vec<u8>, HidError> {
    let mgr = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    mgr.receive_feature_report(&device_key, report_id)
}

#[tauri::command]
fn tray_get_status(
    app: AppHandle,
    state: State<'_, AppHidState>,
) -> Result<TrayStatusDto, HidError> {
    let mut mgr = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    mgr.tray_get_status(&app)
}

#[tauri::command]
fn tray_set_eq_preset(
    preset_index: u8,
    app: AppHandle,
    state: State<'_, AppHidState>,
) -> Result<TrayStatusDto, HidError> {
    let mut mgr = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    mgr.tray_set_eq_preset(&app, preset_index)
}

#[tauri::command]
fn tray_set_global_gain(
    gain_db: f32,
    app: AppHandle,
    state: State<'_, AppHidState>,
) -> Result<TrayStatusDto, HidError> {
    let mut mgr = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    mgr.tray_set_global_gain(&app, gain_db)
}

#[tauri::command]
fn tray_get_lightweight_mode(state: State<'_, LightweightModeState>) -> bool {
    state.0.load(Ordering::Relaxed)
}

#[tauri::command]
fn tray_set_lightweight_mode(
    enabled: bool,
    app: AppHandle,
    state: State<'_, LightweightModeState>,
) -> bool {
    state.0.store(enabled, Ordering::Relaxed);
    if enabled {
        if let Some(main_win) = app.get_webview_window("main") {
            let _ = main_win.hide();
        }
    }
    enabled
}

#[tauri::command]
fn tray_open_main_window(app: AppHandle) {
    if let Some(tray_win) = app.get_webview_window("tray") {
        let _ = tray_win.hide();
    }
    if let Some(main_win) = app.get_webview_window("main") {
        let _ = main_win.show();
        let _ = main_win.unminimize();
        let _ = main_win.set_focus();
    }
}

#[tauri::command]
fn tray_quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn tray_get_autostart() -> bool {
    is_autostart_enabled()
}

#[tauri::command]
fn tray_set_autostart(enabled: bool) -> Result<bool, HidError> {
    set_autostart_enabled(enabled)
}

#[tauri::command]
fn game_mode_get_status(state: State<'_, AppGameState>) -> Result<GameModeStatusDto, HidError> {
    let watcher = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    Ok(watcher.to_status_dto())
}

#[tauri::command]
fn game_mode_set_enabled(
    enabled: bool,
    app: AppHandle,
    state: State<'_, AppGameState>,
) -> Result<GameModeStatusDto, HidError> {
    let mut watcher = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    watcher.config.enabled = enabled;
    if !enabled {
        watcher.detected_exe = None;
        watcher.detected_game_name = None;
        watcher.active_rule_preset = None;
        watcher.last_applied_target = None;
    }
    save_game_mode_config(&app, &watcher.config);
    Ok(watcher.to_status_dto())
}

#[tauri::command]
fn game_mode_set_revert_on_exit(
    revert_on_exit: bool,
    app: AppHandle,
    state: State<'_, AppGameState>,
) -> Result<GameModeStatusDto, HidError> {
    let mut watcher = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    watcher.config.revert_on_exit = revert_on_exit;
    save_game_mode_config(&app, &watcher.config);
    Ok(watcher.to_status_dto())
}

#[tauri::command]
fn game_mode_upsert_rule(
    exe_name: String,
    display_name: String,
    preset_index: u8,
    enabled: bool,
    app: AppHandle,
    state: State<'_, AppGameState>,
) -> Result<GameModeStatusDto, HidError> {
    let norm_exe = normalize_exe_name(&exe_name);
    if norm_exe.is_empty() {
        let watcher = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
        return Ok(watcher.to_status_dto());
    }
    let clean_title = if display_name.trim().is_empty() {
        norm_exe.trim_end_matches(".exe").to_string()
    } else {
        display_name.trim().to_string()
    };

    let mut watcher = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    if let Some(existing) = watcher
        .config
        .rules
        .iter_mut()
        .find(|r| normalize_exe_name(&r.exe_name) == norm_exe)
    {
        existing.display_name = clean_title;
        existing.preset_index = preset_index;
        existing.enabled = enabled;
    } else {
        watcher.config.rules.insert(
            0,
            GameRule {
                exe_name: norm_exe,
                display_name: clean_title,
                preset_index,
                enabled,
            },
        );
    }
    watcher.last_applied_target = None;
    save_game_mode_config(&app, &watcher.config);
    Ok(watcher.to_status_dto())
}

#[tauri::command]
fn game_mode_remove_rule(
    exe_name: String,
    app: AppHandle,
    state: State<'_, AppGameState>,
) -> Result<GameModeStatusDto, HidError> {
    let norm_exe = normalize_exe_name(&exe_name);
    let mut watcher = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    watcher
        .config
        .rules
        .retain(|r| normalize_exe_name(&r.exe_name) != norm_exe);
    watcher.last_applied_target = None;
    save_game_mode_config(&app, &watcher.config);
    Ok(watcher.to_status_dto())
}

#[tauri::command]
fn game_mode_reset_defaults(
    app: AppHandle,
    state: State<'_, AppGameState>,
) -> Result<GameModeStatusDto, HidError> {
    let mut watcher = state.0.lock().map_err(|_| HidError::StateLockPoisoned)?;
    watcher.config.rules = default_game_rules();
    watcher.last_applied_target = None;
    save_game_mode_config(&app, &watcher.config);
    Ok(watcher.to_status_dto())
}

#[tauri::command]
fn game_mode_list_running_exes() -> Vec<String> {
    let mut list: Vec<String> = list_running_exe_names().into_iter().collect();
    list.sort();
    list
}

fn spawn_game_detector_worker(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(2000));

        let is_enabled = app
            .try_state::<AppGameState>()
            .and_then(|s| s.0.lock().ok().map(|w| w.config.enabled))
            .unwrap_or(false);

        if !is_enabled {
            continue;
        }

        let fg_exe = get_foreground_exe_name();
        let running = list_running_exe_names();

        let target_preset_opt = if let Some(game_state) = app.try_state::<AppGameState>() {
            if let Ok(mut watcher) = game_state.0.lock() {
                let prev_detected = watcher.detected_exe.clone();
                let target = watcher.evaluate_processes(fg_exe.as_deref(), &running, None);
                let changed_detection = watcher.detected_exe != prev_detected;
                Some((target, changed_detection))
            } else {
                None
            }
        } else {
            None
        };

        if let Some((target_opt, changed_detection)) = target_preset_opt {
            if let Some(preset_idx) = target_opt {
                if let Some(hid_state) = app.try_state::<AppHidState>() {
                    if let Ok(mut mgr) = hid_state.0.lock() {
                        let _ = mgr.tray_set_eq_preset(&app, preset_idx);
                    }
                }
            }
            if target_opt.is_some() || changed_detection {
                if let Some(tray_win) = app.get_webview_window("tray") {
                    let _ = tray_win.eval(
                        "window.__MOONDROP_TRAY_REFRESH__ && window.__MOONDROP_TRAY_REFRESH__();",
                    );
                }
            }
        }
    });
}

fn saved_tray_icon_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("custom-tray-icon.rgba"))
}

fn apply_rgba_icon(app: &AppHandle, width: u32, height: u32, rgba: Vec<u8>) -> Result<(), HidError> {
    let expected_len = (width as usize).saturating_mul(height as usize).saturating_mul(4);
    if width == 0 || height == 0 || rgba.len() != expected_len {
        return Err(HidError::InvalidIcon(format!(
            "Expected {} bytes for {}x{} RGBA icon, got {}",
            expected_len,
            width,
            height,
            rgba.len()
        )));
    }

    let img = Image::new_owned(rgba, width, height);
    if let Some(tray) = app.tray_by_id(TRAY_ICON_ID) {
        let _ = tray.set_icon(Some(img.clone()));
    }
    if let Some(main_win) = app.get_webview_window("main") {
        let _ = main_win.set_icon(img);
    }
    Ok(())
}

#[tauri::command]
fn tray_set_custom_icon(
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    app: AppHandle,
) -> Result<(), HidError> {
    apply_rgba_icon(&app, width, height, rgba.clone())?;

    if let Some(path) = saved_tray_icon_path(&app) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut blob = Vec::with_capacity(8 + rgba.len());
        blob.extend_from_slice(&width.to_le_bytes());
        blob.extend_from_slice(&height.to_le_bytes());
        blob.extend_from_slice(&rgba);
        let _ = fs::write(path, blob);
    }

    Ok(())
}

#[tauri::command]
fn tray_reset_default_icon(app: AppHandle) -> Result<(), HidError> {
    if let Some(path) = saved_tray_icon_path(&app) {
        let _ = fs::remove_file(path);
    }
    if let Some(default_icon) = app.default_window_icon().cloned() {
        if let Some(tray) = app.tray_by_id(TRAY_ICON_ID) {
            let _ = tray.set_icon(Some(default_icon.clone()));
        }
        if let Some(main_win) = app.get_webview_window("main") {
            let _ = main_win.set_icon(default_icon);
        }
    }
    Ok(())
}

fn restore_saved_tray_icon(app: &AppHandle) {
    let Some(path) = saved_tray_icon_path(app) else {
        return;
    };
    let Ok(bytes) = fs::read(path) else {
        return;
    };
    if bytes.len() < 8 {
        return;
    }
    let width = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let height = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let rgba = bytes[8..].to_vec();
    let _ = apply_rgba_icon(app, width, height, rgba);
}

fn toggle_tray_popover(app: &AppHandle, click_pos: PhysicalPosition<f64>) {
    let Some(tray_win) = app.get_webview_window("tray") else {
        return;
    };

    if tray_win.is_visible().unwrap_or(false) {
        let _ = tray_win.hide();
        return;
    }

    let scale = tray_win.scale_factor().unwrap_or(1.0);
    let win_w = (TRAY_WINDOW_WIDTH as f64 * scale).round() as i32;
    let win_h = (TRAY_WINDOW_HEIGHT as f64 * scale).round() as i32;

    let mut x = (click_pos.x.round() as i32) - (win_w / 2);
    let mut y = (click_pos.y.round() as i32) - win_h - 14;

    if let Ok(Some(monitor)) = tray_win.current_monitor() {
        let mon_pos = monitor.position();
        let mon_size = monitor.size();
        let min_x = mon_pos.x + 8;
        let max_x = mon_pos.x + (mon_size.width as i32) - win_w - 8;
        if x < min_x {
            x = min_x;
        }
        if x > max_x {
            x = max_x;
        }
        // If tray is at the top of the screen, place popover below cursor
        if y < mon_pos.y + 8 {
            y = (click_pos.y.round() as i32) + 14;
        }
    }

    let _ = tray_win.set_position(PhysicalPosition::new(x, y));
    let _ = tray_win.show();
    let _ = tray_win.set_focus();
    let _ = tray_win.eval("window.__MOONDROP_TRAY_REFRESH__ && window.__MOONDROP_TRAY_REFRESH__();");
}

fn setup_system_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let mut tray_builder = TrayIconBuilder::with_id(TRAY_ICON_ID)
        .tooltip("MOONDROP Hub Desktop")
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left | MouseButton::Right,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                toggle_tray_popover(tray.app_handle(), position);
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray_builder = tray_builder.icon(icon);
    }

    let _ = tray_builder.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let hid_manager = match HidManager::new() {
        Ok(mgr) => mgr,
        Err(err) => {
            eprintln!("Failed to initialize USB-HID manager: {err}");
            std::process::exit(1);
        }
    };

    if let Err(err) = tauri::Builder::default()
        .manage(AppHidState(Mutex::new(hid_manager)))
        .manage(LightweightModeState(AtomicBool::new(false)))
        .manage(AppGameState(Mutex::new(GameWatcherState::new(
            GameModeConfig::default(),
        ))))
        .setup(|app| {
            let loaded_cfg = load_game_mode_config(app.handle());
            if let Some(game_state) = app.try_state::<AppGameState>() {
                if let Ok(mut watcher) = game_state.0.lock() {
                    watcher.config = loaded_cfg;
                }
            }

            let _ = setup_system_tray(app);
            restore_saved_tray_icon(app.handle());
            spawn_game_detector_worker(app.handle().clone());

            let start_minimized = has_minimized_arg(std::env::args());
            if start_minimized {
                if let Some(lw_state) = app.try_state::<LightweightModeState>() {
                    lw_state.0.store(true, Ordering::Relaxed);
                }
                if let Some(main_win) = app.get_webview_window("main") {
                    let _ = main_win.hide();
                }
            }

            // Auto-hide tray popover when it loses focus
            if let Some(tray_win) = app.get_webview_window("tray") {
                let tray_clone = tray_win.clone();
                tray_win.on_window_event(move |event| {
                    if let WindowEvent::Focused(false) = event {
                        let _ = tray_clone.hide();
                    }
                });
            }

            // When Lightweight mode is active, closing the main window hides it to tray instead of quitting
            if let Some(main_win) = app.get_webview_window("main") {
                let main_clone = main_win.clone();
                let app_handle = app.handle().clone();
                main_win.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        let lw = app_handle
                            .try_state::<LightweightModeState>()
                            .map(|s| s.0.load(Ordering::Relaxed))
                            .unwrap_or(false);
                        if lw {
                            api.prevent_close();
                            let _ = main_clone.hide();
                        }
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            hid_list_devices,
            hid_open_device,
            hid_close_device,
            hid_send_report,
            hid_send_feature_report,
            hid_receive_feature_report,
            tray_get_status,
            tray_set_eq_preset,
            tray_set_global_gain,
            tray_get_lightweight_mode,
            tray_set_lightweight_mode,
            tray_open_main_window,
            tray_quit_app,
            tray_get_autostart,
            tray_set_autostart,
            tray_set_custom_icon,
            tray_reset_default_icon,
            game_mode_get_status,
            game_mode_set_enabled,
            game_mode_set_revert_on_exit,
            game_mode_upsert_rule,
            game_mode_remove_rule,
            game_mode_reset_defaults,
            game_mode_list_running_exes,
        ])
        .run(tauri::generate_context!())
    {
        eprintln!("Error while running MOONDROP Hub Desktop: {err}");
    }
}
