// WinRT Composition bootstrap.
//
// Owns the per-app singletons that every fence window's compositor target
// needs: the DispatcherQueueController (required before constructing a
// Compositor on a non-CoreWindow thread) and the Compositor itself.
//
// Lives for the lifetime of the process — created from app::run before any
// fence window is built, and never torn down.
//
// Single-threaded; only touched from the message pump thread.

use windows::System::DispatcherQueueController;
use windows::UI::Composition::Compositor;
use windows::Win32::System::WinRT::*;
use windows::core::*;

static mut COMP_STATE: Option<CompState> = None;

struct CompState {
    // Kept alive for the process lifetime — the Compositor depends on a live
    // dispatcher queue on the same thread.
    _dq_controller: DispatcherQueueController,
    compositor: Compositor,
}

/// Initialise the WinRT Composition stack on the current (message pump)
/// thread. Must be called once, after `OleInitialize` (which sets up the STA)
/// and before any `compositor()` call.
pub fn init() -> Result<()> {
    unsafe {
        if COMP_STATE.is_some() {
            return Ok(());
        }
        // DQTAT_COM_NONE = inherit the COM apartment from the current thread.
        // OleInitialize already put us in an STA, which is what Composition
        // expects.
        let options = DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        };
        let dq_controller = CreateDispatcherQueueController(options)?;
        let compositor = Compositor::new()?;
        COMP_STATE = Some(CompState {
            _dq_controller: dq_controller,
            compositor,
        });
        Ok(())
    }
}

/// Borrow the shared Compositor. Panics if `init` has not been called.
pub fn compositor() -> &'static Compositor {
    unsafe {
        &COMP_STATE
            .as_ref()
            .expect("composition::init not called")
            .compositor
    }
}
