//! Small Win32 helpers.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowTextLengthW, GetWindowTextW, MESSAGEBOX_STYLE, MessageBoxW, SetWindowTextW,
};

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

pub fn get_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut buf = vec![0u16; len + 1];
        GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        from_wide(&buf)
    }
}

pub fn set_text(hwnd: HWND, text: &str) {
    let text = wide(text);
    unsafe {
        SetWindowTextW(hwnd, text.as_ptr());
    }
}

pub const fn loword(v: usize) -> u16 {
    (v & 0xFFFF) as u16
}

pub const fn hiword(v: usize) -> u16 {
    ((v >> 16) & 0xFFFF) as u16
}

pub fn scale(v: i32, dpi: u32) -> i32 {
    (v as i64 * dpi as i64 / 96) as i32
}
