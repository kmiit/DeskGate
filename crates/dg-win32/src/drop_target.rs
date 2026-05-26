// IDropTarget is a COM interface — its method signatures (raw `*mut`
// pointers, no `unsafe`) come straight from the windows-rs binding and
// can't be modified. Suppress the lints clippy raises for each of those
// shapes; the methods only ever run via DragDrop dispatch which always
// supplies valid pointers, and `new` returns the COM-wrapped interface
// rather than `Self` on purpose (callers want an IDropTarget).
#![allow(clippy::not_unsafe_ptr_arg_deref, clippy::new_ret_no_self)]

use std::cell::{Cell, RefCell};
use std::mem::ManuallyDrop;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::*;
use windows::Win32::UI::Shell::*;
use windows::core::*;

use dg_core::fence::FenceItem;
use dg_locales as loc;

/// What sort of drop-description label is currently being shown next to
/// the cursor. Tracked so DragOver can skip redundant SetData calls when
/// the cursor moves but the hovered icon hasn't changed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DropDescKind {
    /// "Open with <X>" — cursor is over icon at this index.
    OpenWith(usize),
    /// "Add to <Fence Title>" — cursor is over fence chrome / empty area.
    AddTo,
    /// Nothing set yet, or just cleared. Forces the next state change
    /// to push fresh data.
    None,
}

#[implement(IDropTarget)]
pub struct FenceDropTarget {
    pub hwnd: HWND,
    /// Shell helper that paints the drag image. Forwarding our
    /// DragEnter / DragOver / DragLeave / Drop into it is what makes the
    /// DROPDESCRIPTION text actually appear (without the helper, just
    /// SetData'ing the description is silently dropped by Explorer).
    /// Optional because CoCreateInstance can fail on locked-down boxes
    /// — we degrade to "no drag-image, no description" rather than
    /// breaking the whole drop target.
    helper: Option<IDropTargetHelper>,
    /// Registered clipboard format id for `CFSTR_DROPDESCRIPTION`. Zero
    /// when `RegisterClipboardFormatW` fails — same fallback story.
    cf_drop_description: u16,
    /// IDataObject for the active drag, cached on DragEnter so DragOver
    /// (which doesn't get one) can keep updating the description.
    data_object: RefCell<Option<IDataObject>>,
    /// Last description we pushed. Cell-comparable; lets DragOver bail
    /// out cheaply when the user is just wiggling within one icon.
    last_kind: Cell<DropDescKind>,
}

impl FenceDropTarget {
    pub fn new(hwnd: HWND) -> IDropTarget {
        let helper: Option<IDropTargetHelper> =
            unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_ALL).ok() };
        let cf = unsafe {
            let name: Vec<u16> = "DropDescription\0".encode_utf16().collect();
            RegisterClipboardFormatW(PCWSTR(name.as_ptr())) as u16
        };
        Self {
            hwnd,
            helper,
            cf_drop_description: cf,
            data_object: RefCell::new(None),
            last_kind: Cell::new(DropDescKind::None),
        }
        .into()
    }
}

/// Convert OLE-supplied screen coordinates to client-area pixels and
/// hit-test against the fence's icons. Returns the matched item index
/// when the cursor is over a specific icon, `None` for title-bar /
/// empty space / outside.
unsafe fn icon_hit_under_screen_pt(hwnd: HWND, screen_x: i32, screen_y: i32) -> Option<usize> {
    unsafe {
        let mut pt = POINT {
            x: screen_x,
            y: screen_y,
        };
        let _ = ScreenToClient(hwnd, &mut pt);
        crate::app::with_state_mut(|s| {
            s.fences
                .iter()
                .find(|f| f.hwnd == hwnd)
                .and_then(|fw| fw.hit_test_icon(pt.x, pt.y))
        })
        .flatten()
    }
}

