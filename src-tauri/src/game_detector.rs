use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Mapping rule from a Windows executable name (e.g. `cs2.exe`) to a Moondrop hardware EQ preset index.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameRule {
    pub exe_name: String,
    pub display_name: String,
    pub preset_index: u8,
    pub enabled: bool,
}

/// Persistent configuration for Auto-Detect Game Mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameModeConfig {
    pub enabled: bool,
    pub revert_on_exit: bool,
    pub default_preset_index: u8,
    pub rules: Vec<GameRule>,
}

/// Live state returned to the Tray UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameModeStatusDto {
    pub enabled: bool,
    pub revert_on_exit: bool,
    pub default_preset_index: u8,
    pub detected_exe: Option<String>,
    pub detected_game_name: Option<String>,
    pub active_rule_preset: Option<u8>,
    pub rules: Vec<GameRule>,
}

pub struct GameWatcherState {
    pub config: GameModeConfig,
    pub detected_exe: Option<String>,
    pub detected_game_name: Option<String>,
    pub active_rule_preset: Option<u8>,
    pub last_applied_target: Option<u8>,
    pub baseline_preset_before_game: u8,
}

impl Default for GameModeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            revert_on_exit: true,
            default_preset_index: 0,
            rules: default_game_rules(),
        }
    }
}

pub fn default_game_rules() -> Vec<GameRule> {
    vec![
        // FPS 1 (Preset Index 3 - Footsteps / Tactical Shooter)
        GameRule {
            exe_name: "cs2.exe".to_string(),
            display_name: "Counter-Strike 2".to_string(),
            preset_index: 3,
            enabled: true,
        },
        GameRule {
            exe_name: "valorant-win64-shipping.exe".to_string(),
            display_name: "VALORANT".to_string(),
            preset_index: 3,
            enabled: true,
        },
        GameRule {
            exe_name: "rainbowsix.exe".to_string(),
            display_name: "Rainbow Six Siege".to_string(),
            preset_index: 3,
            enabled: true,
        },
        GameRule {
            exe_name: "tslgame.exe".to_string(),
            display_name: "PUBG: Battlegrounds".to_string(),
            preset_index: 3,
            enabled: true,
        },
        GameRule {
            exe_name: "escapefromtarkov.exe".to_string(),
            display_name: "Escape from Tarkov".to_string(),
            preset_index: 3,
            enabled: true,
        },
        GameRule {
            exe_name: "deltaforceclient-win64-shipping.exe".to_string(),
            display_name: "Delta Force".to_string(),
            preset_index: 3,
            enabled: true,
        },
        // FPS 2 (Preset Index 4 - Fast Arena / Hero Shooter / BR)
        GameRule {
            exe_name: "r5apex.exe".to_string(),
            display_name: "Apex Legends".to_string(),
            preset_index: 4,
            enabled: true,
        },
        GameRule {
            exe_name: "r5apex_dx12.exe".to_string(),
            display_name: "Apex Legends (DX12)".to_string(),
            preset_index: 4,
            enabled: true,
        },
        GameRule {
            exe_name: "overwatch.exe".to_string(),
            display_name: "Overwatch 2".to_string(),
            preset_index: 4,
            enabled: true,
        },
        GameRule {
            exe_name: "cod.exe".to_string(),
            display_name: "Call of Duty".to_string(),
            preset_index: 4,
            enabled: true,
        },
        GameRule {
            exe_name: "thefinals.exe".to_string(),
            display_name: "THE FINALS".to_string(),
            preset_index: 4,
            enabled: true,
        },
        GameRule {
            exe_name: "marvel-win64-shipping.exe".to_string(),
            display_name: "Marvel Rivals".to_string(),
            preset_index: 4,
            enabled: true,
        },
        GameRule {
            exe_name: "fortniteclient-win64-shipping.exe".to_string(),
            display_name: "Fortnite".to_string(),
            preset_index: 4,
            enabled: true,
        },
        // Music & Rhythm Game (Preset Index 1)
        GameRule {
            exe_name: "osu!.exe".to_string(),
            display_name: "osu!".to_string(),
            preset_index: 1,
            enabled: true,
        },
        GameRule {
            exe_name: "musynx.exe".to_string(),
            display_name: "MUSYNX".to_string(),
            preset_index: 1,
            enabled: true,
        },
        GameRule {
            exe_name: "musedash.exe".to_string(),
            display_name: "Muse Dash".to_string(),
            preset_index: 1,
            enabled: true,
        },
        GameRule {
            exe_name: "djmax respect v.exe".to_string(),
            display_name: "DJMAX RESPECT V".to_string(),
            preset_index: 1,
            enabled: true,
        },
        // Triple-A / Open World / Cinema (Preset Index 2)
        GameRule {
            exe_name: "cyberpunk2077.exe".to_string(),
            display_name: "Cyberpunk 2077".to_string(),
            preset_index: 2,
            enabled: true,
        },
        GameRule {
            exe_name: "eldenring.exe".to_string(),
            display_name: "Elden Ring".to_string(),
            preset_index: 2,
            enabled: true,
        },
        GameRule {
            exe_name: "b1-win64-shipping.exe".to_string(),
            display_name: "Black Myth: Wukong".to_string(),
            preset_index: 2,
            enabled: true,
        },
        GameRule {
            exe_name: "genshinimpact.exe".to_string(),
            display_name: "Genshin Impact".to_string(),
            preset_index: 2,
            enabled: true,
        },
        GameRule {
            exe_name: "yuanshen.exe".to_string(),
            display_name: "Genshin Impact (CN)".to_string(),
            preset_index: 2,
            enabled: true,
        },
        GameRule {
            exe_name: "starrail.exe".to_string(),
            display_name: "Honkai: Star Rail".to_string(),
            preset_index: 2,
            enabled: true,
        },
        GameRule {
            exe_name: "zenlesszonezero.exe".to_string(),
            display_name: "Zenless Zone Zero".to_string(),
            preset_index: 2,
            enabled: true,
        },
        GameRule {
            exe_name: "client-win64-shipping.exe".to_string(),
            display_name: "Wuthering Waves".to_string(),
            preset_index: 2,
            enabled: true,
        },
        GameRule {
            exe_name: "monsterhunterwilds.exe".to_string(),
            display_name: "Monster Hunter Wilds".to_string(),
            preset_index: 2,
            enabled: true,
        },
    ]
}

