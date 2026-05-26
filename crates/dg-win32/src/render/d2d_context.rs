use std::collections::HashMap;
use windows::Graphics::DirectX::*;
use windows::UI::Composition::Desktop::*;
use windows::UI::Composition::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::WinRT::Composition::*;
use windows::core::*;
use windows_numerics::Vector2;

use crate::blur_effect::{GaussianBlurEffect, source_name};
use crate::composition::compositor;
use crate::icon::IconCache;

// Corner radius shared between the blur layer clip and the content backdrop.
// Kept in sync with draw_fence's rounded rect radius.
pub const CORNER_RADIUS: f32 = 8.0;

// Title bar height in logical DIPs. Used both for window non-client hit
// testing and for the rolled-up fence height.
pub const TITLE_H_DIP: f32 = 32.0;

pub struct D2DContext {
    pub d2d_device: ID2D1Device,
    pub d2d_factory: ID2D1Factory1,
    pub dwrite_factory: IDWriteFactory,
    pub icon_cache: IconCache,
    // WinRT Composition tree owned by this window.
    _comp_target: DesktopWindowTarget,
    root_visual: ContainerVisual,
    blur_visual: SpriteVisual,
    content_visual: SpriteVisual,
    // D2D-backed surface drawn into by draw_fence and bound to the content
    // visual through a CompositionSurfaceBrush.
    comp_graphics_device: CompositionGraphicsDevice,
    pub drawing_surface: Option<CompositionDrawingSurface>,
    pub surface_size: (u32, u32),
    text_format_cache: HashMap<(u16, bool), IDWriteTextFormat>,
    blur_enabled: bool,
    blur_radius: f32,
    // Current DPI for the window this context paints into. All public
    // arguments to ensure_surface / draw_fence are logical DIPs (96-DPI
    // pixels); we multiply by `dpi/96` internally to size the physical
    // surface and to scale every dimension we draw.
    pub dpi: u32,
}

/// Convert logical DIPs to physical pixels using the given DPI.
#[inline]
pub fn dip_to_px(dip: f32, dpi: u32) -> f32 {
    dip * dpi as f32 / 96.0
}

/// Convert physical pixels to logical DIPs.
#[inline]
#[allow(dead_code)]
pub fn px_to_dip(px: f32, dpi: u32) -> f32 {
    px * 96.0 / dpi as f32
}

impl D2DContext {
    pub fn create(hwnd: HWND) -> windows::core::Result<Self> {
        unsafe {
            // 1. D3D11 device with BGRA support.
            let mut d3d_device: Option<ID3D11Device> = None;
            let create_flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
            let feature_levels = [
                D3D_FEATURE_LEVEL_11_1,
                D3D_FEATURE_LEVEL_11_0,
                D3D_FEATURE_LEVEL_10_1,
                D3D_FEATURE_LEVEL_10_0,
            ];
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                create_flags,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut d3d_device),
                None,
                None,
            )?;
            let d3d_device = d3d_device.unwrap();

            // 2. DXGI device — required to wire D2D into a Composition graphics device.
            let dxgi_device: IDXGIDevice = d3d_device.cast()?;

