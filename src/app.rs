//! The controller: owns the magnifier and overlay windows, the tray icon and
//! the per-tick zoom logic. Everything here runs on the UI thread.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;
use windows_sys::Win32::Foundation::{
    GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    CreateEllipticRgn, InvalidateRect, SetWindowRgn, UpdateWindow,
};
use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::Magnification::{
    MAGCOLOREFFECT, MAGTRANSFORM, MW_FILTERMODE_EXCLUDE, MagInitialize, MagSetColorEffect,
    MagSetWindowFilterList, MagSetWindowSource, MagSetWindowTransform, MagUninitialize,
    WC_MAGNIFIER,
};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::config::Config;
use crate::geometry;
use crate::input::{InputState, Inputs};
use crate::keys;
use crate::overlay;
use crate::ticker::Ticker;
use crate::ui;
use crate::win::{self, wide};

pub const CONTROLLER_CLASS: &str = "PrecisionZoomController";
const HOST_CLASS: &str = "PrecisionZoomHost";
const OVERLAY_CLASS: &str = "PrecisionZoomOverlay";

pub const WM_APP_TICK: u32 = WM_APP + 1;
const WM_APP_TRAY: u32 = WM_APP + 2;
/// Sent by a second instance to bring up the settings window.
pub const WM_APP_SHOW_SETTINGS: u32 = WM_APP + 3;

const ID_MENU_ENABLED: usize = 1;
const ID_MENU_SETTINGS: usize = 2;
const ID_MENU_EXIT: usize = 3;

const IDI_APP: u16 = 1;
const IDI_APP_OFF: u16 = 2;

pub struct App {
    pub hinst: HINSTANCE,
    controller: HWND,
    host: HWND,
    mag: HWND,
    overlay: HWND,
    pub config: Config,
    reticle_png: Option<Pixmap>,
    input: InputState,
    ticker: Option<Ticker>,
    zoomed: bool,
    /// Unsaved changes are written to the registry at this time. Debounced so
    /// slider drags and held zoom keys don't write on every step.
    save_due: Option<Instant>,
    last_cursor: Option<(i32, i32)>,
    taskbar_created: u32,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Runs `f` on the app state. Returns `None` if the state is missing or
/// already borrowed (a re-entrant window message), which callers treat as "skip".
pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

fn key_down(vk: u16) -> bool {
    vk != keys::NONE && unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0
}

fn register_class(hinst: HINSTANCE, name: &str, proc: WNDPROC) -> Result<(), String> {
    let name = wide(name);
    let wc = WNDCLASSW {
        lpfnWndProc: proc,
        hInstance: hinst,
        hCursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) },
        lpszClassName: name.as_ptr(),
        ..unsafe { std::mem::zeroed() }
    };
    if unsafe { RegisterClassW(&wc) } == 0 {
        return Err(format!("RegisterClass failed ({})", unsafe {
            GetLastError()
        }));
    }
    Ok(())
}

pub fn load_icon(hinst: HINSTANCE, id: u16, size: i32) -> HICON {
    unsafe {
        LoadImageW(
            hinst,
            id as usize as *const u16,
            IMAGE_ICON,
            size,
            size,
            LR_DEFAULTCOLOR,
        ) as HICON
    }
}

pub fn run(open_settings: bool) -> Result<(), String> {
    unsafe {
        if MagInitialize() == 0 {
            return Err("The Windows Magnification API is unavailable.".into());
        }
    }
    let result = run_inner(open_settings);
    APP.with(|a| a.borrow_mut().take()); // stops the ticker
    unsafe {
        MagUninitialize();
    }
    result
}

