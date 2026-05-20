use std::path::Path;
use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Shell::*;
use windows::core::*;

#[derive(Debug, Default, Clone)]
pub struct ShortcutInfo {
    pub target: String,
    pub arguments: String,
    pub working_dir: String,
    pub icon_path: String,
    pub icon_index: i32,
}

/// Initialize STA COM for this thread. Idempotent / safe to call repeatedly.
pub fn ensure_com_init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok();
    }
}

/// Resolve a .lnk file to its target + metadata. Returns None on failure.
pub fn resolve_lnk(path: &str) -> Option<ShortcutInfo> {
    let p = Path::new(path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    if ext.as_deref() != Some("lnk") {
        return None;
    }
    ensure_com_init();
    unsafe {
        let shell_link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let pf: IPersistFile = shell_link.cast().ok()?;
        let wpath: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        pf.Load(PCWSTR(wpath.as_ptr()), STGM_READ).ok()?;

        let mut target_buf = [0u16; 1024];
        let mut find: WIN32_FIND_DATAW = std::mem::zeroed();
        shell_link
            .GetPath(&mut target_buf, &mut find, SLGP_RAWPATH.0 as u32)
            .ok()?;

        let mut args_buf = [0u16; 1024];
        let _ = shell_link.GetArguments(&mut args_buf);

        let mut wd_buf = [0u16; 1024];
        let _ = shell_link.GetWorkingDirectory(&mut wd_buf);

        let mut icon_buf = [0u16; 1024];
        let mut icon_index: i32 = 0;
        let _ = shell_link.GetIconLocation(&mut icon_buf, &mut icon_index);

        Some(ShortcutInfo {
            target: wide_to_string(&target_buf),
            arguments: wide_to_string(&args_buf),
            working_dir: wide_to_string(&wd_buf),
            icon_path: wide_to_string(&icon_buf),
            icon_index,
        })
    }
}

/// Resolve a .url internet shortcut by parsing the INI-like file manually.
pub fn resolve_url(path: &str) -> Option<ShortcutInfo> {
    let p = Path::new(path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    if ext.as_deref() != Some("url") {
        return None;
    }
    let data = std::fs::read_to_string(path).ok()?;
    let mut info = ShortcutInfo::default();
    for line in data.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("URL=") {
            info.target = v.to_string();
        } else if let Some(v) = line.strip_prefix("IconFile=") {
            info.icon_path = v.to_string();
        } else if let Some(v) = line.strip_prefix("IconIndex=") {
            info.icon_index = v.parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("WorkingDirectory=") {
            info.working_dir = v.to_string();
        }
    }
    if info.target.is_empty() {
        None
    } else {
        Some(info)
    }
}

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}
