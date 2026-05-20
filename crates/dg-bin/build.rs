// Embed a Windows application manifest into the resulting EXE so the
// process gets:
//   * Common Controls v6     → modern look on EDIT / BUTTON / MessageBox.
//   * PerMonitorV2 DPI       → already set at runtime, but the manifest
//                              entry is what the OS reads *before* WinMain,
//                              avoiding the brief 1x scaling flash on launch.
//   * UTF-8 active code page → so APIs that take/return char* understand
//                              our UTF-8 strings (defence in depth — we
//                              mostly use the W variants already).
//
// embed-manifest does this in pure Rust, no rc.exe needed.

use embed_manifest::manifest::{ActiveCodePage, DpiAwareness};
use embed_manifest::{embed_manifest, new_manifest};

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let manifest = new_manifest("DeskGate.App")
            .dpi_awareness(DpiAwareness::PerMonitorV2)
            .active_code_page(ActiveCodePage::Utf8);
        embed_manifest(manifest).expect("embed manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
