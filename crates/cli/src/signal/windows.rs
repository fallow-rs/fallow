//! Windows console-control handler.
//!
//! Registers a `PHANDLER_ROUTINE` via `SetConsoleCtrlHandler`. The handler
//! runs on a Windows-kernel-spawned thread; it is NOT subject to POSIX
//! async-signal-safety constraints (locks and allocations are allowed),
//! but it MUST return within ~5 seconds before the OS force-kills the
//! process. The drain budget in `registry::drain_and_kill` is capped to
//! 1500ms accordingly.
//!
//! Event-to-exit-code map:
//! - `CTRL_C_EVENT`         (Ctrl+C from console)       -> 130
//! - `CTRL_BREAK_EVENT`     (Ctrl+Break)                -> 130
//! - `CTRL_CLOSE_EVENT`     (console window closed)     -> 143
//! - `CTRL_LOGOFF_EVENT`    (user logoff)               -> 143
//! - `CTRL_SHUTDOWN_EVENT`  (system shutdown)           -> 143
//!
//! Returning TRUE tells Windows we handled the event; returning FALSE
//! falls back to the next handler in the chain.

use std::io;

// `BOOL` moved from `Win32::Foundation` to `windows_sys::core` in
// windows-sys 0.61 (matches the `SetConsoleCtrlHandler` signature
// `add: windows_sys::core::BOOL -> windows_sys::core::BOOL`). The old
// `Win32::Foundation::BOOL` path no longer resolves and broke the
// Windows ARM64 native compile on every PR.
use windows_sys::Win32::System::Console::{
    CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
    SetConsoleCtrlHandler,
};
use windows_sys::core::BOOL;

use super::handle_signal;

/// SAFETY: invoked by the Windows kernel on a dedicated thread; only the
/// arguments documented by the Win32 ABI are passed. The body delegates
/// to safe Rust immediately.
unsafe extern "system" fn handler(ctrl_type: u32) -> BOOL {
    let exit_code = match ctrl_type {
        CTRL_C_EVENT | CTRL_BREAK_EVENT => 130,
        CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT => 143,
        _ => return 0, // TRUE/FALSE BOOL; 0 = FALSE (not handled).
    };
    handle_signal(exit_code);
    1 // TRUE = handled.
}

/// Install the console control handler.
pub fn install() -> io::Result<()> {
    // SAFETY: `handler` matches the documented `PHANDLER_ROUTINE` ABI;
    // the second argument (`Add`) is TRUE so Windows pushes onto the
    // existing handler chain rather than replacing it.
    let ok: BOOL = unsafe { SetConsoleCtrlHandler(Some(handler), 1) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
