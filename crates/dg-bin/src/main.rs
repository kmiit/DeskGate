#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let e = dg_win32::app::run();
    #[cfg(debug_assertions)]
    eprintln!("DeskGate exited: {:?}", e);
    let _ = e;
}
