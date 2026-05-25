use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Registry::*;
use windows::core::*;

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const APP_VALUE_NAME: PCWSTR = w!("DeskGate");

/// Path to the currently-running executable, wrapped in double quotes
/// and null-terminated as UTF-16. Quoting matters: `CreateProcess` (which
/// the shell uses to launch entries in the Run key) splits unquoted
/// paths on spaces, so `C:\Program Files\DeskGate.exe` would otherwise
/// be misinterpreted.
fn quoted_exe_path() -> Option<Vec<u16>> {
    let mut buf = [0u16; 2048];
    unsafe {
        let len = GetModuleFileNameW(None, &mut buf) as usize;
        if len == 0 || len >= buf.len() {
            return None;
        }
        let mut out = Vec::with_capacity(len + 3);
        out.push(b'"' as u16);
        out.extend_from_slice(&buf[..len]);
        out.push(b'"' as u16);
        out.push(0);
        Some(out)
    }
}

/// True if the `DeskGate` value exists under HKCU\...\Run. We don't
/// validate that the stored path still points at *this* exe — the user
/// may have moved the binary, and re-asserting the path on every check
/// would be surprising. Toggling off + on resets the path.
pub fn is_enabled() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        let status = RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_READ, &mut hkey);
        if status != ERROR_SUCCESS {
            return false;
        }
        let query = RegQueryValueExW(hkey, APP_VALUE_NAME, None, None, None, None);
        let _ = RegCloseKey(hkey);
        query == ERROR_SUCCESS
    }
}

/// Add (`enabled = true`) or remove (`false`) the Run entry. Returns
/// `Ok(())` on success; errors surface from registry/exe-path failures
/// and should be logged but not panicked on — autostart is non-critical.
pub fn set_enabled(enabled: bool) -> Result<()> {
    unsafe {
        let mut hkey = HKEY::default();
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        );
        if status != ERROR_SUCCESS {
            return Err(Error::from_hresult(HRESULT::from_win32(status.0)));
        }

        let result = if enabled {
            let Some(path) = quoted_exe_path() else {
                let _ = RegCloseKey(hkey);
                return Err(Error::from_hresult(E_FAIL));
            };
            let bytes = std::slice::from_raw_parts(
                path.as_ptr() as *const u8,
                path.len() * std::mem::size_of::<u16>(),
            );
            RegSetValueExW(hkey, APP_VALUE_NAME, None, REG_SZ, Some(bytes))
        } else {
            // ERROR_FILE_NOT_FOUND is fine — the toggle is idempotent.
            RegDeleteValueW(hkey, APP_VALUE_NAME)
        };
        let _ = RegCloseKey(hkey);

        if result == ERROR_SUCCESS || (!enabled && result == ERROR_FILE_NOT_FOUND) {
            Ok(())
        } else {
            Err(Error::from_hresult(HRESULT::from_win32(result.0)))
        }
    }
}
