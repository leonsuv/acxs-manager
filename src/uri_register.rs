//! Windows per-user URL protocol registration for `acmanager://` and `acxs://`.
//!
//! Writes under `HKCU\Software\Classes\<scheme>` so no UAC elevation is required.
//! On non-Windows hosts this module compiles cleanly as a stub while keeping
//! command-line formatting helpers testable everywhere.

use std::path::Path;

/// URI schemes registered by ACXS Manager.
pub const SCHEMES: &[&str] = &["acmanager", "acxs"];

/// Formats the Windows registry `shell\open\command` value for `exe_path`.
///
/// Always double-quotes the executable path so paths containing spaces work reliably.
pub fn format_open_command(exe_path: &Path) -> String {
    let raw = exe_path.display().to_string().replace('"', "");
    format!("\"{raw}\" \"%1\"")
}

/// Formats the Windows registry `DefaultIcon` value for `exe_path`.
pub fn format_default_icon(exe_path: &Path) -> String {
    let raw = exe_path.display().to_string().replace('"', "");
    format!("\"{raw}\",0")
}

/// Registers `acmanager://` and `acxs://` in `HKCU\Software\Classes` pointing to `exe_path`.
#[cfg(windows)]
pub fn register_protocols(exe_path: &Path) -> Result<(), String> {
    for scheme in SCHEMES {
        register_one(scheme, exe_path)?;
    }
    Ok(())
}

/// Removes `acmanager://` and `acxs://` from `HKCU\Software\Classes`.
#[cfg(windows)]
pub fn unregister_protocols() -> Result<(), String> {
    for scheme in SCHEMES {
        unregister_one(scheme)?;
    }
    Ok(())
}

/// Returns true when both `acmanager://` and `acxs://` are registered to `exe_path`.
#[cfg(windows)]
pub fn is_registered(exe_path: &Path) -> bool {
    let expected = format_open_command(exe_path);
    SCHEMES
        .iter()
        .all(|scheme| read_open_command(scheme).is_some_and(|cmd| cmd.eq_ignore_ascii_case(&expected)))
}

#[cfg(not(windows))]
pub fn register_protocols(_exe_path: &Path) -> Result<(), String> {
    Err("URL protocol registration is only supported on Windows.".to_string())
}

#[cfg(not(windows))]
pub fn unregister_protocols() -> Result<(), String> {
    Err("URL protocol unregistration is only supported on Windows.".to_string())
}

#[cfg(not(windows))]
pub fn is_registered(_exe_path: &Path) -> bool {
    false
}

#[cfg(windows)]
mod win {
    use super::{format_default_icon, format_open_command};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegOpenKeyExW,
        RegQueryValueExW, RegSetValueExW,
    };

    const KEY_READ: u32 = 0x0002_0019;
    const KEY_WRITE: u32 = 0x0002_0006;
    const REG_SZ: u32 = 1;

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    fn set_sz(hkey: HKEY, name: Option<&str>, value: &str) -> Result<(), String> {
        let name_w = name.map(wide);
        let val_w = wide(value);
        let status = unsafe {
            RegSetValueExW(
                hkey,
                name_w.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
                0,
                REG_SZ,
                val_w.as_ptr().cast::<u8>(),
                (val_w.len() * 2) as u32,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(format!("RegSetValueExW failed with error {status}"))
        }
    }

    fn create_and_set(subkey: &str, values: &[(Option<&str>, String)]) -> Result<(), String> {
        let sub_w = wide(subkey);
        let mut hkey: HKEY = std::ptr::null_mut();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                sub_w.as_ptr(),
                0,
                std::ptr::null_mut(),
                0,
                KEY_WRITE,
                std::ptr::null(),
                &mut hkey,
                std::ptr::null_mut(),
            )
        };
        if status != 0 {
            return Err(format!("RegCreateKeyExW({subkey}) failed with error {status}"));
        }
        let mut res = Ok(());
        for (name, val) in values {
            if let Err(e) = set_sz(hkey, *name, val) {
                res = Err(e);
                break;
            }
        }
        unsafe {
            RegCloseKey(hkey);
        }
        res
    }

    pub(super) fn register_one(scheme: &str, exe_path: &Path) -> Result<(), String> {
        let root = format!(r"Software\Classes\{scheme}");
        create_and_set(
            &root,
            &[
                (None, format!("URL:ACXS Manager ({scheme})")),
                (Some("URL Protocol"), String::new()),
            ],
        )?;
        create_and_set(
            &format!(r"{root}\DefaultIcon"),
            &[(None, format_default_icon(exe_path))],
        )?;
        create_and_set(
            &format!(r"{root}\shell\open\command"),
            &[(None, format_open_command(exe_path))],
        )?;
        Ok(())
    }

    pub(super) fn unregister_one(scheme: &str) -> Result<(), String> {
        let sub_w = wide(&format!(r"Software\Classes\{scheme}"));
        let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, sub_w.as_ptr()) };
        // ERROR_FILE_NOT_FOUND (2) is fine when already unregistered.
        if status == 0 || status == 2 {
            Ok(())
        } else {
            Err(format!("RegDeleteTreeW({scheme}) failed with error {status}"))
        }
    }

    pub(super) fn read_open_command(scheme: &str) -> Option<String> {
        let sub_w = wide(&format!(r"Software\Classes\{scheme}\shell\open\command"));
        let mut hkey: HKEY = std::ptr::null_mut();
        let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, sub_w.as_ptr(), 0, KEY_READ, &mut hkey) };
        if status != 0 {
            return None;
        }
        let mut ty: u32 = 0;
        let mut bytes: u32 = 0;
        let q1 = unsafe {
            RegQueryValueExW(
                hkey,
                std::ptr::null(),
                std::ptr::null_mut(),
                &mut ty,
                std::ptr::null_mut(),
                &mut bytes,
            )
        };
        if q1 != 0 || ty != REG_SZ || bytes < 2 {
            unsafe { RegCloseKey(hkey) };
            return None;
        }
        let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
        let q2 = unsafe {
            RegQueryValueExW(
                hkey,
                std::ptr::null(),
                std::ptr::null_mut(),
                &mut ty,
                buf.as_mut_ptr().cast::<u8>(),
                &mut bytes,
            )
        };
        unsafe { RegCloseKey(hkey) };
        if q2 != 0 {
            return None;
        }
        while buf.last() == Some(&0) {
            buf.pop();
        }
        String::from_utf16(&buf).ok()
    }
}

#[cfg(windows)]
use win::{read_open_command, register_one, unregister_one};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_command_line_properly_quotes_exe_with_spaces() {
        let exe = Path::new(r"C:\Program Files\ACXS Manager\ACXS-Manager.exe");
        assert_eq!(
            format_open_command(exe),
            r#""C:\Program Files\ACXS Manager\ACXS-Manager.exe" "%1""#
        );
        assert_eq!(
            format_default_icon(exe),
            r#""C:\Program Files\ACXS Manager\ACXS-Manager.exe",0"#
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_stub_reports_unsupported_cleanly() {
        assert!(!is_registered(Path::new("/tmp/acxs")));
        assert!(register_protocols(Path::new("/tmp/acxs")).is_err());
        assert!(unregister_protocols().is_err());
    }
}
