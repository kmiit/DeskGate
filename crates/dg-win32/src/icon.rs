use std::collections::HashMap;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::shortcut::{ensure_com_init, resolve_lnk, resolve_url};

const ICON_CACHE_CAPACITY: usize = 512;

pub struct IconCache {
    // (path, target_px) -> (bitmap, monotonic insertion order)
    map: HashMap<(String, u32), (ID2D1Bitmap1, u64)>,
    tick: u64,
}

impl Default for IconCache {
    fn default() -> Self {
        Self::new()
    }
}

impl IconCache {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            tick: 0,
        }
    }

    pub fn invalidate(&mut self) {
        self.map.clear();
    }

    pub fn get_or_load(
        &mut self,
        dc: &ID2D1DeviceContext,
        path: &str,
        target_px: u32,
    ) -> Option<ID2D1Bitmap1> {
        let key = (path.to_string(), target_px);
        self.tick = self.tick.wrapping_add(1);
        let tick = self.tick;
        if let Some(entry) = self.map.get_mut(&key) {
            entry.1 = tick;
            return Some(entry.0.clone());
        }
        let bmp = load_icon_bitmap(dc, path, target_px)?;
        if self.map.len() >= ICON_CACHE_CAPACITY {
            // Evict least-recently-used entry.
            if let Some(victim) = self
                .map
                .iter()
                .min_by_key(|(_, (_, t))| *t)
                .map(|(k, _)| k.clone())
            {
                self.map.remove(&victim);
            }
        }
        self.map.insert(key, (bmp.clone(), tick));
        Some(bmp)
    }
}

fn load_icon_bitmap(dc: &ID2D1DeviceContext, path: &str, target_px: u32) -> Option<ID2D1Bitmap1> {
    // For Large/Huge sizes prefer IShellItemImageFactory — it gives sharp
    // 48/64/256 px renderings instead of an upscaled 32 px icon.
    if target_px >= 40
        && let Some(hbm) = image_factory_hbitmap(path, target_px)
    {
        let r = hbitmap_to_d2d_bitmap(dc, hbm, target_px);
        unsafe {
            let _ = DeleteObject(hbm.into());
        }
        if let Ok(b) = r {
            return Some(b);
        }
    }

    let (icon_source, icon_index) = resolve_icon_source(path);
    let hicon = extract_hicon(&icon_source, icon_index, target_px)?;
    let bmp = hicon_to_d2d_bitmap(dc, hicon, target_px).ok();
    unsafe {
        let _ = DestroyIcon(hicon);
    }
    bmp
}

fn image_factory_hbitmap(path: &str, target_px: u32) -> Option<HBITMAP> {
    unsafe {
        ensure_com_init();
        let wpath: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let item: IShellItem =
            SHCreateItemFromParsingName(PCWSTR(wpath.as_ptr()), None::<&IBindCtx>).ok()?;
        let factory: IShellItemImageFactory = item.cast().ok()?;
        let size = windows::Win32::Foundation::SIZE {
            cx: target_px as i32,
            cy: target_px as i32,
        };
        factory
            .GetImage(size, SIIGBF_RESIZETOFIT | SIIGBF_BIGGERSIZEOK)
            .ok()
    }
}

fn hbitmap_to_d2d_bitmap(
    dc: &ID2D1DeviceContext,
    src: HBITMAP,
    size: u32,
) -> windows::core::Result<ID2D1Bitmap1> {
    unsafe {
        // Find the source bitmap's actual dimensions.
        let mut bm: BITMAP = std::mem::zeroed();
        let n = GetObjectW(
            src.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut _),
        );
        if n == 0 {
            return Err(Error::from_thread());
        }
        let src_w = bm.bmWidth.max(1) as u32;
        let src_h = bm.bmHeight.abs().max(1) as u32;

        // Render source HBITMAP into a 32bpp top-down DIB of (size x size),
        // stretching to fit.
        let hdc_screen = GetDC(None);
        let hdc_dst = CreateCompatibleDC(Some(hdc_screen));
        let hdc_src = CreateCompatibleDC(Some(hdc_screen));

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let dest_dib =
            CreateDIBSection(Some(hdc_dst), &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0)?;

        let old_dst = SelectObject(hdc_dst, dest_dib.into());
        let old_src = SelectObject(hdc_src, src.into());

        // Use AlphaBlend (not StretchBlt) so the source's premultiplied alpha
        // channel survives. StretchBlt/SRCCOPY would zero-fill alpha for
        // bitmaps returned by IShellItemImageFactory, leaving us with opaque
        // black behind the icon.
        SetStretchBltMode(hdc_dst, HALFTONE);
        let _ = SetBrushOrgEx(hdc_dst, 0, 0, None);
        let bf = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = AlphaBlend(
            hdc_dst,
            0,
            0,
            size as i32,
            size as i32,
            hdc_src,
            0,
            0,
            src_w as i32,
            src_h as i32,
            bf,
        );

        // Source is already premultiplied. Do NOT run the all-zero-alpha
        // fix-up (that's for old 1-bpp-mask icons), and do NOT premultiply
        // again here.
        let pixel_count = (size * size) as usize;
        let pixel_slice = std::slice::from_raw_parts(bits_ptr as *const u8, pixel_count * 4);

        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        let size_u = D2D_SIZE_U {
            width: size,
            height: size,
        };
        let bmp = dc.CreateBitmap(
            size_u,
            Some(pixel_slice.as_ptr() as *const _),
            size * 4,
            &props,
        )?;

        SelectObject(hdc_src, old_src);
        SelectObject(hdc_dst, old_dst);
        let _ = DeleteObject(dest_dib.into());
        let _ = DeleteDC(hdc_src);
        let _ = DeleteDC(hdc_dst);
        ReleaseDC(None, hdc_screen);

        Ok(bmp)
    }
}

