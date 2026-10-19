//! Small Win32 helpers.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{MESSAGEBOX_STYLE, MessageBoxW};

/// UTF-16, NUL-terminated.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Converts UTF-16 up to the first NUL.
pub fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

pub fn message_box(owner: HWND, text: &str, style: MESSAGEBOX_STYLE) -> i32 {
    let text = wide(text);
    let title = wide("Precision Zoom");
    unsafe { MessageBoxW(owner, text.as_ptr(), title.as_ptr(), style) }
}

pub const fn loword(v: usize) -> u16 {
    (v & 0xFFFF) as u16
}
