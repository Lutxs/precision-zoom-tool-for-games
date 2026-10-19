//! Settings window. Validates input before applying, applies changes live
//! (no relaunch), and lays itself out per monitor DPI.

use std::cell::{Cell, RefCell};
use std::fmt::Display;
use std::ops::RangeInclusive;
use std::str::FromStr;

use windows_sys::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, CreateFontIndirectW, CreateSolidBrush, DeleteObject, HBRUSH, HFONT,
    InvalidateRect,
};
use windows_sys::Win32::System::SystemServices::{
    SS_LEFT, SS_NOPREFIX, SS_NOTIFY, SS_PATHELLIPSIS,
};
use windows_sys::Win32::UI::Controls::Dialogs::{
    CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW, GetOpenFileNameW, OFN_FILEMUSTEXIST,
    OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows_sys::Win32::UI::Controls::{BST_CHECKED, BST_UNCHECKED, EM_SETSEL};
use windows_sys::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForWindow, SystemParametersInfoForDpi,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::app::{self, with_app};
use crate::config::{self, Config, parse_field};
use crate::keys;
use crate::overlay;
use crate::win::{self, get_text, scale, set_text, wide};

const CLASS: &str = "PrecisionZoomSettings";

const ID_ZOOM: i32 = 100;
const ID_SIZE: i32 = 101;
const ID_OPACITY: i32 = 102;
const ID_REFRESH: i32 = 103;
const ID_BORDER: i32 = 104;
const ID_BORDER_SWATCH: i32 = 105;
const ID_BORDER_COLOR: i32 = 106;
const ID_CIRCULAR: i32 = 107;
const ID_NEGATIVE: i32 = 108;
const ID_RETICLE: i32 = 109;
const ID_RETICLE_SIZE: i32 = 110;
const ID_RETICLE_THICKNESS: i32 = 111;
const ID_RETICLE_SWATCH: i32 = 112;
const ID_RETICLE_COLOR: i32 = 113;
const ID_OFFSET_X: i32 = 114;
const ID_OFFSET_Y: i32 = 115;
const ID_LOAD_PNG: i32 = 116;
const ID_CLEAR_PNG: i32 = 117;
const ID_PNG_LABEL: i32 = 118;
const ID_TRIGGER: i32 = 119;
const ID_MODIFIER: i32 = 120;
const ID_ZOOM_IN: i32 = 121;
const ID_ZOOM_OUT: i32 = 122;
const ID_BEEP: i32 = 123;
const ID_HIDE_CAPTURE: i32 = 124;
const ID_STATUS: i32 = 125;
const ID_RESET: i32 = 126;
const ID_APPLY: i32 = IDOK; // Enter
const ID_CLOSE: i32 = IDCANCEL; // Esc
const ID_STATIC: i32 = -1;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Group,
    Label,
    Note,
    Edit,
    Check,
    Combo,
    Button,
    DefButton,
    Swatch,
    Path,
}

/// One control, positioned on a grid in 96-DPI units: `col` picks the column,
/// `row` the 28-px row, `x`/`w` are relative to the column, `rows` is the height in rows.
#[derive(Clone, Copy)]
struct Item {
    id: i32,
    kind: Kind,
    text: &'static str,
    col: i32,
    row: i32,
    x: i32,
    w: i32,
    rows: i32,
}

const fn it(id: i32, kind: Kind, text: &'static str, col: i32, row: i32, x: i32, w: i32) -> Item {
    Item {
        id,
        kind,
        text,
        col,
        row,
        x,
        w,
        rows: 1,
    }
}

const fn group(text: &'static str, col: i32, row: i32, rows: i32) -> Item {
    Item {
        id: ID_STATIC,
        kind: Kind::Group,
        text,
        col,
        row,
        x: 0,
        w: COL_W,
        rows,
    }
}

const MARGIN: i32 = 12;
const ROW_H: i32 = 28;
const COL_W: i32 = 292;
const COL_X: [i32; 2] = [MARGIN, MARGIN + COL_W + 12];
const ROWS: i32 = 16;
const CLIENT_W: i32 = COL_X[1] + COL_W + MARGIN;
const CLIENT_H: i32 = MARGIN + ROWS * ROW_H + 4;