pub fn normalize_exe_name(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('"');
    let file_part = trimmed
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(trimmed)
        .trim()
        .to_ascii_lowercase();
    if file_part.is_empty() {
        return String::new();
    }
    if file_part.ends_with(".exe") {
        file_part
    } else {
        format!("{file_part}.exe")
    }
}

impl GameWatcherState {
    pub fn new(config: GameModeConfig) -> Self {
        let default_idx = config.default_preset_index;
        Self {
            config,
            detected_exe: None,
            detected_game_name: None,
            active_rule_preset: None,
            last_applied_target: None,
            baseline_preset_before_game: default_idx,
        }
    }

    pub fn to_status_dto(&self) -> GameModeStatusDto {
        GameModeStatusDto {
            enabled: self.config.enabled,
            revert_on_exit: self.config.revert_on_exit,
            default_preset_index: self.config.default_preset_index,
            detected_exe: self.detected_exe.clone(),
            detected_game_name: self.detected_game_name.clone(),
            active_rule_preset: self.active_rule_preset,
            rules: self.config.rules.clone(),
        }
    }

    /// Evaluates the currently foregrounded window exe and running process list against configured rules.
    /// Returns `Some(target_preset_index)` when a hardware EQ switch should be triggered.
    pub fn evaluate_processes(
        &mut self,
        foreground_exe: Option<&str>,
        running_exes: &HashSet<String>,
        current_device_preset: Option<u8>,
    ) -> Option<u8> {
        if !self.config.enabled {
            self.detected_exe = None;
            self.detected_game_name = None;
            self.active_rule_preset = None;
            self.last_applied_target = None;
            return None;
        }

        // Priority 1: Check if the active foreground window matches an enabled game rule
        let mut matched_rule: Option<&GameRule> = None;
        if let Some(fg) = foreground_exe {
            let fg_norm = normalize_exe_name(fg);
            matched_rule = self
                .config
                .rules
                .iter()
                .find(|r| r.enabled && normalize_exe_name(&r.exe_name) == fg_norm);
        }

        // Priority 2: Check if any running process matches an enabled game rule
        if matched_rule.is_none() {
            matched_rule = self
                .config
                .rules
                .iter()
                .find(|r| r.enabled && running_exes.contains(&normalize_exe_name(&r.exe_name)));
        }

        if let Some(rule) = matched_rule {
            let rule_exe = normalize_exe_name(&rule.exe_name);
            let rule_name = rule.display_name.clone();
            let target_preset = rule.preset_index;

            // If transitioning from no-game -> game, remember the user's current preset
            if self.active_rule_preset.is_none() {
                if let Some(cur) = current_device_preset {
                    self.baseline_preset_before_game = cur;
                } else {
                    self.baseline_preset_before_game = self.config.default_preset_index;
                }
            }

            self.detected_exe = Some(rule_exe);
            self.detected_game_name = Some(rule_name);
            self.active_rule_preset = Some(target_preset);

            if self.last_applied_target != Some(target_preset) {
                self.last_applied_target = Some(target_preset);
                return Some(target_preset);
            }
            None
        } else {
            let was_in_game = self.active_rule_preset.is_some();
            self.detected_exe = None;
            self.detected_game_name = None;
            self.active_rule_preset = None;

            if was_in_game && self.config.revert_on_exit {
                let revert_target = self.config.default_preset_index;
                self.last_applied_target = None;
                return Some(revert_target);
            }
            self.last_applied_target = None;
            None
        }
    }
}

