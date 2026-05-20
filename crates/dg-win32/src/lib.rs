#![allow(static_mut_refs)]
// Edition 2024 promoted `unsafe_op_in_unsafe_fn` from allow-by-default
// to warn-by-default. This crate is full of `unsafe extern "system" fn`
// wndprocs and Win32-call-heavy `unsafe fn` helpers whose bodies are
// already understood to be unsafe top to bottom — wrapping each call
// in `unsafe { ... }` adds noise without improving safety. Suppress
// here so the rest of the file stays readable.
#![allow(unsafe_op_in_unsafe_fn)]

pub mod app;
pub mod blur;
pub mod blur_effect;
pub mod composition;
pub mod customize;
pub mod drop_target;
pub mod fence_window;
pub mod icon;
pub mod layout;
pub mod modal;
pub mod render;
pub mod shortcut;
pub mod tray;