use Kind::*;
// Order is the tab order.
#[rustfmt::skip]
const ITEMS: &[Item] = &[
    group("Magnifier", 0, 0, 8),
    it(ID_STATIC, Label, "Zoom factor", 0, 1, 12, 130),
    it(ID_ZOOM, Edit, "", 0, 1, 150, 50),
    it(ID_STATIC, Note, "1.1 – 16", 0, 1, 208, 76),
    it(ID_STATIC, Label, "Box size (px)", 0, 2, 12, 130),
    it(ID_SIZE, Edit, "", 0, 2, 150, 50),
    it(ID_STATIC, Note, "64 – 1600", 0, 2, 208, 76),
    it(ID_STATIC, Label, "Opacity (%)", 0, 3, 12, 130),
    it(ID_OPACITY, Edit, "", 0, 3, 150, 50),
    it(ID_STATIC, Note, "10 – 100", 0, 3, 208, 76),
    it(ID_STATIC, Label, "Refresh interval (ms)", 0, 4, 12, 136),
    it(ID_REFRESH, Edit, "", 0, 4, 150, 50),
    it(ID_STATIC, Note, "1 – 100", 0, 4, 208, 76),
    it(ID_STATIC, Label, "Border (px)", 0, 5, 12, 130),
    it(ID_BORDER, Edit, "", 0, 5, 150, 50),
    it(ID_BORDER_SWATCH, Swatch, "", 0, 5, 208, 20),
    it(ID_BORDER_COLOR, Button, "Color…", 0, 5, 232, 52),
    it(ID_CIRCULAR, Check, "Circular scope", 0, 6, 12, 270),
    it(ID_NEGATIVE, Check, "Negative mode (invert colors)", 0, 7, 12, 270),

    group("Reticle", 0, 8, 8),
    it(ID_RETICLE, Check, "Show reticle", 0, 9, 12, 270),
    it(ID_STATIC, Label, "Size (px)", 0, 10, 12, 130),
    it(ID_RETICLE_SIZE, Edit, "", 0, 10, 150, 50),
    it(ID_STATIC, Label, "Line width (px)", 0, 11, 12, 130),
    it(ID_RETICLE_THICKNESS, Edit, "", 0, 11, 150, 50),
    it(ID_RETICLE_SWATCH, Swatch, "", 0, 11, 208, 20),
    it(ID_RETICLE_COLOR, Button, "Color…", 0, 11, 232, 52),
    it(ID_STATIC, Label, "X offset (px)", 0, 12, 12, 130),
    it(ID_OFFSET_X, Edit, "", 0, 12, 150, 50),
    it(ID_STATIC, Label, "Y offset (px)", 0, 13, 12, 130),
    it(ID_OFFSET_Y, Edit, "", 0, 13, 150, 50),
    it(ID_LOAD_PNG, Button, "Load PNG…", 0, 14, 12, 110),
    it(ID_CLEAR_PNG, Button, "Use built-in cross", 0, 14, 128, 140),
    it(ID_PNG_LABEL, Path, "", 0, 15, 12, 272),

    group("Hotkeys", 1, 0, 6),
    it(ID_STATIC, Label, "Hold to zoom", 1, 1, 12, 110),
    it(ID_TRIGGER, Combo, "", 1, 1, 126, 158),
    it(ID_STATIC, Label, "Toggle on/off", 1, 2, 12, 110),
    it(ID_MODIFIER, Combo, "", 1, 2, 126, 158),
    it(ID_STATIC, Note, "Hold the toggle key, then press the zoom key.", 1, 3, 12, 272),
    it(ID_STATIC, Label, "Zoom in", 1, 4, 12, 110),
    it(ID_ZOOM_IN, Combo, "", 1, 4, 126, 158),
    it(ID_STATIC, Label, "Zoom out", 1, 5, 12, 110),
    it(ID_ZOOM_OUT, Combo, "", 1, 5, 126, 158),

    group("Behavior", 1, 6, 5),
    it(ID_BEEP, Check, "Beep when toggled on/off", 1, 7, 12, 270),
    it(ID_HIDE_CAPTURE, Check, "Hide overlay from screen capture", 1, 8, 12, 270),
    Item { id: ID_STATIC, kind: Note, text: "Keeps the overlay out of screenshots and recordings. Some anti-cheat systems may treat this as suspicious.", col: 1, row: 9, x: 30, w: 254, rows: 2 },

    Item { id: ID_STATUS, kind: Note, text: "", col: 1, row: 12, x: 4, w: 284, rows: 2 },
    it(ID_RESET, Button, "Reset to defaults", 1, 15, 4, 118),
    it(ID_APPLY, DefButton, "Apply", 1, 15, 128, 76),
    it(ID_CLOSE, Button, "Close", 1, 15, 210, 76),
];

