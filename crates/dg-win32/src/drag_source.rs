// IDropSource is a COM interface — its method signatures (raw `*mut`
// pointers, no `unsafe`) come straight from the windows-rs binding and
// can't be modified. Suppress the lints clippy raises for each of those
// shapes; the methods only ever run via DoDragDrop dispatch and `new`
// returns the COM-wrapped interface rather than `Self` on purpose.
#![allow(clippy::new_ret_no_self)]

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::*;
use windows::Win32::UI::Shell::*;
use windows::core::*;

#[implement(IDropSource)]
pub struct FenceDragSource;

impl FenceDragSource {
    pub fn new() -> IDropSource {
        Self.into()
    }
}

impl IDropSource_Impl for FenceDragSource_Impl {
    /// Called by the OS on every mouse move / button transition. Returning
    /// `DRAGDROP_S_DROP` commits the drop; `DRAGDROP_S_CANCEL` aborts;
    /// `S_OK` keeps the drag going. Standard contract:
    ///
    ///   - Esc pressed -> cancel.
    ///   - Left button released -> drop. (The drag started with the left
    ///     button down; releasing it is the natural commit signal.)
    ///   - Otherwise continue.
    ///
    /// We deliberately do not let right-button drags through — the source
    /// only ever calls DoDragDrop after a confirmed left-button drag.
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        if (grfkeystate.0 & MK_LBUTTON.0) == 0 {
            return DRAGDROP_S_DROP;
        }
        S_OK
    }

    /// Let the system render the standard drag cursors (the no-entry
    /// circle, the copy/move overlays). We have nothing custom to paint.
    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Outcome of a drag-out that was committed (DROPEFFECT_NONE means the
/// user cancelled or dropped on something that refused the operation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragOutResult {
    /// Target moved (or appeared to move) the file. The fence item now
    /// points at a path that's been emptied out; the caller should
    /// remove the item from the fence.
    Moved,
    /// Target copied the file; original still in storage. Caller keeps
    /// the fence item as-is.
    Copied,
    /// User cancelled the drag or dropped on a non-target.
    Cancelled,
}

/// Kick off an OLE drag-drop with `path` as the single CF_HDROP payload.
/// Modal — Win32 spins its own message loop and this call returns only
/// after the drag is committed or cancelled. Must run on the STA
/// message-pump thread.
pub fn start_drag_out(path: &str) -> DragOutResult {
    unsafe {
        let wpath: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let item: IShellItem =
            match SHCreateItemFromParsingName(PCWSTR(wpath.as_ptr()), None::<&IBindCtx>) {
                Ok(i) => i,
                Err(_) => return DragOutResult::Cancelled,
            };
        let data_obj: IDataObject = match item.BindToHandler(None, &BHID_DataObject) {
            Ok(d) => d,
            Err(_) => return DragOutResult::Cancelled,
        };
        let source: IDropSource = FenceDragSource::new();
        let mut effect = DROPEFFECT::default();
        let hr = DoDragDrop(
            &data_obj,
            &source,
            DROPEFFECT_COPY | DROPEFFECT_MOVE,
            &mut effect,
        );
        // DRAGDROP_S_DROP = the target accepted; DRAGDROP_S_CANCEL = the
        // user pressed Esc or the source said no. Any other error → treat
        // as cancel so we don't lose the item from the fence.
        if hr != DRAGDROP_S_DROP {
            return DragOutResult::Cancelled;
        }
        // The returned `effect` is unreliable on Vista+ when the target
        // performs an "optimized move": Explorer can MOVE the file but
        // still report DROPEFFECT_COPY here, with the real outcome sent
        // back via SetData(CFSTR_PERFORMEDDROPEFFECT). Rather than parse
        // that out, just look at the source file directly — if it's
        // gone, the move happened, end of story.
        if !std::path::Path::new(path).exists() {
            return DragOutResult::Moved;
        }
        if (effect.0 & DROPEFFECT_COPY.0) != 0 {
            DragOutResult::Copied
        } else {
            DragOutResult::Cancelled
        }
    }
}