fn config_file_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("game-mode-config.json"))
}

pub fn load_game_mode_config(app: &AppHandle) -> GameModeConfig {
    let Some(path) = config_file_path(app) else {
        return GameModeConfig::default();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return GameModeConfig::default();
    };
    serde_json::from_str::<GameModeConfig>(&raw).unwrap_or_default()
}

pub fn save_game_mode_config(app: &AppHandle, config: &GameModeConfig) {
    let Some(path) = config_file_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(config) {
        let _ = fs::write(path, json);
    }
}

/// Returns the lowercase `.exe` name of the current foreground window on Windows.
#[cfg(windows)]
pub fn get_foreground_exe_name() -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(handle);
        if ok == 0 || size == 0 {
            return None;
        }
        let full_path = String::from_utf16_lossy(&buf[..size as usize]);
        let norm = normalize_exe_name(&full_path);
        if norm.is_empty() {
            None
        } else {
            Some(norm)
        }
    }
}

#[cfg(not(windows))]
pub fn get_foreground_exe_name() -> Option<String> {
    None
}

/// Enumerates all running process `.exe` names on Windows in ~0.5ms using ToolHelp32 snapshot.
#[cfg(windows)]
pub fn list_running_exe_names() -> HashSet<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut set = HashSet::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
            return set;
        }

        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                if len > 0 {
                    let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                    let norm = normalize_exe_name(&name);
                    if !norm.is_empty() {
                        set.insert(norm);
                    }
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }

        CloseHandle(snapshot);
    }
    set
}

#[cfg(not(windows))]
pub fn list_running_exe_names() -> HashSet<String> {
    HashSet::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_exe_name_handles_paths_and_missing_extension() {
        assert_eq!(
            normalize_exe_name(r#"C:\Program Files\Steam\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe"#),
            "cs2.exe"
        );
        assert_eq!(normalize_exe_name("VALORANT-Win64-Shipping"), "valorant-win64-shipping.exe");
        assert_eq!(normalize_exe_name("  r5apex.EXE "), "r5apex.exe");
    }

    #[test]
    fn evaluate_processes_switches_on_game_launch_and_reverts_on_exit() {
        let mut watcher = GameWatcherState::new(GameModeConfig::default());
        let mut running = HashSet::new();

        // No game running
        assert_eq!(watcher.evaluate_processes(None, &running, Some(0)), None);
        assert_eq!(watcher.detected_exe, None);

        // Launch CS2 -> should switch to FPS 1 (preset 3)
        running.insert("cs2.exe".to_string());
        assert_eq!(
            watcher.evaluate_processes(Some("cs2.exe"), &running, Some(0)),
            Some(3)
        );
        assert_eq!(watcher.detected_game_name.as_deref(), Some("Counter-Strike 2"));
        assert_eq!(watcher.active_rule_preset, Some(3));

        // Subsequent poll while CS2 is still running -> no duplicate write
        assert_eq!(
            watcher.evaluate_processes(Some("cs2.exe"), &running, Some(3)),
            None
        );

        // Exit CS2 -> should revert to default preset (0)
        running.clear();
        assert_eq!(watcher.evaluate_processes(None, &running, Some(3)), Some(0));
        assert_eq!(watcher.detected_exe, None);
        assert_eq!(watcher.active_rule_preset, None);
    }
}