struct State {
    hwnd: HWND,
    dpi: u32,
    font: HFONT,
    controls: Vec<(HWND, Item)>,
    border_color: u32,
    reticle_color: u32,
    border_brush: HBRUSH,
    reticle_brush: HBRUSH,
    png_path: String,
    custom_colors: [COLORREF; 16],
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static HWND_SETTINGS: Cell<HWND> = const { Cell::new(std::ptr::null_mut()) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut s| s.as_mut().map(f)))
}

fn hwnd() -> HWND {
    HWND_SETTINGS.with(Cell::get)
}

fn ctrl(id: i32) -> HWND {
    unsafe { GetDlgItem(hwnd(), id) }
}

pub fn create(hinst: HINSTANCE) -> Result<(), String> {
    let class = wide(CLASS);
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: hinst,
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_BTNFACE + 1) as usize as HBRUSH,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.as_ptr(),
            wide("Precision Zoom Settings").as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err("Couldn't create the settings window.".into());
        }
        HWND_SETTINGS.with(|h| h.set(hwnd));

        let big = app::load_icon(hinst, 1, GetSystemMetrics(SM_CXICON));
        let small = app::load_icon(hinst, 1, GetSystemMetrics(SM_CXSMICON));
        SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as isize);
        SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as isize);

        let mut controls = Vec::with_capacity(ITEMS.len());
        for item in ITEMS {
            let (cls, style, ex) = match item.kind {
                Group => ("BUTTON", BS_GROUPBOX as u32, 0),
                Label | Note => ("STATIC", SS_LEFT, 0),
                Path => ("STATIC", SS_LEFT | SS_PATHELLIPSIS | SS_NOPREFIX, 0),
                Swatch => ("STATIC", SS_LEFT | SS_NOTIFY | WS_BORDER, 0),
                Edit => ("EDIT", ES_AUTOHSCROLL as u32 | WS_TABSTOP, WS_EX_CLIENTEDGE),
                Check => ("BUTTON", BS_AUTOCHECKBOX as u32 | WS_TABSTOP, 0),
                Combo => (
                    "COMBOBOX",
                    (CBS_DROPDOWNLIST as u32) | WS_VSCROLL | WS_TABSTOP,
                    0,
                ),
                Button => ("BUTTON", BS_PUSHBUTTON as u32 | WS_TABSTOP, 0),
                DefButton => ("BUTTON", BS_DEFPUSHBUTTON as u32 | WS_TABSTOP, 0),
            };
            let c = CreateWindowExW(
                ex,
                wide(cls).as_ptr(),
                wide(item.text).as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                0,
                0,
                0,
                0,
                hwnd,
                item.id as isize as _,
                hinst,
                std::ptr::null(),
            );
            if item.kind == Combo {
                for k in keys::all() {
                    SendMessageW(c, CB_ADDSTRING, 0, wide(&k.name).as_ptr() as isize);
                }
            }
            controls.push((c, *item));
        }

        let defaults = Config::default();
        let state = State {
            hwnd,
            dpi: GetDpiForWindow(hwnd),
            font: std::ptr::null_mut(),
            controls,
            border_color: defaults.border_color,
            reticle_color: defaults.reticle_color,
            border_brush: std::ptr::null_mut(),
            reticle_brush: std::ptr::null_mut(),
            png_path: String::new(),
            custom_colors: [0x00FF_FFFF; 16],
        };
        STATE.with(|s| *s.borrow_mut() = Some(state));
        with_state(|s| s.layout(true));
    }
    Ok(())
}

