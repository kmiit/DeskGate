// HostBackdropBrush in WinRT Composition needs the window to opt in via
// SetWindowCompositionAttribute(WCA_ACCENT_POLICY, ACCENT_ENABLE_HOSTBACKDROP).
// Without that flag DWM never feeds the blurred backdrop into our visual
// tree and the layer paints nothing — even though the brush is attached.
//
// This is the only thing this module does now; the actual blur visual is
// owned by D2DContext.

use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::*;
use windows::core::*;

#[repr(C)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    gradient_color: u32,
    animation_id: u32,
}

#[repr(C)]
struct WindowCompositionAttribData {
    attribute: u32,
    data: *mut std::ffi::c_void,
    data_size: usize,
}

const WCA_ACCENT_POLICY: u32 = 0x13;
const ACCENT_DISABLED: u32 = 0;
const ACCENT_ENABLE_BLURBEHIND: u32 = 3;
const ACCENT_ENABLE_HOSTBACKDROP: u32 = 5;

type SwcaFn = unsafe extern "system" fn(HWND, *mut WindowCompositionAttribData) -> BOOL;

/// Apply an arbitrary accent state. Internal helper.
unsafe fn apply_accent(hwnd: HWND, state: u32) {
    let user32: Vec<u16> = "user32.dll\0".encode_utf16().collect();
    let Ok(h) = LoadLibraryW(PCWSTR(user32.as_ptr())) else {
        return;
    };
    let Some(proc) = GetProcAddress(h, s!("SetWindowCompositionAttribute")) else {
        return;
    };
    let f: SwcaFn = std::mem::transmute(proc);

    let mut policy = AccentPolicy {
        accent_state: state,
        accent_flags: 0,
        gradient_color: 0,
        animation_id: 0,
    };
    let mut data = WindowCompositionAttribData {
        attribute: WCA_ACCENT_POLICY,
        data: &mut policy as *mut _ as *mut _,
        data_size: std::mem::size_of::<AccentPolicy>(),
    };
    let _ = f(hwnd, &mut data);
}

/// Toggle host-backdrop opt-in on the given window. Combined with a
/// HostBackdropBrush bound to a SpriteVisual, this is what makes the blur
/// actually sample the wallpaper underneath.
pub fn enable_host_backdrop(hwnd: HWND, enable: bool) {
    unsafe {
        apply_accent(
            hwnd,
            if enable {
                ACCENT_ENABLE_HOSTBACKDROP
            } else {
                ACCENT_DISABLED
            },
        );
    }
}

/// Enable DWM's plain BlurBehind on the window. Used by the modal dialog
/// which has a real redirection bitmap (so it can host child controls)
/// and can't use the Composition HostBackdropBrush path.
pub fn enable_dwm_blur_behind(hwnd: HWND, enable: bool) {
    unsafe {
        apply_accent(
            hwnd,
            if enable {
                ACCENT_ENABLE_BLURBEHIND
            } else {
                ACCENT_DISABLED
            },
        );
    }
}

/// Back-compat shim — fence_window/customize still call this. Routes to the
/// Composition-side toggle on the matching FenceWindow and flips the OS-side
/// opt-in so the brush actually shows pixels.
pub fn set_blur(hwnd: HWND, enable: bool) {
    enable_host_backdrop(hwnd, enable);
    unsafe {
        crate::app::with_state_mut(|s| {
            if let Some(fw) = s.fences.iter_mut().find(|f| f.hwnd == hwnd) {
                let _ = fw.d2d.set_blur_enabled(enable);
            }
        });
    }
}