/// True when the item is something we can ask the shell to launch with
/// additional file arguments: shortcuts (`.lnk`/`.url`) and direct
/// executables / scripts.
fn item_is_launchable(item: &FenceItem) -> bool {
    if item.is_link {
        return true;
    }
    if item.is_folder {
        return false;
    }
    let lower = item.filename.to_ascii_lowercase();
    matches!(
        std::path::Path::new(&lower)
            .extension()
            .and_then(|e| e.to_str()),
        Some("exe" | "bat" | "cmd" | "com" | "ps1")
    )
}

/// Look up the fence item under the screen cursor and return its data
/// (cloned, so the AppState borrow ends before the caller runs anything
/// modal like ShellExecute). `Some` iff the cursor is on an icon AND
/// that icon is launchable.
unsafe fn launchable_item_at(hwnd: HWND, screen_x: i32, screen_y: i32) -> Option<FenceItem> {
    unsafe {
        let idx = icon_hit_under_screen_pt(hwnd, screen_x, screen_y)?;
        crate::app::with_state_mut(|s| {
            let fw = s.fences.iter().find(|f| f.hwnd == hwnd)?;
            let item = fw.fence_data.items.get(idx)?;
            if item_is_launchable(item) {
                Some(item.clone())
            } else {
                None
            }
        })
        .flatten()
    }
}

/// Read the fence's display title (for the "Add to %1" description).
unsafe fn fence_title(hwnd: HWND) -> String {
    unsafe {
        crate::app::with_state_mut(|s| {
            s.fences
                .iter()
                .find(|f| f.hwnd == hwnd)
                .map(|fw| fw.fence_data.title.clone())
        })
        .flatten()
        .unwrap_or_default()
    }
}

/// Read the display name of the fence item at `idx` (for "Open with %1").
unsafe fn item_display_name(hwnd: HWND, idx: usize) -> String {
    unsafe {
        crate::app::with_state_mut(|s| {
            s.fences
                .iter()
                .find(|f| f.hwnd == hwnd)
                .and_then(|fw| fw.fence_data.items.get(idx))
                .map(|it| it.display_name.clone())
        })
        .flatten()
        .unwrap_or_default()
    }
}

/// Write `data` into a GMEM_MOVEABLE block, wrap it in an STGMEDIUM, and
/// hand ownership to `dataobj` via SetData(fRelease=TRUE). On SetData
/// failure we free the block ourselves so it doesn't leak.
unsafe fn set_blob(dataobj: &IDataObject, cf: u16, data: &[u8]) -> Result<()> {
    unsafe {
        let hglobal = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, data.len())?;
        let ptr = GlobalLock(hglobal);
        if ptr.is_null() {
            let _ = GlobalFree(Some(hglobal));
            return Err(Error::from_thread());
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
        let _ = GlobalUnlock(hglobal);

        let format = FORMATETC {
            cfFormat: cf,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        let medium = STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: hglobal },
            pUnkForRelease: ManuallyDrop::new(None),
        };

        match dataobj.SetData(&format, &medium, true) {
            Ok(()) => Ok(()),
            Err(e) => {
                // SetData failed → we still own the block.
                let _ = GlobalFree(Some(hglobal));
                Err(e)
            }
        }
    }
}

/// Build a `DROPDESCRIPTION` from a template ("Open with %1") and an
/// insert ("Notepad"). Shell substitutes `%1` with the insert at paint
/// time, so the template must keep that literal.
fn build_drop_description(image: DROPIMAGETYPE, message: &str, insert: &str) -> DROPDESCRIPTION {
    let mut desc = DROPDESCRIPTION {
        r#type: image,
        szMessage: [0u16; 260],
        szInsert: [0u16; 260],
    };
    let msg_u16: Vec<u16> = message.encode_utf16().take(259).collect();
    let ins_u16: Vec<u16> = insert.encode_utf16().take(259).collect();
    // DROPDESCRIPTION is `repr(C, packed(1))`, so taking a slice of the
    // szMessage / szInsert fields would create an unaligned reference
    // (UB even when never dereferenced). Use raw-pointer writes instead.
    unsafe {
        let msg_ptr = std::ptr::addr_of_mut!(desc.szMessage) as *mut u16;
        for (i, &c) in msg_u16.iter().enumerate() {
            msg_ptr.add(i).write_unaligned(c);
        }
        let ins_ptr = std::ptr::addr_of_mut!(desc.szInsert) as *mut u16;
        for (i, &c) in ins_u16.iter().enumerate() {
            ins_ptr.add(i).write_unaligned(c);
        }
    }
    desc
}