            // 3. D2D factory + device + dwrite.
            let d2d_factory: ID2D1Factory1 =
                D2D1CreateFactory::<ID2D1Factory1>(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let d2d_device = d2d_factory.CreateDevice(&dxgi_device)?;
            let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            // 4. WinRT Composition: bind the shared Compositor to this HWND.
            let comp = compositor();
            let desktop_interop: ICompositorDesktopInterop = comp.cast()?;
            let comp_target: DesktopWindowTarget =
                desktop_interop.CreateDesktopWindowTarget(hwnd, true)?;

            // 5. Bridge D2D into Composition through ICompositorInterop so the
            //    drawing surface we hand out for the content layer is backed
            //    by the same D2D device used for everything we draw.
            let comp_interop: ICompositorInterop = comp.cast()?;
            let comp_graphics_device: CompositionGraphicsDevice = {
                let d2d_unk: IUnknown = d2d_device.cast()?;
                comp_interop.CreateGraphicsDevice(&d2d_unk)?
            };

            // 6. Visual tree.
            //    root (container)
            //      ├─ blur_visual    : HostBackdropBrush, rounded-rect clip
            //      └─ content_visual : SurfaceBrush(d2d drawing surface)
            let root_visual = comp.CreateContainerVisual()?;
            let blur_visual = comp.CreateSpriteVisual()?;
            let content_visual = comp.CreateSpriteVisual()?;

            root_visual.Children()?.InsertAtBottom(&blur_visual)?;
            root_visual.Children()?.InsertAtTop(&content_visual)?;
            comp_target.SetRoot(&root_visual)?;

            Ok(Self {
                d2d_device,
                d2d_factory,
                dwrite_factory,
                icon_cache: IconCache::new(),
                _comp_target: comp_target,
                root_visual,
                blur_visual,
                content_visual,
                comp_graphics_device,
                drawing_surface: None,
                surface_size: (0, 0),
                text_format_cache: HashMap::new(),
                blur_enabled: false,
                blur_radius: 20.0,
                dpi: 96,
            })
        }
    }

    /// Toggle the host-backdrop blur layer. The brush is created lazily on
    /// first enable so we don't pay for it on always-transparent fences.
    pub fn set_blur_enabled(&mut self, enable: bool) -> windows::core::Result<()> {
        self.blur_enabled = enable;
        self.rebuild_blur_brush()
    }

    /// Update the gaussian standard deviation in DIPs. Range guidance:
    /// 0 = passthrough (no extra blur on top of the host backdrop), ~20 =
    /// default frosted glass, up to ~150 before the D2D effect clamps.
    pub fn set_blur_radius(&mut self, radius: f32) -> windows::core::Result<()> {
        let r = radius.clamp(0.0, 150.0);
        if (r - self.blur_radius).abs() < 0.01 {
            return Ok(());
        }
        self.blur_radius = r;
        if self.blur_enabled {
            self.rebuild_blur_brush()?;
        }
        Ok(())
    }

    /// (Re)build the CompositionEffectBrush for the blur layer. With blur
    /// disabled, detaches the brush so the layer paints nothing.
    fn rebuild_blur_brush(&mut self) -> windows::core::Result<()> {
        if !self.blur_enabled {
            let null_brush: Option<CompositionBrush> = None;
            self.blur_visual.SetBrush(null_brush.as_ref())?;
            return Ok(());
        }
        let comp = compositor();
        // Custom gaussian-on-top-of-host-backdrop effect chain:
        //   GaussianBlur(σ = blur_radius) ← HostBackdropBrush (already pre-blurred wallpaper)
        // Stacking lets us push σ higher than the DWM default for a softer
        // look without losing the wallpaper sampling that only HostBackdrop
        // can provide on Win32 desktop apps.
        let effect_desc = match GaussianBlurEffect::new(self.blur_radius) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[dg] blur: GaussianBlurEffect::new failed: {:?}", e);
                return Err(e);
            }
        };
        let factory = match comp.CreateEffectFactory(&effect_desc) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[dg] blur: CreateEffectFactory failed: {:?}", e);
                // Fall back to the plain HostBackdropBrush — at least the
                // user still sees the system blur if our custom chain breaks.
                let host = comp.CreateHostBackdropBrush()?;
                self.blur_visual.SetBrush(&host)?;
                return Err(e);
            }
        };
        let brush = factory.CreateBrush()?;
        let host_brush = comp.CreateHostBackdropBrush()?;
        brush.SetSourceParameter(&source_name(), &host_brush)?;
        self.blur_visual.SetBrush(&brush)?;
        Ok(())
    }

    pub(super) fn get_text_format(
        &mut self,
        size: f32,
        bold: bool,
    ) -> windows::core::Result<IDWriteTextFormat> {
        let key = ((size * 2.0) as u16, bold);
        if let Some(f) = self.text_format_cache.get(&key) {
            return Ok(f.clone());
        }
        let weight = if bold {
            DWRITE_FONT_WEIGHT_BOLD
        } else {
            DWRITE_FONT_WEIGHT_NORMAL
        };
        let f = unsafe {
            self.dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!(""),
            )?
        };
        self.text_format_cache.insert(key, f.clone());
        Ok(f)
    }

    /// Measure the rendered width of a string in DIPs at the given size /
    /// weight, as it would be drawn by `dc.DrawText`. Used by fence_window
    /// to size the title's hot region for the dbl-click → roll gesture.
    pub fn measure_text_width(
        &mut self,
        text: &str,
        size: f32,
        bold: bool,
    ) -> windows::core::Result<f32> {
        let fmt = self.get_text_format(size, bold)?;
        // The cached format is shared with `draw_fence`, which calls
        // SetTextAlignment(TRAILING/CENTER) for non-Left titles. Combined
        // with our enormous layoutWidth (≈ f32::MAX/4), trailing/center
        // alignment makes DirectWrite report widthIncludingTrailingWhitespace
        // as 0 — the layout positions the text near +∞ and the metric
        // collapses. Reset to leading before measuring so width is stable
        // regardless of whatever the last draw pass left on the format.
        unsafe {
            fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
        }
        let utf16: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let layout =
                self.dwrite_factory
                    .CreateTextLayout(&utf16, &fmt, f32::MAX / 4.0, size * 4.0)?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics)?;
            Ok(metrics.widthIncludingTrailingWhitespace)
        }
    }

    /// Measure the rendered height of a string in DIPs at the given size /
    /// weight when laid out within `max_width`. Used by the TODO-list
    /// renderer and click hit-tester to agree on how much vertical room
    /// each wrapped row consumes.
    pub fn measure_text_height(
        &mut self,
        text: &str,
        size: f32,
        bold: bool,
        max_width: f32,
    ) -> windows::core::Result<f32> {
        let fmt = self.get_text_format(size, bold)?;
        unsafe {
            fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
        }
        let utf16: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let layout = self.dwrite_factory.CreateTextLayout(
                &utf16,
                &fmt,
                max_width.max(1.0),
                f32::MAX / 4.0,
            )?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics)?;
            Ok(metrics.height.max(size * 1.3))
        }
    }

    /// Update the DPI for this fence's window. Caller should re-render after
    /// changing DPI; the next ensure_surface call will reallocate the drawing
    /// surface at the new physical size and update the root visual scale.
    pub fn set_dpi(&mut self, dpi: u32) {
        let dpi = dpi.clamp(72, 480);
        if dpi != self.dpi {
            self.dpi = dpi;
            // Force surface re-creation on next render.
            self.surface_size = (0, 0);
            self.drawing_surface = None;
        }
    }

    pub(super) fn ensure_surface(
        &mut self,
        logical_w: u32,
        logical_h: u32,
    ) -> windows::core::Result<()> {
        self.ensure_surface_with_radius(logical_w, logical_h, CORNER_RADIUS)
    }

    /// Same as `ensure_surface` but with a caller-controlled corner radius
    /// for the blur layer's clip. Used by the modal dialog which wants a
    /// slightly more pronounced curve than fences do.
    pub fn ensure_modal_surface(
        &mut self,
        logical_w: u32,
        logical_h: u32,
        radius: f32,
    ) -> windows::core::Result<()> {
        self.ensure_surface_with_radius(logical_w, logical_h, radius)
    }

    fn ensure_surface_with_radius(
        &mut self,
        logical_w: u32,
        logical_h: u32,
        radius: f32,
    ) -> windows::core::Result<()> {
        // DesktopWindowTarget visuals live in *physical pixels* by default —
        // Composition doesn't auto-scale to the HWND's DPI. We author the
        // tree in DIPs and bake the scale onto the root visual so child
        // visuals can keep using logical coordinates.
        let scale = self.dpi as f32 / 96.0;
        let v3 = windows_numerics::Vector3 {
            X: scale,
            Y: scale,
            Z: 1.0,
        };
        self.root_visual.SetScale(v3)?;

        // Visuals sized in logical DIPs; root Scale converts to physical px.
        let logical_size = Vector2 {
            X: logical_w as f32,
            Y: logical_h as f32,
        };
        self.root_visual.SetSize(logical_size)?;
        self.blur_visual.SetSize(logical_size)?;
        self.content_visual.SetSize(logical_size)?;

        // Rounded clip on the blur layer so the backdrop respects the
        // window's corner radius. Authored in DIPs to match the visual.
        let geom = compositor().CreateRoundedRectangleGeometry()?;
        geom.SetSize(logical_size)?;
        geom.SetCornerRadius(Vector2 {
            X: radius,
            Y: radius,
        })?;
        let clip = compositor().CreateGeometricClipWithGeometry(&geom)?;
        self.blur_visual.SetClip(&clip)?;

        // Drawing surface holds physical pixels so text and icons stay
        // crisp at high DPI. We pair this with `dc.SetDpi` at BeginDraw
        // time so D2D draw calls keep speaking DIPs.
        let phys_w = dip_to_px(logical_w as f32, self.dpi).round() as u32;
        let phys_h = dip_to_px(logical_h as f32, self.dpi).round() as u32;
        if self.surface_size == (phys_w, phys_h) && self.drawing_surface.is_some() {
            return Ok(());
        }
        let size = windows::Foundation::Size {
            Width: phys_w as f32,
            Height: phys_h as f32,
        };
        let surface = self.comp_graphics_device.CreateDrawingSurface(
            size,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            DirectXAlphaMode::Premultiplied,
        )?;
        let brush = compositor().CreateSurfaceBrushWithSurface(&surface)?;
        // Stretch the physical-pixel surface to cover the DIP-sized visual.
        brush.SetStretch(CompositionStretch::Fill)?;
        self.content_visual.SetBrush(&brush)?;
        self.drawing_surface = Some(surface);
        self.surface_size = (phys_w, phys_h);
        Ok(())
    }
}
