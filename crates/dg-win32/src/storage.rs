use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use dg_core::fence::Fence;

use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Shell::*;
use windows::core::*;

/// Resolve the two desktop folders Explorer paints icons from — the
/// per-user one under `%USERPROFILE%\Desktop` and the shared one under
/// `C:\Users\Public\Desktop`. Either may fail to resolve on locked-down
/// systems; we just skip the missing one rather than failing the whole
/// "is this on the desktop?" check.
fn desktop_paths() -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(2);
    for id in &[FOLDERID_Desktop, FOLDERID_PublicDesktop] {
        if let Some(p) = known_folder(id) {
            out.push(p);
        }
    }
    out
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let pwstr = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        if pwstr.is_null() {
            return None;
        }
        let mut len = 0usize;
        while *pwstr.0.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(pwstr.0, len);
        let s = String::from_utf16_lossy(slice);
        // SHGetKnownFolderPath returns a CoTaskMem-allocated buffer that
        // we own and must free.
        CoTaskMemFree(Some(pwstr.0 as _));
        Some(PathBuf::from(s))
    }
}

/// Canonical, lower-cased, prefix-stripped path key. Used to compare
/// paths in a Windows-friendly way: case-insensitive and tolerant of
/// `\\?\` extended-length prefixes that `canonicalize()` adds.
fn path_key(p: &Path) -> String {
    let s = p
        .canonicalize()
        .ok()
        .and_then(|c| c.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| p.to_string_lossy().into_owned());
    let trimmed = s.strip_prefix(r"\\?\").unwrap_or(&s);
    trimmed.to_ascii_lowercase()
}

/// True when `path` lives directly inside one of the desktop folders.
/// Nested subfolders don't count — those aren't drawn as desktop icons.
pub fn is_on_desktop(path: &str) -> bool {
    let p = Path::new(path);
    let Some(parent) = p.parent() else {
        return false;
    };
    let parent_key = path_key(parent);
    desktop_paths().iter().any(|d| path_key(d) == parent_key)
}

/// Root of DeskGate's managed-file storage: `<profile_dir>/items/`.
/// Each fence gets a subdirectory keyed by fence id, created lazily.
pub fn items_root(profile_dir: &Path) -> PathBuf {
    profile_dir.join("items")
}

/// Storage directory for one specific fence; ensured to exist.
pub fn fence_storage(profile_dir: &Path, fence_id: &str) -> PathBuf {
    let dir = items_root(profile_dir).join(fence_id);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// True when `path` lives anywhere under `<profile_dir>/items/`. Used by
/// the drop target to silently ignore drops that came from inside our
/// own storage (e.g. dragging a fence icon onto another fence in the
/// same app — see plan §3 / §5 for why this is out-of-scope for now).
pub fn is_inside_storage(profile_dir: &Path, path: &str) -> bool {
    let root = path_key(&items_root(profile_dir));
    let key = path_key(Path::new(path));
    key.starts_with(&format!("{}\\", root))
}

/// Compare two paths for equality after canonicalization. Used for
/// dedup. Falls back to a case-insensitive string compare when neither
/// path exists yet (e.g. paths that have already been moved).
pub fn paths_equal(a: &str, b: &str) -> bool {
    path_key(Path::new(a)) == path_key(Path::new(b))
}

pub fn is_in_place_desktop_item(filename: &str, original_path: Option<&str>) -> bool {
    original_path.is_some_and(|original| paths_equal(filename, original))
}

pub fn set_desktop_managed_hidden(path: &str, hidden: bool) -> io::Result<()> {
    let path_w = wide_path(Path::new(path));
    let attrs = unsafe { GetFileAttributesW(PCWSTR(path_w.as_ptr())) };
    if attrs == INVALID_FILE_ATTRIBUTES {
        return Err(io::Error::last_os_error());
    }

    let mut new_attrs = attrs;
    if hidden {
        new_attrs |= FILE_ATTRIBUTE_HIDDEN.0;
        new_attrs |= FILE_ATTRIBUTE_SYSTEM.0;
    } else {
        new_attrs &= !FILE_ATTRIBUTE_HIDDEN.0;
        new_attrs &= !FILE_ATTRIBUTE_SYSTEM.0;
    }
    if new_attrs == attrs {
        return Ok(());
    }

    unsafe {
        SetFileAttributesW(
            PCWSTR(path_w.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(new_attrs),
        )
        .map_err(|e| io::Error::other(format!("SetFileAttributesW: {e:?}")))?;
        notify_shell_path_changed(Path::new(path), &path_w, hidden);
    }
    Ok(())
}

unsafe fn notify_shell_path_changed(path: &Path, path_w: &[u16], hidden: bool) {
    let is_dir = path.is_dir();
    let visibility_event = match (hidden, is_dir) {
        (true, true) => SHCNE_RMDIR,
        (true, false) => SHCNE_DELETE,
        (false, true) => SHCNE_MKDIR,
        (false, false) => SHCNE_CREATE,
    };

    unsafe {
        // The desktop view can keep showing an item whose attributes just
        // changed, especially when Explorer is configured to show hidden
        // files. Tell the shell that the visible desktop entry itself has
        // gone away (or returned) while leaving the real filesystem object
        // untouched.
        SHChangeNotify(
            visibility_event,
            SHCNF_PATHW | SHCNF_FLUSH,
            Some(path_w.as_ptr() as *const _),
            None,
        );
        SHChangeNotify(
            SHCNE_ATTRIBUTES,
            SHCNF_PATHW | SHCNF_FLUSH,
            Some(path_w.as_ptr() as *const _),
            None,
        );
        SHChangeNotify(
            SHCNE_UPDATEITEM,
            SHCNF_PATHW | SHCNF_FLUSH,
            Some(path_w.as_ptr() as *const _),
            None,
        );
        if let Some(parent) = path.parent() {
            let parent_w = wide_path(parent);
            SHChangeNotify(
                SHCNE_UPDATEDIR,
                SHCNF_PATHW | SHCNF_FLUSH,
                Some(parent_w.as_ptr() as *const _),
                None,
            );
        }
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST | SHCNF_FLUSH, None, None);
    }
}

pub fn hide_desktop_item(path: &str) {
    if let Err(e) = set_desktop_managed_hidden(path, true) {
        eprintln!("[dg] hide desktop item failed: {}", e);
    }
}

pub fn unhide_desktop_item(path: &str) {
    if let Err(e) = set_desktop_managed_hidden(path, false) {
        eprintln!("[dg] unhide desktop item failed: {}", e);
    }
}

/// Pick a destination path inside `dest_dir` that doesn't collide with
/// anything already there. Tries the source's name first, then
/// `name (1)`, `name (2)`, … up to a sane cap. Preserves the extension.
fn pick_non_colliding(dest_dir: &Path, src_name: &OsStr) -> PathBuf {
    let candidate = dest_dir.join(src_name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = Path::new(src_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = Path::new(src_name)
        .extension()
        .map(|s| s.to_string_lossy().into_owned());
    for n in 1..1000 {
        let name = match &ext {
            Some(e) => format!("{} ({}).{}", stem, n, e),
            None => format!("{} ({})", stem, n),
        };
        let p = dest_dir.join(&name);
        if !p.exists() {
            return p;
        }
    }
    // Fallback: timestamp-style name. Extremely unlikely to hit.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    dest_dir.join(format!("{}-{}", stem, ts))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Encode an OS path for win32 calls.
fn wide_path(p: &Path) -> Vec<u16> {
    p.as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

/// Run an `IFileOperation::MoveItem` from `src` to `dest_dir`, optionally
/// renaming to `new_name`. Silent — no UI, no confirmation prompts, no
/// recycle-bin entry. Returns Ok on success and bubbles the HRESULT as
/// an `io::Error` on failure so callers can fall back.
fn shell_move(src: &Path, dest_dir: &Path, new_name: Option<&str>) -> io::Result<()> {
    // STA was set up by app::run via OleInitialize; calling CoCreate from
    // any other thread would assert here. Belt-and-braces: the operation
    // crate uses CoCreateInstance which requires an initialized apartment.
    let result: Result<()> = unsafe {
        let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        op.SetOperationFlags(
            FOF_NO_UI | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT | FOFX_EARLYFAILURE,
        )?;
        let src_w = wide_path(src);
        let dst_w = wide_path(dest_dir);
        let src_item: IShellItem =
            SHCreateItemFromParsingName(PCWSTR(src_w.as_ptr()), None::<&IBindCtx>)?;
        let dst_item: IShellItem =
            SHCreateItemFromParsingName(PCWSTR(dst_w.as_ptr()), None::<&IBindCtx>)?;
        let rename_w = new_name.map(wide);
        let rename_ptr = rename_w
            .as_ref()
            .map(|w| PCWSTR(w.as_ptr()))
            .unwrap_or(PCWSTR::null());
        op.MoveItem(&src_item, &dst_item, rename_ptr, None)?;
        op.PerformOperations()?;
        Ok(())
    };
    result.map_err(|e| io::Error::other(format!("IFileOperation::MoveItem: {e:?}")))
}

/// Move a file or folder into the fence's storage directory. Returns
/// the absolute path of the moved item. Falls back to `std::fs::rename`
/// if the shell operation fails AND source/dest are on the same volume.
pub fn move_into_storage(profile_dir: &Path, fence_id: &str, src: &str) -> io::Result<PathBuf> {
    let src_path = Path::new(src);
    let dest_dir = fence_storage(profile_dir, fence_id);
    let src_name = src_path
        .file_name()
        .ok_or_else(|| io::Error::other("source has no filename"))?;
    let target = pick_non_colliding(&dest_dir, src_name);
    let rename = target
        .file_name()
        .and_then(|s| s.to_str())
        .map(String::from);
    // First try the shell (handles folders + cross-volume); fall back to
    // fs::rename only if the shell path didn't work, since rename can't
    // handle cross-volume folders.
    if let Err(_e) = shell_move(src_path, &dest_dir, rename.as_deref()) {
        #[cfg(debug_assertions)]
        eprintln!("[dg] shell move failed, trying fs::rename: {}", _e);
        std::fs::rename(src_path, &target)?;
    }
    Ok(target)
}

/// Move a previously-stored file back to its original parent. Uses the
/// stored file's current location (`stored`) as the source; resolves
/// the original parent from `original` and picks a non-colliding name
/// there in case the user has created another file with the same name
/// in the meantime.
pub fn move_back_to_original(stored: &str, original: &str) -> io::Result<()> {
    let stored_path = Path::new(stored);
    let original_path = Path::new(original);
    if paths_equal(stored, original) {
        return Ok(());
    }
    if !stored_path.exists() {
        // Nothing to move (e.g. user deleted the file from inside
        // Explorer). Treat as success — there's no work to do.
        return Ok(());
    }
    let parent = original_path
        .parent()
        .ok_or_else(|| io::Error::other("original path has no parent"))?;
    let _ = std::fs::create_dir_all(parent);
    let desired_name = original_path
        .file_name()
        .ok_or_else(|| io::Error::other("original path has no filename"))?;
    let target = pick_non_colliding(parent, desired_name);
    let rename = target
        .file_name()
        .and_then(|s| s.to_str())
        .map(String::from);
    if let Err(_e) = shell_move(stored_path, parent, rename.as_deref()) {
        #[cfg(debug_assertions)]
        eprintln!("[dg] shell move-back failed, trying fs::rename: {}", _e);
        std::fs::rename(stored_path, &target)?;
    }
    Ok(())
}

/// Best-effort: remove `<profile_dir>/items/<fence_id>/` when it's empty.
/// Used after deleting a fence, once all its items have been restored.
pub fn try_remove_fence_storage(profile_dir: &Path, fence_id: &str) {
    let dir = items_root(profile_dir).join(fence_id);
    let _ = std::fs::remove_dir(&dir);
}

/// Move folders from older configs back to their desktop path, then hide
/// that real desktop item while the fence represents it.
pub fn migrate_desktop_folders(fences: &mut [Fence]) -> bool {
    let mut changed = false;
    for fence in fences {
        for item in &mut fence.items {
            let Some(original) = item.original_path.clone() else {
                continue;
            };
            if !is_on_desktop(&original) {
                continue;
            }
            if is_in_place_desktop_item(&item.filename, Some(&original)) {
                if Path::new(&item.filename).is_dir() {
                    hide_desktop_item(&item.filename);
                    item.is_folder = true;
                }
                continue;
            }
            let current = Path::new(&item.filename);
            if !current.is_dir() {
                continue;
            }

            match move_back_to_original(&item.filename, &original) {
                Ok(()) => {
                    hide_desktop_item(&original);
                    item.filename = original.clone();
                    item.original_path = Some(original);
                    item.is_folder = true;
                    changed = true;
                }
                Err(e) => {
                    eprintln!("[dg] restore desktop folder failed: {}", e);
                }
            }
        }
    }
    changed
}
