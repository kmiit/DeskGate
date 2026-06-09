// IGraphicsEffectD2D1Interop is a COM interface — its `*mut` out-param
// signatures come from the windows-rs binding and can't be changed.
// `GaussianBlurEffect::new` returns IGraphicsEffect (the COM-wrapped
// interface) rather than `Self` since that's what callers actually want
// to hand to the compositor. Suppress the lints clippy raises for those
// two shapes.
#![allow(clippy::not_unsafe_ptr_arg_deref, clippy::new_ret_no_self)]

// Custom GaussianBlur graphics effect description.
//
// WinRT Composition's `Compositor::CreateEffectFactory` takes any object
// implementing `IGraphicsEffect`. The Composition runtime queries that object
// for `IGraphicsEffectD2D1Interop` to learn which D2D1 effect to instantiate
// (here: CLSID_D2D1GaussianBlur), what its properties are, and what its
// inputs are. We answer those queries with a single `StandardDeviation` float
// property and one `CompositionEffectSourceParameter("backdrop")` input,
// which the caller binds to a `HostBackdropBrush` via SetSourceParameter.

use std::cell::Cell;
use windows::Foundation::*;
use windows::Graphics::Effects::*;
use windows::UI::Composition::*;
use windows::Win32::System::WinRT::Graphics::Direct2D::*;
use windows::core::*;

// CLSID for the built-in D2D1 Gaussian Blur effect.
const CLSID_D2D1_GAUSSIAN_BLUR: GUID = GUID::from_u128(0x1FEB6D69_2FE6_4AC9_8C58_1D7F93E7A6A5);

/// Name of the single property exposed by this effect, queryable by host
/// frameworks via GetNamedPropertyMapping. Direct mapping: the value we hand
/// back from GetProperty is plugged straight into the D2D effect property at
/// the returned index.
pub const PROP_BLUR_AMOUNT: PCWSTR = w!("BlurAmount");

/// Name of the single source parameter. Callers bind a real brush to this
/// name through `CompositionEffectBrush::SetSourceParameter`.
pub fn source_name() -> HSTRING {
    HSTRING::from("backdrop")
}

#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
pub struct GaussianBlurEffect {
    radius: Cell<f32>,
    name: Cell<HSTRING>,
    source: CompositionEffectSourceParameter,
}

impl GaussianBlurEffect {
    /// Build the effect description. The returned `IGraphicsEffect` can be
    /// handed to `Compositor::CreateEffectFactory`.
    pub fn new(radius: f32) -> Result<IGraphicsEffect> {
        let source = CompositionEffectSourceParameter::Create(&source_name())?;
        let me = Self {
            radius: Cell::new(radius),
            name: Cell::new(HSTRING::new()),
            source,
        };
        Ok(me.into())
    }
}

impl IGraphicsEffect_Impl for GaussianBlurEffect_Impl {
    fn Name(&self) -> Result<HSTRING> {
        // Cell<HSTRING> is non-Clone, so swap in a temporary and put back.
        let h = self.name.take();
        let out = h.clone();
        self.name.set(h);
        Ok(out)
    }

    fn SetName(&self, name: &HSTRING) -> Result<()> {
        self.name.set(name.clone());
        Ok(())
    }
}

impl IGraphicsEffectSource_Impl for GaussianBlurEffect_Impl {}

impl IGraphicsEffectD2D1Interop_Impl for GaussianBlurEffect_Impl {
    fn GetEffectId(&self) -> Result<GUID> {
        Ok(CLSID_D2D1_GAUSSIAN_BLUR)
    }

    fn GetNamedPropertyMapping(
        &self,
        name: &PCWSTR,
        index: *mut u32,
        mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> Result<()> {
        let n = unsafe { name.to_string().unwrap_or_default() };
        if n.eq_ignore_ascii_case("BlurAmount") {
            unsafe {
                *index = 0;
                *mapping = GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT;
            }
            Ok(())
        } else {
            Err(Error::from_hresult(HRESULT(0x80070057u32 as i32)))
        }
    }

    fn GetPropertyCount(&self) -> Result<u32> {
        // The underlying D2D1 Gaussian Blur effect has 3 properties:
        //   0 = StandardDeviation (float)
        //   1 = Optimization      (enum: speed/balanced/quality)
        //   2 = BorderMode        (enum: soft/hard)
        // Composition probes all of them by index, so we report them all.
        Ok(3)
    }

    fn GetProperty(&self, index: u32) -> Result<IPropertyValue> {
        let value = match index {
            // StandardDeviation in DIPs.
            0 => PropertyValue::CreateSingle(self.radius.get())?,
            // Optimization = D2D1_GAUSSIANBLUR_OPTIMIZATION_SPEED (0).
            1 => PropertyValue::CreateUInt32(0)?,
            // BorderMode = D2D1_BORDER_MODE_HARD (1) — keeps edges crisp so
            // the wallpaper doesn't bleed outside the visual.
            2 => PropertyValue::CreateUInt32(1)?,
            _ => return Err(Error::from_hresult(HRESULT(0x80070057u32 as i32))),
        };
        value.cast()
    }

    fn GetSource(&self, index: u32) -> Result<IGraphicsEffectSource> {
        if index != 0 {
            return Err(Error::from_hresult(HRESULT(0x80070057u32 as i32)));
        }
        self.source.cast()
    }

    fn GetSourceCount(&self) -> Result<u32> {
        Ok(1)
    }
}