/// Push `desc` onto `dataobj` under the registered DropDescription
/// format. Cheap-failure (logs in debug, no-op otherwise).
unsafe fn push_drop_description(dataobj: &IDataObject, cf: u16, desc: &DROPDESCRIPTION) {
    if cf == 0 {
        return;
    }
    let bytes = unsafe {
        std::slice::from_raw_parts(
            desc as *const _ as *const u8,
            std::mem::size_of::<DROPDESCRIPTION>(),
        )
    };
    if let Err(_e) = unsafe { set_blob(dataobj, cf, bytes) } {
        #[cfg(debug_assertions)]
        eprintln!("[dg] push_drop_description failed: {:?}", _e);
    }
}

impl FenceDropTarget_Impl {
    /// Recompute the drop description for `(hwnd, screen_pt)` and push
    /// it to the cached IDataObject if anything changed. Called from
    /// DragEnter and DragOver.
    fn refresh_description(&self, screen_x: i32, screen_y: i32) {
        let hwnd = self.hwnd;
        let kind = unsafe { icon_hit_under_screen_pt(hwnd, screen_x, screen_y) }
            .and_then(|idx| {
                let item = unsafe {
                    crate::app::with_state_mut(|s| {
                        s.fences
                            .iter()
                            .find(|f| f.hwnd == hwnd)
                            .and_then(|fw| fw.fence_data.items.get(idx).cloned())
                    })
                    .flatten()
                };
                item.and_then(|it| {
                    if item_is_launchable(&it) {
                        Some(DropDescKind::OpenWith(idx))
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(DropDescKind::AddTo);

        if self.last_kind.get() == kind {
            return;
        }
        self.last_kind.set(kind);

        let Some(dataobj) = self.data_object.borrow().clone() else {
            return;
        };

        let (image, message, insert) = match kind {
            DropDescKind::OpenWith(idx) => {
                let name = unsafe { item_display_name(hwnd, idx) };
                (
                    DROPIMAGE_LINK,
                    loc::t(loc::DROP_DESC_OPEN_WITH).to_string(),
                    name,
                )
            }
            DropDescKind::AddTo => {
                let title = unsafe { fence_title(hwnd) };
                (
                    DROPIMAGE_COPY,
                    loc::t(loc::DROP_DESC_ADD_TO).to_string(),
                    title,
                )
            }
            DropDescKind::None => unreachable!(),
        };
        let desc = build_drop_description(image, &message, &insert);
        unsafe { push_drop_description(&dataobj, self.cf_drop_description, &desc) };
    }
}

impl IDropTarget_Impl for FenceDropTarget_Impl {
    fn DragEnter(
        &self,
        pdataobj: windows_core::Ref<IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        unsafe {
            let effect = if launchable_item_at(self.hwnd, pt.x, pt.y).is_some() {
                DROPEFFECT_LINK
            } else {
                DROPEFFECT_COPY
            };
            *pdweffect = effect;

            // Cache the data object so DragOver can keep updating the
            // description without an inbound dataobj of its own.
            if let Some(obj) = pdataobj.as_ref() {
                *self.data_object.borrow_mut() = Some(obj.clone());
                if let Some(helper) = &self.helper {
                    let point = POINT { x: pt.x, y: pt.y };
                    let _ = helper.DragEnter(self.hwnd, obj, &point, effect);
                }
            }
        }
        // Force the next refresh to send fresh data (we just cleared the
        // tracker on DragLeave / are entering fresh).
        self.last_kind.set(DropDescKind::None);
        self.refresh_description(pt.x, pt.y);
        Ok(())
    }

    fn DragOver(
        &self,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        unsafe {
            let effect = if launchable_item_at(self.hwnd, pt.x, pt.y).is_some() {
                DROPEFFECT_LINK
            } else {
                DROPEFFECT_COPY
            };
            *pdweffect = effect;
            if let Some(helper) = &self.helper {
                let point = POINT { x: pt.x, y: pt.y };
                let _ = helper.DragOver(&point, effect);
            }
        }
        self.refresh_description(pt.x, pt.y);
        Ok(())
    }

    fn DragLeave(&self) -> windows_core::Result<()> {
        if let Some(helper) = &self.helper {
            unsafe {
                let _ = helper.DragLeave();
            }
        }
        *self.data_object.borrow_mut() = None;
        self.last_kind.set(DropDescKind::None);
        Ok(())
    }

    fn Drop(
        &self,
        pdataobj: windows_core::Ref<IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        // Default to LINK if landing on a launchable icon, COPY for the
        // add-to-fence path. The actual final effect doesn't matter much
        // — the source uses "file no longer exists at original path" to
        // detect MOVE, not this flag — but reporting the right one keeps
        // the visual feedback honest.
        let target_item = unsafe { launchable_item_at(self.hwnd, pt.x, pt.y) };
        let effect = if target_item.is_some() {
            DROPEFFECT_LINK
        } else {
            DROPEFFECT_COPY
        };
        unsafe {
            *pdweffect = effect;
        }

        // Let the shell helper finish its drag-image animation before
        // we run any modal calls (ShellExecute, IFileOperation).
        if let Some(helper) = &self.helper
            && let Some(obj) = pdataobj.as_ref()
        {
            unsafe {
                let point = POINT { x: pt.x, y: pt.y };
                let _ = helper.Drop(obj, &point, effect);
            }
        }
        *self.data_object.borrow_mut() = None;
        self.last_kind.set(DropDescKind::None);

        let Some(dataobj) = pdataobj.as_ref() else {
            return Ok(());
        };

        let fmt = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };

        let mut medium = match unsafe { dataobj.GetData(&fmt) } {
            Ok(m) => m,
            Err(_) => return Ok(()),
        };

        let mut paths: Vec<String> = Vec::new();
        unsafe {
            let hdrop = HDROP(medium.u.hGlobal.0);
            let count = DragQueryFileW(hdrop, 0xFFFFFFFF, None);
            for i in 0..count {
                let len = DragQueryFileW(hdrop, i, None) as usize;
                if len == 0 {
                    continue;
                }
                let mut buf = vec![0u16; len + 1];
                DragQueryFileW(hdrop, i, Some(&mut buf));
                paths.push(String::from_utf16_lossy(&buf[..len]));
            }
            ReleaseStgMedium(&mut medium);
        }

        // Drop-onto-icon: hand off to ShellExecute with the dropped
        // files as additional command-line arguments. This bypasses the
        // add-to-fence path entirely — the fence's icon set doesn't
        // change, we just invoke the target program.
        if let Some(item) = target_item {
            crate::fence_window::launch_item_with_files(&item, &paths);
            return Ok(());
        }

        let target_hwnd = self.hwnd;
        let _ = unsafe {
            crate::app::with_state_mut(|s| -> windows::core::Result<()> {
                let profile_dir = s.config.config_dir.clone();
                let fence_id = {
                    let Some(fw) = s.fences.iter().find(|f| f.hwnd == target_hwnd) else {
                        return Ok(());
                    };
                    fw.fence_data.id.clone()
                };
                for path in &paths {
                    // Cross-fence classification. A drop whose source
                    // path lives under `<profile>/items/<other_id>/` is
                    // the user dragging an icon out of one fence and
                    // into a different one — we move the file into our
                    // own storage and inherit the source item's
                    // OriginalPath so later remove/delete still restores
                    // to the user's original desktop location. A drop
                    // whose source is in OUR own storage (same fence
                    // drag-out → drag-back) is a no-op.
                    let in_any_storage = crate::storage::is_inside_storage(&profile_dir, path);
                    let same_fence = in_any_storage && {
                        let parent = std::path::Path::new(path).parent();
                        let our_dir = crate::storage::fence_storage(&profile_dir, &fence_id);
                        parent.is_some_and(|p| {
                            crate::storage::paths_equal(
                                &p.to_string_lossy(),
                                &our_dir.to_string_lossy(),
                            )
                        })
                    };
                    if same_fence {
                        continue;
                    }

                    // For cross-fence drops, look up the source item in
                    // the other fence's config so we can carry its
                    // original_path forward. Done BEFORE the move (and
                    // BEFORE taking a &mut on the destination fence) so
                    // we can read `s.config.fences` immutably.
                    let inherited_orig: Option<String> = if in_any_storage {
                        let mut found = None;
                        for fence in &s.config.fences {
                            if fence.id == fence_id {
                                continue;
                            }
                            if let Some(it) = fence
                                .items
                                .iter()
                                .find(|it| crate::storage::paths_equal(&it.filename, path))
                            {
                                found = it.original_path.clone();
                                break;
                            }
                        }
                        found
                    } else {
                        None
                    };

                    // Dedup against the destination fence's existing
                    // items: skip if the same source path / original
                    // path is already represented.
                    let already = {
                        let Some(fw) = s.fences.iter().find(|f| f.hwnd == target_hwnd) else {
                            return Ok(());
                        };
                        fw.fence_data.items.iter().any(|it| {
                            crate::storage::paths_equal(&it.filename, path)
                                || it
                                    .original_path
                                    .as_deref()
                                    .is_some_and(|op| crate::storage::paths_equal(op, path))
                        })
                    };
                    if already {
                        continue;
                    }

                    // Storage decision:
                    //   - cross-fence: move file into our storage,
                    //     carry inherited OriginalPath.
                    //   - desktop:    move file into our storage,
                    //     set OriginalPath = source path.
                    //   - elsewhere:  leave file in place, no OriginalPath.
                    let (filename, original_path) = if in_any_storage {
                        match crate::storage::move_into_storage(&profile_dir, &fence_id, path) {
                            Ok(new_path) => {
                                (new_path.to_string_lossy().into_owned(), inherited_orig)
                            }
                            Err(e) => {
                                eprintln!("[dg] cross-fence move failed: {}", e);
                                continue;
                            }
                        }
                    } else if crate::storage::is_on_desktop(path) {
                        match crate::storage::move_into_storage(&profile_dir, &fence_id, path) {
                            Ok(new_path) => {
                                (new_path.to_string_lossy().into_owned(), Some(path.clone()))
                            }
                            Err(e) => {
                                eprintln!("[dg] move_into_storage failed: {}", e);
                                (path.clone(), None)
                            }
                        }
                    } else {
                        (path.clone(), None)
                    };

                    let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == target_hwnd) else {
                        return Ok(());
                    };
                    let p = std::path::Path::new(&filename);
                    let is_folder = p.is_dir();
                    let lower = filename.to_ascii_lowercase();
                    let is_link = lower.ends_with(".lnk") || lower.ends_with(".url");
                    let display = p
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    fw.fence_data.items.push(FenceItem {
                        filename,
                        display_name: display,
                        is_folder,
                        is_link,
                        display_order: fw.fence_data.items.len() as i32,
                        arguments: None,
                        original_path,
                    });
                }
                if let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == target_hwnd) {
                    fw.d2d.icon_cache.invalidate();
                    fw.render()?;
                    if let Some(cf) = s.config.fences.iter_mut().find(|f| f.id == fence_id) {
                        cf.items = fw.fence_data.items.clone();
                    }
                }
                let _ = s.config.save_fences();
                Ok(())
            })
        };

        Ok(())
    }
}
