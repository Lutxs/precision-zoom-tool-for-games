//! Precision Zoom: a magnifier overlay that follows the cursor while a key is held.
#![windows_subsystem = "windows"]

mod app;
mod config;
mod geometry;
mod icon_art;
mod input;
mod keys;
mod overlay;
mod preview;
mod ticker;
mod ui;
mod ui_theme;
mod win;

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowW, GetWindowThreadProcessId, MB_ICONERROR, PostMessageW,
};

use win::wide;

fn main() {
    let open_settings = std::env::args().skip(1).any(|a| a == "--settings");
    unsafe {
        // Also declared in the manifest; this covers running without it.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let _mutex = CreateMutexW(
            std::ptr::null(),
            0,
            wide(r"Local\PrecisionZoomTool.SingleInstance").as_ptr(),
        );
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // Already running: open its settings instead of starting a second copy.
            let other = FindWindowW(wide(app::CONTROLLER_CLASS).as_ptr(), std::ptr::null());
            if !other.is_null() {
                let mut pid = 0;
                GetWindowThreadProcessId(other, &mut pid);
                AllowSetForegroundWindow(pid);
                PostMessageW(other, app::WM_APP_SHOW_SETTINGS, 0, 0);
            }
            return;
        }
    }
    if let Err(e) = app::run(open_settings) {
        win::message_box(std::ptr::null_mut(), &e, MB_ICONERROR);
        std::process::exit(1);
    }
}
