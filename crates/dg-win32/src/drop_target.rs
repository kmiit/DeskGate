// IDropTarget is a COM interface — its method signatures (raw `*mut`
// pointers, no `unsafe`) come straight from the windows-rs binding and
// can't be modified. Suppress the lints clippy raises for each of those
// shapes; the methods only ever run via DragDrop dispatch which always
// supplies valid pointers, and `new` returns the COM-wrapped interface
// rather than `Self` on purpose (callers want an IDropTarget).
#![allow(clippy::not_unsafe_ptr_arg_deref, clippy::new_ret_no_self)]

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::*;
use windows::Win32::UI::Shell::*;
use windows::core::*;

use dg_core::fence::FenceItem;

#[implement(IDropTarget)]
pub struct FenceDropTarget {
    pub hwnd: HWND,
}

impl FenceDropTarget {
    pub fn new(hwnd: HWND) -> IDropTarget {
        Self { hwnd }.into()
    }
}

impl IDropTarget_Impl for FenceDropTarget_Impl {
    fn DragEnter(
        &self,
        _pdataobj: windows_core::Ref<IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        unsafe {
            *pdweffect = DROPEFFECT_COPY;
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        unsafe {
            *pdweffect = DROPEFFECT_COPY;
        }
        Ok(())
    }

    fn DragLeave(&self) -> windows_core::Result<()> {
        Ok(())
    }

    fn Drop(
        &self,
        pdataobj: windows_core::Ref<IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        unsafe {
            *pdweffect = DROPEFFECT_COPY;
        }

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
