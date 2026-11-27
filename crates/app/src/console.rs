//! Windows console handling for the headless launcher modes.
//!
//! The GUI binary uses the Windows GUI subsystem so Explorer does not open a
//! terminal window. The `--check`, `--e2e` and `--help` modes still need a
//! console: when started from a shell with inherited pipes no action is needed,
//! otherwise we attach to the parent console or allocate one.

// SAFETY scope: console attachment runs once, on the main thread, before the
// first standard I/O call of a headless mode.
#![allow(unsafe_code, reason = "attach a console before the first stdio use")]

/// Makes standard output and error visible to a headless CLI invocation.
#[cfg(windows)]
pub fn attach_for_cli() {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };

    unsafe {
        // Inherited pipes (CI, shell redirection, tests) are already valid.
        if GetStdHandle(STD_OUTPUT_HANDLE) == INVALID_HANDLE_VALUE
            && AttachConsole(ATTACH_PARENT_PROCESS) == 0
        {
            let _ = AllocConsole();
        }
    }
}

/// No-op on non-Windows hosts, where the terminal is always available.
#[cfg(not(windows))]
pub fn attach_for_cli() {}
