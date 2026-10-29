use crate::error::HidError;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APP_RUN_VALUE_NAME: &str = "MOONDROPHubDesktop";

pub fn format_autostart_command(exe_path: &str) -> String {
    format!("\"{}\" --minimized", exe_path.trim_matches('"'))
}

pub fn has_minimized_arg<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|arg| {
        let a = arg.as_ref().trim();
        a.eq_ignore_ascii_case("--minimized") || a.eq_ignore_ascii_case("-m")
    })
}

#[cfg(target_os = "windows")]
pub fn is_autostart_enabled() -> bool {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_SZ,
    };

    let subkey: Vec<u16> = RUN_KEY_PATH.encode_utf16().chain(std::iter::once(0)).collect();
    let val_name: Vec<u16> = APP_RUN_VALUE_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_READ,
            &mut hkey,
        );
        if status != ERROR_SUCCESS {
            return false;
        }

        let mut value_type: u32 = 0;
        let mut data_len: u32 = 0;
        let query_status = RegQueryValueExW(
            hkey,
            val_name.as_ptr(),
            std::ptr::null_mut(),
            &mut value_type,
            std::ptr::null_mut(),
            &mut data_len,
        );
        let _ = RegCloseKey(hkey);

        query_status == ERROR_SUCCESS && value_type == REG_SZ && data_len > 2
    }
}

#[cfg(not(target_os = "windows"))]
pub fn is_autostart_enabled() -> bool {
    false
}

#[cfg(target_os = "windows")]
pub fn set_autostart_enabled(enabled: bool) -> Result<bool, HidError> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ,
    };

    let subkey: Vec<u16> = RUN_KEY_PATH.encode_utf16().chain(std::iter::once(0)).collect();
    let val_name: Vec<u16> = APP_RUN_VALUE_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        );
        if status != ERROR_SUCCESS {
            return Ok(false);
        }

        if enabled {
            let exe = std::env::current_exe()
                .ok()
                .and_then(|p| p.to_str().map(|s| s.to_string()))
                .unwrap_or_default();
            if !exe.is_empty() {
                let cmd = format_autostart_command(&exe);
                let cmd_wide: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
                let byte_len = (cmd_wide.len() * 2) as u32;
                let _ = RegSetValueExW(
                    hkey,
                    val_name.as_ptr(),
                    0,
                    REG_SZ,
                    cmd_wide.as_ptr() as *const u8,
                    byte_len,
                );
            }
        } else {
            let _ = RegDeleteValueW(hkey, val_name.as_ptr());
        }

        let _ = RegCloseKey(hkey);
    }

    Ok(is_autostart_enabled())
}

#[cfg(not(target_os = "windows"))]
pub fn set_autostart_enabled(_enabled: bool) -> Result<bool, HidError> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_autostart_command_quotes_exe_and_appends_minimized_flag() {
        let cmd = format_autostart_command(r"C:\Program Files\MOONDROP Hub\moondrop-hub.exe");
        assert_eq!(
            cmd,
            r#""C:\Program Files\MOONDROP Hub\moondrop-hub.exe" --minimized"#
        );
    }

    #[test]
    fn has_minimized_arg_detects_minimized_flags() {
        assert!(has_minimized_arg(["moondrop-hub.exe", "--minimized"]));
        assert!(has_minimized_arg(["moondrop-hub.exe", "-m"]));
        assert!(!has_minimized_arg(["moondrop-hub.exe"]));
    }
}