fn resolve_icon_source(path: &str) -> (String, i32) {
    if let Some(info) = resolve_lnk(path).or_else(|| resolve_url(path)) {
        if !info.icon_path.is_empty() {
            return (info.icon_path, info.icon_index);
        }
        if !info.target.is_empty() {
            return (info.target, 0);
        }
    }
    (path.to_string(), 0)
}

fn extract_hicon(path: &str, icon_index: i32, target_px: u32) -> Option<HICON> {
    unsafe {
        let wpath: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".exe") || lower.ends_with(".dll") || lower.ends_with(".ico") {
            let mut large = HICON::default();
            let mut small = HICON::default();
            let want_large = target_px > 24;
            let large_opt: Option<*mut HICON> = if want_large { Some(&mut large) } else { None };
            let small_opt: Option<*mut HICON> = if !want_large { Some(&mut small) } else { None };
            let n = ExtractIconExW(PCWSTR(wpath.as_ptr()), icon_index, large_opt, small_opt, 1);
            if n > 0 {
                let h = if want_large { large } else { small };
                if !h.is_invalid() {
                    return Some(h);
                }
            }
        }

        let mut sfi = SHFILEINFOW::default();
        let flags = if target_px > 24 {
            SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES
        } else {
            SHGFI_ICON | SHGFI_SMALLICON | SHGFI_USEFILEATTRIBUTES
        };
        let r = SHGetFileInfoW(
            PCWSTR(wpath.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut sfi as *mut _),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        );
        if r != 0 && !sfi.hIcon.is_invalid() {
            return Some(sfi.hIcon);
        }
        None
    }
}

fn hicon_to_d2d_bitmap(
    dc: &ID2D1DeviceContext,
    hicon: HICON,
    size: u32,
) -> windows::core::Result<ID2D1Bitmap1> {
    unsafe {
        let hdc_screen = GetDC(None);
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbmp = CreateDIBSection(Some(hdc_mem), &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0)?;

        let old_obj = SelectObject(hdc_mem, hbmp.into());

        let pixel_count = (size * size) as usize;
        std::ptr::write_bytes(bits_ptr as *mut u8, 0, pixel_count * 4);

        let _ = DrawIconEx(
            hdc_mem,
            0,
            0,
            hicon,
            size as i32,
            size as i32,
            0,
            None,
            DI_NORMAL,
        );

        let pixel_slice = std::slice::from_raw_parts_mut(bits_ptr as *mut u8, pixel_count * 4);
        let mut all_zero_alpha = true;
        for i in 0..pixel_count {
            if pixel_slice[i * 4 + 3] != 0 {
                all_zero_alpha = false;
                break;
            }
        }
        if all_zero_alpha {
            for i in 0..pixel_count {
                let off = i * 4;
                if pixel_slice[off] != 0 || pixel_slice[off + 1] != 0 || pixel_slice[off + 2] != 0 {
                    pixel_slice[off + 3] = 255;
                }
            }
        }
        for i in 0..pixel_count {
            let off = i * 4;
            let a = pixel_slice[off + 3] as u32;
            if a < 255 {
                pixel_slice[off] = ((pixel_slice[off] as u32 * a) / 255) as u8;
                pixel_slice[off + 1] = ((pixel_slice[off + 1] as u32 * a) / 255) as u8;
                pixel_slice[off + 2] = ((pixel_slice[off + 2] as u32 * a) / 255) as u8;
            }
        }

        let bitmap_props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        let size_u = D2D_SIZE_U {
            width: size,
            height: size,
        };
        let stride = size * 4;
        let bmp = dc.CreateBitmap(
            size_u,
            Some(pixel_slice.as_ptr() as *const _),
            stride,
            &bitmap_props,
        )?;

        SelectObject(hdc_mem, old_obj);
        let _ = DeleteObject(hbmp.into());
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(None, hdc_screen);

        Ok(bmp)
    }
}