impl State {
    /// Positions every control for the current DPI and sizes the window to fit.
    fn layout(&mut self, center: bool) {
        let dpi = self.dpi;
        let s = |v| scale(v, dpi);
        unsafe {
            let mut ncm: NONCLIENTMETRICSW = std::mem::zeroed();
            ncm.cbSize = size_of::<NONCLIENTMETRICSW>() as u32;
            SystemParametersInfoForDpi(
                SPI_GETNONCLIENTMETRICS,
                ncm.cbSize,
                (&mut ncm as *mut NONCLIENTMETRICSW).cast(),
                0,
                dpi,
            );
            let font = CreateFontIndirectW(&ncm.lfMessageFont);
            for &(c, item) in &self.controls {
                let x = COL_X[item.col as usize] + item.x;
                let y = MARGIN + item.row * ROW_H;
                let (dy, h) = match item.kind {
                    Group => (4, item.rows * ROW_H - 6),
                    Label => (3, 20),
                    Note | Path => (3, item.rows * ROW_H - 6),
                    Swatch => (1, 21),
                    Check => (2, 20),
                    Edit => (0, 23),
                    Combo => (0, 300), // includes the drop-down list
                    Button | DefButton => (-2, 27),
                };
                MoveWindow(c, s(x), s(y + dy), s(item.w), s(h), 0);
                SendMessageW(c, WM_SETFONT, font as usize, 0);
            }
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
            self.font = font;

            let style = GetWindowLongW(self.hwnd, GWL_STYLE) as u32;
            let ex = GetWindowLongW(self.hwnd, GWL_EXSTYLE) as u32;
            let mut rc = RECT {
                left: 0,
                top: 0,
                right: s(CLIENT_W),
                bottom: s(CLIENT_H),
            };
            AdjustWindowRectExForDpi(&mut rc, style, 0, ex, dpi);
            let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
            if center {
                let x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
                let y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2;
                SetWindowPos(
                    self.hwnd,
                    std::ptr::null_mut(),
                    x,
                    y,
                    w,
                    h,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            } else {
                SetWindowPos(
                    self.hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    w,
                    h,
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE,
                );
            }
            InvalidateRect(self.hwnd, std::ptr::null(), 1);
        }
    }

    fn set_colors(&mut self, border: u32, reticle: u32) {
        self.border_color = border;
        self.reticle_color = reticle;
        unsafe {
            for brush in [self.border_brush, self.reticle_brush] {
                if !brush.is_null() {
                    DeleteObject(brush);
                }
            }
            self.border_brush = CreateSolidBrush(border);
            self.reticle_brush = CreateSolidBrush(reticle);
        }
        for id in [ID_BORDER_SWATCH, ID_RETICLE_SWATCH] {
            unsafe {
                InvalidateRect(GetDlgItem(self.hwnd, id), std::ptr::null(), 1);
            }
        }
    }

    fn set_png(&mut self, path: &str) {
        self.png_path = path.to_string();
        let label = if path.is_empty() {
            "Reticle image: built-in cross"
        } else {
            path
        };
        set_text(ctrl(ID_PNG_LABEL), label);
    }
}

fn set_check(id: i32, on: bool) {
    unsafe {
        SendMessageW(
            ctrl(id),
            BM_SETCHECK,
            if on { BST_CHECKED } else { BST_UNCHECKED } as usize,
            0,
        );
    }
}

fn get_check(id: i32) -> bool {
    unsafe { SendMessageW(ctrl(id), BM_GETCHECK, 0, 0) as u32 == BST_CHECKED }
}

fn set_key(id: i32, vk: u16) {
    unsafe {
        SendMessageW(ctrl(id), CB_SETCURSEL, keys::index_of(vk).unwrap_or(0), 0);
    }
}

fn get_key(id: i32) -> u16 {
    let i = unsafe { SendMessageW(ctrl(id), CB_GETCURSEL, 0, 0) };
    keys::all()
        .get(i.max(0) as usize)
        .map_or(keys::NONE, |k| k.vk)
}

fn status(text: &str) {
    set_text(ctrl(ID_STATUS), text);
}

fn fill(c: &Config) {
    set_text(ctrl(ID_ZOOM), &format!("{:.2}", c.zoom));
    set_text(ctrl(ID_SIZE), &c.size.to_string());
    set_text(ctrl(ID_OPACITY), &c.opacity_pct.to_string());
    set_text(ctrl(ID_REFRESH), &c.refresh_ms.to_string());
    set_text(ctrl(ID_BORDER), &c.border_px.to_string());
    set_text(ctrl(ID_RETICLE_SIZE), &c.reticle_size.to_string());
    set_text(ctrl(ID_RETICLE_THICKNESS), &c.reticle_thickness.to_string());
    set_text(ctrl(ID_OFFSET_X), &c.offset_x.to_string());
    set_text(ctrl(ID_OFFSET_Y), &c.offset_y.to_string());
    set_check(ID_CIRCULAR, c.circular);
    set_check(ID_NEGATIVE, c.negative);
    set_check(ID_RETICLE, c.reticle);
    set_check(ID_BEEP, c.beep_on_toggle);
    set_check(ID_HIDE_CAPTURE, c.hide_from_capture);
    set_key(ID_TRIGGER, c.trigger_key);
    set_key(ID_MODIFIER, c.toggle_modifier);
    set_key(ID_ZOOM_IN, c.zoom_in_key);
    set_key(ID_ZOOM_OUT, c.zoom_out_key);
    with_state(|s| {
        s.set_colors(c.border_color, c.reticle_color);
        s.set_png(&c.png_path);
    });
}

/// Reads and validates the form. On error returns the offending control and a message.
fn read() -> Result<Config, (i32, String)> {
    fn num<T>(id: i32, label: &str, range: RangeInclusive<T>) -> Result<T, (i32, String)>
    where
        T: FromStr + PartialOrd + Display + Copy,
    {
        parse_field(label, &get_text(ctrl(id)), range).map_err(|e| (id, e))
    }
    let size = num(ID_SIZE, "Box size", config::SIZE)?;
    let max_offset = size / 2;
    let (border_color, reticle_color, png_path) =
        with_state(|s| (s.border_color, s.reticle_color, s.png_path.clone())).unwrap_or_default();
    let c = Config {
        zoom: num(ID_ZOOM, "Zoom factor", config::ZOOM)?,
        size,
        opacity_pct: num(ID_OPACITY, "Opacity", config::OPACITY_PCT)?,
        refresh_ms: num(ID_REFRESH, "Refresh interval", config::REFRESH_MS)?,
        border_px: num(ID_BORDER, "Border", config::BORDER_PX)?,
        border_color,
        circular: get_check(ID_CIRCULAR),
        negative: get_check(ID_NEGATIVE),
        reticle: get_check(ID_RETICLE),
        reticle_size: num(ID_RETICLE_SIZE, "Reticle size", config::RETICLE_SIZE)?,
        reticle_thickness: num(
            ID_RETICLE_THICKNESS,
            "Line width",
            config::RETICLE_THICKNESS,
        )?,
        reticle_color,
        offset_x: num(ID_OFFSET_X, "X offset", -max_offset..=max_offset)?,
        offset_y: num(ID_OFFSET_Y, "Y offset", -max_offset..=max_offset)?,
        png_path,
        trigger_key: get_key(ID_TRIGGER),
        toggle_modifier: get_key(ID_MODIFIER),
        zoom_in_key: get_key(ID_ZOOM_IN),
        zoom_out_key: get_key(ID_ZOOM_OUT),
        hide_from_capture: get_check(ID_HIDE_CAPTURE),
        beep_on_toggle: get_check(ID_BEEP),
    };
    c.check_hotkeys().map_err(|e| (ID_TRIGGER, e))?;
    Ok(c)
}

fn apply() {
    let cfg = match read() {
        Ok(c) => c,
        Err((id, msg)) => {
            win::message_box(hwnd(), &msg, MB_ICONWARNING);
            unsafe {
                let c = ctrl(id);
                SetFocus(c);
                SendMessageW(c, EM_SETSEL, 0, -1);
            }
            return;
        }
    };
    let Some((result, applied)) = with_app(|a| {
        let r = a.apply(cfg);
        a.config.save();
        (r, a.config.clone())
    }) else {
        return;
    };
    // Reflect anything the app adjusted (e.g. a PNG that failed to load).
    with_state(|s| s.set_png(&applied.png_path));
    match result {
        Ok(()) => status("Settings applied and saved."),
        Err(warning) => {
            status("Settings applied, but the PNG reticle couldn't be loaded.");
            win::message_box(hwnd(), &warning, MB_ICONWARNING);
        }
    }
}

fn pick_color(for_border: bool) {
    let Some((current, mut custom)) = with_state(|s| {
        (
            if for_border {
                s.border_color
            } else {
                s.reticle_color
            },
            s.custom_colors,
        )
    }) else {
        return;
    };
    let mut cc: CHOOSECOLORW = unsafe { std::mem::zeroed() };
    cc.lStructSize = size_of::<CHOOSECOLORW>() as u32;
    cc.hwndOwner = hwnd();
    cc.rgbResult = current;
    cc.lpCustColors = custom.as_mut_ptr();
    cc.Flags = CC_FULLOPEN | CC_RGBINIT;
    // Modal: no state borrow may be held here, ticks keep running meanwhile.
    if unsafe { ChooseColorW(&mut cc) } != 0 {
        with_state(|s| {
            s.custom_colors = custom;
            let (b, r) = if for_border {
                (cc.rgbResult, s.reticle_color)
            } else {
                (s.border_color, cc.rgbResult)
            };
            s.set_colors(b, r);
        });
        status("Color changed. Click Apply to use it.");
    }
}

fn load_png() {
    let mut file = [0u16; 1024];
    let filter: Vec<u16> = "PNG images (*.png)\0*.png\0All files\0*.*\0\0"
        .encode_utf16()
        .collect();
    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = hwnd();
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = file.as_mut_ptr();
    ofn.nMaxFile = file.len() as u32;
    ofn.Flags = OFN_PATHMUSTEXIST | OFN_FILEMUSTEXIST;
    if unsafe { GetOpenFileNameW(&mut ofn) } == 0 {
        return;
    }
    let path = win::from_wide(&file);
    // Validate now so a bad file is reported right away, not on Apply.
    match overlay::load_reticle(&path) {
        Ok(_) => {
            with_state(|s| s.set_png(&path));
            set_check(ID_RETICLE, true);
            status("Reticle image selected. Click Apply to use it.");
        }
        Err(e) => {
            win::message_box(hwnd(), &e, MB_ICONWARNING);
        }
    }
}

/// Shows the window, refreshed from the live settings.
pub fn show() {
    let h = hwnd();
    if h.is_null() {
        return;
    }
    unsafe {
        if IsWindowVisible(h) == 0 {
            if let Some(cfg) = with_app(|a| a.config.clone()) {
                fill(&cfg);
            }
            status("Changes apply instantly — no restart needed.");
        }
        ShowWindow(h, SW_SHOWNORMAL);
        SetForegroundWindow(h);
    }
}

/// Keeps the zoom field in sync when the zoom hotkeys are used.
pub fn zoom_changed(zoom: f32) {
    let h = hwnd();
    if !h.is_null() && unsafe { IsWindowVisible(h) } != 0 {
        set_text(ctrl(ID_ZOOM), &format!("{zoom:.2}"));
    }
}

/// Gives the settings window keyboard navigation (Tab, Enter = Apply, Esc = Close).
pub fn is_dialog_message(msg: &MSG) -> bool {
    let h = hwnd();
    !h.is_null() && unsafe { IsWindowVisible(h) != 0 && IsDialogMessageW(h, msg) != 0 }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = win::loword(wp) as i16 as i32;
            let code = win::hiword(wp) as u32;
            match (id, code) {
                (ID_APPLY, BN_CLICKED) => apply(),
                (ID_CLOSE, BN_CLICKED) => unsafe {
                    ShowWindow(hwnd, SW_HIDE);
                },
                (ID_RESET, BN_CLICKED) => {
                    fill(&Config::default());
                    status("Defaults loaded. Click Apply to use them.");
                }
                (ID_BORDER_COLOR, BN_CLICKED) => pick_color(true),
                (ID_RETICLE_COLOR, BN_CLICKED) => pick_color(false),
                (ID_BORDER_SWATCH, STN_CLICKED) => pick_color(true),
                (ID_RETICLE_SWATCH, STN_CLICKED) => pick_color(false),
                (ID_LOAD_PNG, BN_CLICKED) => load_png(),
                (ID_CLEAR_PNG, BN_CLICKED) => {
                    with_state(|s| s.set_png(""));
                    status("Built-in cross selected. Click Apply to use it.");
                }
                _ => {}
            }
            0
        }
        WM_CTLCOLORSTATIC => {
            let id = unsafe { GetDlgCtrlID(lp as HWND) };
            let brush = with_state(|s| match id {
                ID_BORDER_SWATCH => s.border_brush,
                ID_RETICLE_SWATCH => s.reticle_brush,
                _ => std::ptr::null_mut(),
            });
            match brush {
                Some(b) if !b.is_null() => b as LRESULT,
                _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
            }
        }
        WM_DPICHANGED => {
            with_state(|s| {
                s.dpi = win::hiword(wp) as u32;
                s.layout(false);
            });
            let rc = unsafe { &*(lp as *const RECT) };
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    rc.left,
                    rc.top,
                    0,
                    0,
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSIZE,
                );
            }
            0
        }
        WM_CLOSE => {
            unsafe { ShowWindow(hwnd, SW_HIDE) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
