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
                if let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == target_hwnd) {
                    for path in &paths {
                        let p = std::path::Path::new(path);
                        let is_folder = p.is_dir();
                        let lower = path.to_ascii_lowercase();
                        let is_link = lower.ends_with(".lnk") || lower.ends_with(".url");
                        let display = p
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string();
                        fw.fence_data.items.push(FenceItem {
                            filename: path.clone(),
                            display_name: display,
                            is_folder,
                            is_link,
                            display_order: fw.fence_data.items.len() as i32,
                            arguments: None,
                        });
                    }
                    fw.d2d.icon_cache.invalidate();
                    fw.render()?;
                    // Mirror into config.fences so save works.
                    if let Some(cf) = s
                        .config
                        .fences
                        .iter_mut()
                        .find(|f| f.id == fw.fence_data.id)
                    {
                        cf.items = fw.fence_data.items.clone();
                    }
                    let _ = s.config.save_fences();
                }
                Ok(())
            })
        };

        Ok(())
    }
}