fn run_inner(open_settings: bool) -> Result<(), String> {
    let hinst = unsafe { GetModuleHandleW(std::ptr::null()) };
    register_class(hinst, CONTROLLER_CLASS, Some(controller_proc))?;
    register_class(hinst, HOST_CLASS, Some(passive_proc))?;
    register_class(hinst, OVERLAY_CLASS, Some(passive_proc))?;

    let config = Config::load();
    let ex =
        WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE;
    let create = |ex: u32, class: &str, title: &str, style: u32, parent: HWND| {
        let class = wide(class);
        let title = wide(title);
        let size = config.size;
        let hwnd = unsafe {
            CreateWindowExW(
                ex,
                class.as_ptr(),
                title.as_ptr(),
                style,
                -32000,
                -32000,
                size,
                size,
                parent,
                std::ptr::null_mut(),
                hinst,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            Err(format!(
                "CreateWindow({}) failed ({})",
                win::from_wide(&class),
                unsafe { GetLastError() }
            ))
        } else {
            Ok(hwnd)
        }
    };
    // Never shown; a normal top-level window (not message-only) so it receives
    // the TaskbarCreated broadcast and a second instance can find it.
    let controller = create(
        0,
        CONTROLLER_CLASS,
        "Precision Zoom",
        WS_POPUP,
        std::ptr::null_mut(),
    )?;
    let host = create(
        ex,
        HOST_CLASS,
        "Precision Zoom",
        WS_POPUP | WS_CLIPCHILDREN,
        std::ptr::null_mut(),
    )?;
    let mag = unsafe {
        CreateWindowExW(
            0,
            WC_MAGNIFIER,
            wide("Magnifier").as_ptr(),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            config.size,
            config.size,
            host,
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        )
    };
    if mag.is_null() {
        return Err(format!(
            "Couldn't create the magnifier control ({})",
            unsafe { GetLastError() }
        ));
    }
    let overlay = create(
        ex,
        OVERLAY_CLASS,
        "Precision Zoom Overlay",
        WS_POPUP,
        std::ptr::null_mut(),
    )?;

    let mut app = App {
        hinst,
        controller,
        host,
        mag,
        overlay,
        config: config.clone(),
        reticle_png: None,
        input: InputState::default(),
        ticker: None,
        zoomed: false,
        save_due: None,
        last_cursor: None,
        taskbar_created: unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
    };
    let startup_warning = app.apply(config).err();
    ui::init(controller, &app.config, app.input.enabled);
    if let Some(warning) = startup_warning {
        // Shown as a banner the next time the settings window opens.
        ui::publish(&app.config, app.input.enabled, true, Some(warning));
    }
    app.add_tray_icon();
    app.ticker = Some(Ticker::start(
        controller,
        WM_APP_TICK,
        app.config.refresh_ms,
    ));
    APP.with(|a| *a.borrow_mut() = Some(app));

    if open_settings {
        ui::show();
    }

    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

impl App {
    /// Applies a new configuration live: no relaunch needed.
    /// Returns a warning if the PNG reticle couldn't be loaded.
    pub fn apply(&mut self, mut cfg: Config) -> Result<(), String> {
        cfg.sanitize();
        let mut warning = Ok(());
        if cfg.png_path != self.config.png_path || self.reticle_png.is_none() {
            self.reticle_png = None;
            if !cfg.png_path.is_empty() {
                match overlay::load_reticle(&cfg.png_path) {
                    Ok(img) => self.reticle_png = Some(img),
                    Err(e) => {
                        cfg.png_path.clear();
                        warning = Err(format!("{e}\n\nThe built-in cross will be used instead."));
                    }
                }
            }
        }
        self.config = cfg;
        let c = &self.config;
        unsafe {
            let flags = SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE;
            SetWindowPos(self.host, std::ptr::null_mut(), 0, 0, c.size, c.size, flags);
            SetWindowPos(self.mag, std::ptr::null_mut(), 0, 0, c.size, c.size, flags);
            SetWindowPos(
                self.overlay,
                std::ptr::null_mut(),
                0,
                0,
                c.size,
                c.size,
                flags,
            );

            // The window owns the region after SetWindowRgn, so no DeleteObject.
            let rgn = if c.circular {
                CreateEllipticRgn(0, 0, c.size + 1, c.size + 1)
            } else {
                std::ptr::null_mut()
            };
            SetWindowRgn(self.host, rgn, 1);

            let alpha = (c.opacity_pct * 255 / 100) as u8;
            SetLayeredWindowAttributes(self.host, 0, alpha, LWA_ALPHA);

            let affinity = if c.hide_from_capture {
                WDA_EXCLUDEFROMCAPTURE
            } else {
                WDA_NONE
            };
            SetWindowDisplayAffinity(self.host, affinity);
            SetWindowDisplayAffinity(self.overlay, affinity);

            let mut effect = MAGCOLOREFFECT {
                transform: [0.0; 25],
            };
            if c.negative {
                #[rustfmt::skip]
                let invert = [
                    -1.0,  0.0,  0.0, 0.0, 0.0,
                     0.0, -1.0,  0.0, 0.0, 0.0,
                     0.0,  0.0, -1.0, 0.0, 0.0,
                     0.0,  0.0,  0.0, 1.0, 0.0,
                     1.0,  1.0,  1.0, 0.0, 1.0,
                ];
                effect.transform = invert;
                MagSetColorEffect(self.mag, &mut effect);
            } else {
                MagSetColorEffect(self.mag, std::ptr::null_mut());
            }
        }
        self.set_transform();
        overlay::present(
            self.overlay,
            &overlay::render(&self.config, self.reticle_png.as_ref()),
        );
        if let Some(t) = &self.ticker {
            t.set_interval(self.config.refresh_ms);
        }
        if self.zoomed {
            self.last_cursor = None; // force a re-layout at the new size
            self.place(cursor_pos());
        }
        self.update_tray();
        warning
    }

    fn schedule_save(&mut self) {
        self.save_due = Some(Instant::now() + Duration::from_millis(600));
    }

    fn save_now(&mut self) {
        self.config.save();
        self.save_due = None;
    }

    /// Tells the settings window about a change it didn't make itself.
    fn publish(&self) {
        ui::publish(&self.config, self.input.enabled, true, None);
    }

    fn set_transform(&self) {
        let z = self.config.zoom;
        let mut m = MAGTRANSFORM {
            v: [z, 0.0, 0.0, 0.0, z, 0.0, 0.0, 0.0, 1.0],
        };
        unsafe {
            MagSetWindowTransform(self.mag, &mut m);
        }
    }

    fn tick(&mut self) {
        let c = &self.config;
        let inputs = Inputs {
            trigger: key_down(c.trigger_key),
            modifier: key_down(c.toggle_modifier),
            zoom_in: key_down(c.zoom_in_key),
            zoom_out: key_down(c.zoom_out_key),
        };
        let out = self.input.update(inputs, Instant::now());
        if out.toggled {
            self.on_enabled_changed();
        }

        if out.zoom_active {
            if out.zoom_steps != 0 {
                let z = geometry::step_zoom(self.config.zoom, out.zoom_steps);
                if z != self.config.zoom {
                    self.config.zoom = z;
                    self.set_transform();
                    self.schedule_save();
                    self.update_tray();
                    self.publish();
                }
            }
            let pt = cursor_pos();
            if self.zoomed {
                self.place(pt);
            } else {
                self.show(pt);
            }
        } else if self.zoomed {
            self.hide();
        }
        if self.save_due.is_some_and(|t| Instant::now() >= t) {
            self.save_now();
        }
    }

    /// Moves both windows (in one atomic step) and refreshes the magnified image.
    fn place(&mut self, pt: (i32, i32)) {
        if self.last_cursor != Some(pt) {
            let (x, y) = geometry::window_origin(pt, self.config.size);
            unsafe {
                let flags = SWP_NOSIZE | SWP_NOACTIVATE;
                let mut dwp = BeginDeferWindowPos(2);
                dwp = DeferWindowPos(dwp, self.overlay, HWND_TOPMOST, x, y, 0, 0, flags);
                dwp = DeferWindowPos(dwp, self.host, self.overlay, x, y, 0, 0, flags);
                EndDeferWindowPos(dwp);
            }
            self.last_cursor = Some(pt);
        }
        let (left, top, right, bottom) =
            geometry::source_rect(pt, self.config.size, self.config.zoom);
        unsafe {
            // The magnifier control only captures when its source is set, so this
            // runs every tick even when the cursor hasn't moved.
            MagSetWindowSource(
                self.mag,
                RECT {
                    left,
                    top,
                    right,
                    bottom,
                },
            );
            InvalidateRect(self.mag, std::ptr::null(), 0);
        }
    }

    /// Position, transform and capture area are set before the windows appear, so
    /// a stale or unscaled frame is never shown, and the reticle is excluded from
    /// magnification before the first paint (no "ghosting").
    fn show(&mut self, pt: (i32, i32)) {
        self.last_cursor = None;
        self.place(pt);
        unsafe {
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW;
            SetWindowPos(self.host, HWND_TOPMOST, 0, 0, 0, 0, flags);
            SetWindowPos(self.overlay, HWND_TOPMOST, 0, 0, 0, 0, flags);
            // The exclusion list is ignored for windows that are hidden when it is
            // set (measured: the reticle got magnified on every activation), so it
            // is set here, after showing and before the first paint.
            let mut filter = [self.host, self.overlay];
            MagSetWindowFilterList(
                self.mag,
                MW_FILTERMODE_EXCLUDE,
                filter.len() as i32,
                filter.as_mut_ptr(),
            );
            UpdateWindow(self.mag);
        }
        self.zoomed = true;
    }

    fn hide(&mut self) {
        unsafe {
            ShowWindow(self.overlay, SW_HIDE);
            ShowWindow(self.host, SW_HIDE);
        }
        self.zoomed = false;
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.input.enabled != enabled {
            self.input.enabled = enabled;
            self.on_enabled_changed();
        }
    }

    fn on_enabled_changed(&mut self) {
        if self.config.beep_on_toggle {
            unsafe {
                MessageBeep(if self.input.enabled {
                    MB_OK
                } else {
                    MB_ICONHAND
                });
            }
        }
        self.update_tray();
        self.publish();
    }

    fn tray_data(&self) -> NOTIFYICONDATAW {
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.controller;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_APP_TRAY;
        let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
        nid.hIcon = load_icon(
            self.hinst,
            if self.input.enabled {
                IDI_APP
            } else {
                IDI_APP_OFF
            },
            size,
        );
        let tip = if self.input.enabled {
            format!(
                "Precision Zoom: on ({:.2}×)\nHold {} to zoom",
                self.config.zoom,
                keys::name_of(self.config.trigger_key)
            )
        } else {
            "Precision Zoom: off".to_string()
        };
        let tip: Vec<u16> = tip.encode_utf16().take(nid.szTip.len() - 1).collect();
        nid.szTip[..tip.len()].copy_from_slice(&tip);
        nid
    }

    fn add_tray_icon(&self) {
        let nid = self.tray_data();
        unsafe {
            Shell_NotifyIconW(NIM_ADD, &nid);
        }
    }

    fn update_tray(&self) {
        let nid = self.tray_data();
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    fn remove_tray_icon(&self) {
        let nid = self.tray_data();
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }

    fn tray_menu_state(&self) -> (HWND, bool) {
        (self.controller, self.input.enabled)
    }
}

fn cursor_pos() -> (i32, i32) {
    let mut pt = POINT { x: 0, y: 0 };
    unsafe {
        GetCursorPos(&mut pt);
    }
    (pt.x, pt.y)
}

fn show_tray_menu() {
    let Some((hwnd, enabled)) = with_app(|a| a.tray_menu_state()) else {
        return;
    };
    unsafe {
        let menu = CreatePopupMenu();
        let check = if enabled { MF_CHECKED } else { MF_UNCHECKED };
        AppendMenuW(
            menu,
            MF_STRING | check,
            ID_MENU_ENABLED,
            wide("&Enabled").as_ptr(),
        );
        AppendMenuW(
            menu,
            MF_STRING,
            ID_MENU_SETTINGS,
            wide("&Settings…").as_ptr(),
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, ID_MENU_EXIT, wide("E&xit").as_ptr());
        SetMenuDefaultItem(menu, ID_MENU_SETTINGS as u32, 0);
        let pt = cursor_pos();
        // Required so the menu closes when clicking elsewhere (KB135788).
        SetForegroundWindow(hwnd);
        TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            pt.0,
            pt.1,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
    }
}

unsafe extern "system" fn passive_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

unsafe extern "system" fn controller_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_APP_TICK => {
            with_app(|a| {
                if let Some(t) = &a.ticker {
                    t.ack();
                }
                a.tick();
            });
            0
        }
        WM_APP_TRAY => {
            match lp as u32 {
                WM_RBUTTONUP | WM_CONTEXTMENU => show_tray_menu(),
                WM_LBUTTONDBLCLK => ui::show(),
                _ => {}
            }
            0
        }
        WM_APP_SHOW_SETTINGS => {
            ui::show();
            0
        }
        ui::WM_APP_APPLY => {
            let cfg = unsafe { ui::take_applied(lp) };
            with_app(|a| {
                let warning = a.apply(cfg).err();
                a.schedule_save();
                ui::publish(&a.config, a.input.enabled, false, warning);
            });
            0
        }
        ui::WM_APP_SET_ENABLED => {
            with_app(|a| a.set_enabled(wp != 0));
            0
        }
        WM_COMMAND => {
            match win::loword(wp) as usize {
                ID_MENU_ENABLED => {
                    with_app(|a| a.set_enabled(!a.input.enabled));
                }
                ID_MENU_SETTINGS => ui::show(),
                ID_MENU_EXIT => unsafe {
                    DestroyWindow(hwnd);
                },
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            with_app(|a| {
                a.remove_tray_icon();
                if a.save_due.is_some() {
                    a.save_now();
                }
            });
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => {
            // Explorer restarted: the tray icon must be added again.
            if with_app(|a| msg == a.taskbar_created) == Some(true) {
                with_app(|a| a.add_tray_icon());
                return 0;
            }
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
    }
}
