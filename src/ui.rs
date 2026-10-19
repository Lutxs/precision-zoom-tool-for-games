//! Settings window (egui). It runs on its own thread with its own event loop and
//! talks to the controller only through posted messages (UI → app) and the
//! `SHARED` snapshot (app → UI), so the zoom logic never waits on the UI.
//!
//! Every change applies instantly; the controller saves shortly afterwards.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, ColorImage, CornerRadius, Layout, Margin, RichText, Stroke,
    TextureHandle, TextureOptions, Vec2, ViewportCommand,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tiny_skia::Pixmap;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
};
use windows_sys::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, PostMessageW, WM_APP};

use crate::config::{self, Config};
use crate::icon_art;
use crate::keys;
use crate::overlay;
use crate::preview;
use crate::ui_theme::{self as theme, card, row, toggle};
use crate::win;

/// UI → app: `lparam` is a `Box<Config>` from `Box::into_raw`.
pub const WM_APP_APPLY: u32 = WM_APP + 10;
/// UI → app: `wparam` is the new enabled state (0 or 1).
pub const WM_APP_SET_ENABLED: u32 = WM_APP + 11;

const VK_ESCAPE: u16 = 0x1B;
const RESET_CONFIRM: Duration = Duration::from_secs(3);

struct Shared {
    config: Config,
    enabled: bool,
    notice: Option<String>,
    revision: u64,
    /// The last change came from the app (hotkeys, tray) rather than this window.
    external: bool,
}

static SHARED: Mutex<Option<Shared>> = Mutex::new(None);
static CONTROLLER: AtomicUsize = AtomicUsize::new(0);
static CTX: OnceLock<egui::Context> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);
static SHOW_REQUESTED: AtomicBool = AtomicBool::new(false);

fn shared() -> std::sync::MutexGuard<'static, Option<Shared>> {
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn init(controller: HWND, config: &Config, enabled: bool) {
    CONTROLLER.store(controller as usize, Ordering::Relaxed);
    *shared() = Some(Shared {
        config: config.clone(),
        enabled,
        notice: None,
        revision: 0,
        external: true,
    });
}

/// Called by the app whenever its state changes.
pub fn publish(config: &Config, enabled: bool, external: bool, notice: Option<String>) {
    if let Some(s) = shared().as_mut() {
        s.config = config.clone();
        s.enabled = enabled;
        s.external = external;
        if notice.is_some() {
            s.notice = notice;
        }
        s.revision += 1;
    }
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint();
    }
}

/// Opens (or brings back) the settings window. Callable from any thread.
pub fn show() {
    SHOW_REQUESTED.store(true, Ordering::Release);
    if !STARTED.swap(true, Ordering::AcqRel) {
        let spawned = std::thread::Builder::new()
            .name("settings-ui".into())
            .spawn(run);
        if spawned.is_err() {
            STARTED.store(false, Ordering::Release);
        }
    } else if let Some(ctx) = CTX.get() {
        // While hidden, eframe only runs `App::logic`, which handles the request.
        ctx.request_repaint();
    }
}

/// Reclaims the config sent with [`WM_APP_APPLY`].
///
/// # Safety
/// `lparam` must come from a `WM_APP_APPLY` message posted by this module.
pub unsafe fn take_applied(lparam: isize) -> Config {
    *unsafe { Box::from_raw(lparam as *mut Config) }
}

fn post(msg: u32, wparam: usize, lparam: isize) -> bool {
    let hwnd = CONTROLLER.load(Ordering::Relaxed) as HWND;
    unsafe { PostMessageW(hwnd, msg, wparam, lparam) != 0 }
}

fn key_down(vk: u16) -> bool {
    vk != keys::NONE && unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0
}

fn run() {
    let icon = icon_art::render(64, icon_art::ACCENT_ON);
    let icon = egui::IconData {
        rgba: icon.into_iter().flatten().collect(),
        width: 64,
        height: 64,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Precision Zoom")
            .with_app_id("PrecisionZoom")
            .with_inner_size([960.0, 880.0])
            .with_min_inner_size([860.0, 600.0])
            .with_icon(Arc::new(icon)),
        renderer: eframe::Renderer::Glow,
        centered: true,
        event_loop_builder: Some(Box::new(|builder| {
            use winit::platform::windows::EventLoopBuilderExtWindows;
            builder.with_any_thread(true);
        })),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Precision Zoom",
        options,
        Box::new(|cc| Ok(Box::new(SettingsApp::new(cc)))),
    );
    if let Err(e) = result {
        win::message_box(
            std::ptr::null_mut(),
            &format!("Couldn't open the settings window:\n{e}\n\nIt needs OpenGL 2.0 or newer."),
            MB_ICONERROR,
        );
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Trigger,
    Modifier,
    ZoomIn,
    ZoomOut,
}

impl Slot {
    const ALL: [Slot; 4] = [Slot::Trigger, Slot::Modifier, Slot::ZoomIn, Slot::ZoomOut];

    fn label(self) -> &'static str {
        match self {
            Slot::Trigger => "Hold to zoom",
            Slot::Modifier => "Toggle on/off",
            Slot::ZoomIn => "Zoom in",
            Slot::ZoomOut => "Zoom out",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Slot::Trigger => "Shows the magnifier while held",
            Slot::Modifier => "Hold it, then press the zoom key",
            Slot::ZoomIn | Slot::ZoomOut => "While zoomed; hold to repeat",
        }
    }

    fn get(self, c: &Config) -> u16 {
        match self {
            Slot::Trigger => c.trigger_key,
            Slot::Modifier => c.toggle_modifier,
            Slot::ZoomIn => c.zoom_in_key,
            Slot::ZoomOut => c.zoom_out_key,
        }
    }

    fn set(self, c: &mut Config, vk: u16) {
        match self {
            Slot::Trigger => c.trigger_key = vk,
            Slot::Modifier => c.toggle_modifier = vk,
            Slot::ZoomIn => c.zoom_in_key = vk,
            Slot::ZoomOut => c.zoom_out_key = vk,
        }
    }
}

struct Binding {
    slot: Slot,
    /// Keys already down when binding started; they must be released first.
    held: Vec<u16>,
}

#[derive(Clone, Copy, PartialEq)]
enum ReticleStyle {
    Cross,
    Image,
}

struct SettingsApp {
    hwnd: HWND,
    cfg: Config,
    /// Last config sent to the app.
    sent: Config,
    enabled: bool,
    seen_revision: u64,
    notice: Option<String>,
    binding: Option<Binding>,
    hotkey_error: Option<String>,
    png_error: Option<String>,
    /// Decoded reticle image for the preview, keyed by path.
    png: Option<(String, Pixmap)>,
    preview: Option<(Config, u32, TextureHandle)>,
    icons: [TextureHandle; 2],
    reset_armed: Option<Instant>,
}

impl SettingsApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        theme::install(ctx);
        let _ = CTX.set(ctx.clone());

        let hwnd = match cc.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(h)) => h.hwnd.get() as HWND,
            _ => std::ptr::null_mut(),
        };
        if !hwnd.is_null() {
            // Dark title bar matching the window background (caption color: Windows 11).
            let [r, g, b, _] = theme::BG.to_array();
            let dark: i32 = 1;
            let caption = config::rgb(r, g, b);
            unsafe {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                    (&dark as *const i32).cast(),
                    4,
                );
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_CAPTION_COLOR as u32,
                    (&caption as *const u32).cast(),
                    4,
                );
            }
        }

        let icon = |accent| {
            let px: Vec<u8> = icon_art::render(64, accent).into_iter().flatten().collect();
            ctx.load_texture(
                "icon",
                ColorImage::from_rgba_unmultiplied([64, 64], &px),
                TextureOptions::LINEAR,
            )
        };
        let icons = [icon(icon_art::ACCENT_ON), icon(icon_art::ACCENT_OFF)];

        let (cfg, enabled, revision) = shared()
            .as_ref()
            .map(|s| (s.config.clone(), s.enabled, s.revision))
            .unwrap_or((Config::default(), true, 0));
        SHOW_REQUESTED.store(false, Ordering::Release);
        Self {
            hwnd,
            sent: cfg.clone(),
            cfg,
            enabled,
            seen_revision: revision,
            notice: None,
            binding: None,
            hotkey_error: None,
            png_error: None,
            png: None,
            preview: None,
            icons,
            reset_armed: None,
        }
    }

    /// Takes over app-side changes: always on `force`, otherwise only those the
    /// app made itself, so in-flight edits from this window aren't reverted.
    fn sync_from_app(&mut self, force: bool) {
        let guard = shared();
        let Some(s) = guard.as_ref() else { return };
        if s.revision == self.seen_revision && !force {
            return;
        }
        self.seen_revision = s.revision;
        self.enabled = s.enabled;
        if s.notice.is_some() {
            self.notice.clone_from(&s.notice);
        }
        if s.external || force {
            self.cfg = s.config.clone();
            self.sent = self.cfg.clone();
        }
    }

    fn push_changes(&mut self) {
        self.cfg.sanitize();
        if self.cfg == self.sent {
            return;
        }
        if let Err(e) = self.cfg.check_hotkeys() {
            self.hotkey_error = Some(e);
            return;
        }
        let raw = Box::into_raw(Box::new(self.cfg.clone()));
        if post(WM_APP_APPLY, 0, raw as isize) {
            self.sent = self.cfg.clone();
        } else {
            drop(unsafe { Box::from_raw(raw) });
        }
    }

    fn poll_binding(&mut self, ctx: &egui::Context) {
        let Some(b) = &mut self.binding else { return };
        ctx.request_repaint_after(Duration::from_millis(15));
        if key_down(VK_ESCAPE) {
            self.binding = None;
            return;
        }
        b.held.retain(|&vk| key_down(vk));
        let pressed = keys::all()
            .iter()
            .map(|k| k.vk)
            .filter(|&vk| vk != keys::NONE && vk != keys::VK_LBUTTON)
            .find(|&vk| key_down(vk) && !b.held.contains(&vk));
        if let Some(vk) = pressed {
            let slot = b.slot;
            self.binding = None;
            self.bind(slot, vk);
        }
    }

    fn bind(&mut self, slot: Slot, vk: u16) {
        if let Some(other) = Slot::ALL
            .into_iter()
            .find(|&s| s != slot && s.get(&self.cfg) == vk)
        {
            self.hotkey_error = Some(format!(
                "{} is already used for \"{}\".",
                keys::name_of(vk),
                other.label()
            ));
            return;
        }
        slot.set(&mut self.cfg, vk);
        self.hotkey_error = None;
    }

    fn choose_png(&mut self) {
        let mut file = [0u16; 1024];
        let filter: Vec<u16> = "PNG images (*.png)\0*.png\0All files\0*.*\0\0"
            .encode_utf16()
            .collect();
        let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
        ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = self.hwnd;
        ofn.lpstrFilter = filter.as_ptr();
        ofn.lpstrFile = file.as_mut_ptr();
        ofn.nMaxFile = file.len() as u32;
        ofn.Flags = OFN_PATHMUSTEXIST | OFN_FILEMUSTEXIST;
        if unsafe { GetOpenFileNameW(&mut ofn) } == 0 {
            return;
        }
        let path = win::from_wide(&file);
        match overlay::load_reticle(&path) {
            Ok(img) => {
                self.cfg.png_path = path.clone();
                self.cfg.reticle = true;
                self.png = Some((path, img));
                self.png_error = None;
            }
            Err(e) => self.png_error = Some(e),
        }
    }

    /// Keeps the decoded PNG in step with the configured path (for the preview).
    fn sync_png(&mut self) {
        let path = &self.cfg.png_path;
        if path.is_empty() {
            self.png = None;
            return;
        }
        if self.png.as_ref().is_some_and(|(p, _)| p == path) {
            return;
        }
        match overlay::load_reticle(path) {
            Ok(img) => self.png = Some((path.clone(), img)),
            Err(e) => {
                self.png_error = Some(e);
                self.png = None;
                self.cfg.png_path.clear();
            }
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let icon = &self.icons[if self.enabled { 0 } else { 1 }];
            ui.add(egui::Image::new((icon.id(), Vec2::splat(40.0))));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.heading("Precision Zoom");
                let trigger = keys::name_of(self.cfg.trigger_key);
                let status = if self.enabled {
                    format!("Hold {trigger} to zoom")
                } else if self.cfg.toggle_modifier != keys::NONE {
                    format!(
                        "Disabled. Turn it on here or with {} + {trigger}",
                        keys::name_of(self.cfg.toggle_modifier)
                    )
                } else {
                    "Disabled. Turn it on here or from the tray icon".to_string()
                };
                ui.label(RichText::new(status).color(theme::MUTED));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut on = self.enabled;
                if toggle(ui, &mut on).changed() && post(WM_APP_SET_ENABLED, on as usize, 0) {
                    self.enabled = on;
                }
                let (text, color) = if self.enabled {
                    ("ON", theme::ACCENT)
                } else {
                    ("OFF", theme::MUTED)
                };
                ui.label(RichText::new(text).font(theme::semibold(15.0)).color(color));
            });
        });
    }

    fn notice_banner(&mut self, ui: &mut egui::Ui) {
        let Some(text) = self.notice.clone() else {
            return;
        };
        egui::Frame::new()
            .fill(Color32::from_rgb(0x3A, 0x2C, 0x12))
            .stroke(Stroke::new(1.0, Color32::from_rgb(0x6B, 0x4E, 0x1A)))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(text).color(theme::WARN));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Dismiss").clicked() {
                            self.notice = None;
                            if let Some(s) = shared().as_mut() {
                                s.notice = None;
                            }
                        }
                    });
                });
            });
    }

    fn magnifier_card(&mut self, ui: &mut egui::Ui) {
        let c = &mut self.cfg;
        card(ui, "Magnifier", |ui| {
            row(ui, "Zoom", Some("Hotkeys work too"), |ui| {
                ui.add(
                    egui::Slider::new(&mut c.zoom, *config::ZOOM.start()..=*config::ZOOM.end())
                        .logarithmic(true)
                        .fixed_decimals(2)
                        .suffix("×"),
                );
            });
            row(ui, "Size", None, |ui| {
                ui.add(egui::Slider::new(&mut c.size, config::SIZE).suffix(" px"));
            });
            row(ui, "Opacity", None, |ui| {
                ui.add(egui::Slider::new(&mut c.opacity_pct, config::OPACITY_PCT).suffix("%"));
            });
            row(ui, "Border", None, |ui| {
                ui.add(egui::Slider::new(&mut c.border_px, config::BORDER_PX).suffix(" px"));
                theme::color_button(ui, &mut c.border_color);
            });
            row(ui, "Circular scope", None, |ui| toggle(ui, &mut c.circular));
            row(
                ui,
                "Negative mode",
                Some("Inverts colors for contrast"),
                |ui| toggle(ui, &mut c.negative),
            );
        });
    }

    fn reticle_card(&mut self, ui: &mut egui::Ui) {
        let mut choose = false;
        card(ui, "Reticle", |ui| {
            let c = &mut self.cfg;
            row(ui, "Show reticle", None, |ui| toggle(ui, &mut c.reticle));
            ui.add_enabled_ui(c.reticle, |ui| {
                let mut style = if c.png_path.is_empty() {
                    ReticleStyle::Cross
                } else {
                    ReticleStyle::Image
                };
                row(ui, "Style", None, |ui| {
                    if theme::segmented(
                        ui,
                        &mut style,
                        &[
                            (ReticleStyle::Image, "Image"),
                            (ReticleStyle::Cross, "Cross"),
                        ],
                    ) {
                        match style {
                            ReticleStyle::Cross => c.png_path.clear(),
                            ReticleStyle::Image => choose = true,
                        }
                    }
                });
                if style == ReticleStyle::Image && !c.png_path.is_empty() {
                    let name = std::path::Path::new(&c.png_path)
                        .file_name()
                        .map_or(c.png_path.clone(), |n| n.to_string_lossy().into_owned());
                    row(ui, "Image", Some(&name), |ui| {
                        if ui.button("Choose…").on_hover_text(&c.png_path).clicked() {
                            choose = true;
                        }
                    });
                }
                if let Some(e) = &self.png_error {
                    ui.label(RichText::new(e).small().color(theme::WARN));
                }
                row(ui, "Size", None, |ui| {
                    ui.add(
                        egui::Slider::new(
                            &mut c.reticle_size,
                            *config::RETICLE_SIZE.start()..=c.size,
                        )
                        .suffix(" px"),
                    );
                });
                if style == ReticleStyle::Cross {
                    row(ui, "Line width", None, |ui| {
                        ui.add(
                            egui::Slider::new(&mut c.reticle_thickness, config::RETICLE_THICKNESS)
                                .suffix(" px"),
                        );
                        theme::color_button(ui, &mut c.reticle_color);
                    });
                }
                let max = c.size / 2;
                row(
                    ui,
                    "Offset",
                    Some("Nudge to line up with your game"),
                    |ui| {
                        if ui
                            .add_enabled(
                                c.offset_x != 0 || c.offset_y != 0,
                                egui::Button::new("Center"),
                            )
                            .clicked()
                        {
                            c.offset_x = 0;
                            c.offset_y = 0;
                        }
                        ui.add(
                            egui::DragValue::new(&mut c.offset_y)
                                .range(-max..=max)
                                .prefix("Y "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut c.offset_x)
                                .range(-max..=max)
                                .prefix("X "),
                        );
                    },
                );
            });
        });
        if choose {
            // Modal: blocks only this window's thread; zooming keeps working.
            self.choose_png();
        }
    }

    fn preview_card(&mut self, ui: &mut egui::Ui) {
        card(ui, "Preview", |ui| {
            let side = ui.available_width().min(300.0);
            let px = (side * ui.ctx().pixels_per_point()).round() as u32;
            let stale = self
                .preview
                .as_ref()
                .is_none_or(|(c, p, _)| *c != self.cfg || *p != px);
            if stale {
                let pm = preview::render(&self.cfg, self.png.as_ref().map(|(_, img)| img), px);
                let image =
                    ColorImage::from_rgba_premultiplied([px as usize, px as usize], pm.data());
                match &mut self.preview {
                    Some((c, p, tex)) => {
                        tex.set(image, TextureOptions::LINEAR);
                        *c = self.cfg.clone();
                        *p = px;
                    }
                    None => {
                        let tex = ui
                            .ctx()
                            .load_texture("preview", image, TextureOptions::LINEAR);
                        self.preview = Some((self.cfg.clone(), px, tex));
                    }
                }
            }
            if let Some((_, _, tex)) = &self.preview {
                ui.vertical_centered(|ui| {
                    ui.add(egui::Image::new((tex.id(), Vec2::splat(side))));
                    ui.label(
                        RichText::new(format!(
                            "{:.2}× · {} px · sample scene",
                            self.cfg.zoom, self.cfg.size
                        ))
                        .small()
                        .color(theme::MUTED),
                    );
                });
            }
        });
    }

    fn hotkeys_card(&mut self, ui: &mut egui::Ui) {
        card(ui, "Hotkeys", |ui| {
            for slot in Slot::ALL {
                let vk = slot.get(&self.cfg);
                let binding = self.binding.as_ref().is_some_and(|b| b.slot == slot);
                row(ui, slot.label(), Some(slot.hint()), |ui| {
                    if slot != Slot::Trigger
                        && vk != keys::NONE
                        && ui.small_button("×").on_hover_text("Unbind").clicked()
                    {
                        slot.set(&mut self.cfg, keys::NONE);
                    }
                    let text = if binding {
                        RichText::new("Press a key…").color(Color32::WHITE)
                    } else if vk == keys::NONE {
                        RichText::new("Not set").color(theme::MUTED)
                    } else {
                        RichText::new(keys::name_of(vk))
                    };
                    let button = egui::Button::new(text)
                        .min_size(Vec2::new(150.0, 28.0))
                        .fill(if binding {
                            theme::ACCENT_DIM
                        } else {
                            theme::WIDGET
                        })
                        .stroke(if binding {
                            Stroke::new(1.0, theme::ACCENT)
                        } else {
                            Stroke::NONE
                        });
                    let response = ui
                        .add(button)
                        .on_hover_text("Click, then press a key or mouse button");
                    if response.clicked() {
                        // Keep Space/Enter from re-clicking the button while binding.
                        response.surrender_focus();
                        self.binding = if binding {
                            None
                        } else {
                            let held = keys::all()
                                .iter()
                                .map(|k| k.vk)
                                .filter(|&v| key_down(v))
                                .collect();
                            Some(Binding { slot, held })
                        };
                        self.hotkey_error = None;
                    }
                });
            }
            match (&self.hotkey_error, self.binding.is_some()) {
                (Some(e), _) => {
                    ui.label(RichText::new(e).small().color(theme::WARN));
                }
                (None, true) => {
                    ui.label(
                        RichText::new("Press Esc to cancel.")
                            .small()
                            .color(theme::MUTED),
                    );
                }
                _ => {}
            }
        });
    }

    fn behavior_card(&mut self, ui: &mut egui::Ui) {
        let c = &mut self.cfg;
        card(ui, "Behavior", |ui| {
            row(ui, "Beep on toggle", None, |ui| {
                toggle(ui, &mut c.beep_on_toggle)
            });
            row(
                ui,
                "Hide from screen capture",
                Some("Keeps the overlay out of screenshots"),
                |ui| toggle(ui, &mut c.hide_from_capture),
            );
            if c.hide_from_capture {
                ui.label(
                    RichText::new(
                        "Some anti-cheat systems treat windows hidden from capture as suspicious.",
                    )
                    .small()
                    .color(theme::WARN),
                );
            }
            row(ui, "Update interval", Some("Lower is smoother"), |ui| {
                ui.add(egui::Slider::new(&mut c.refresh_ms, config::REFRESH_MS).suffix(" ms"));
            });
        });
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Changes apply instantly and are saved automatically.")
                    .small()
                    .color(theme::MUTED),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                        .small()
                        .color(theme::MUTED),
                );
                let armed = self
                    .reset_armed
                    .is_some_and(|t| t.elapsed() < RESET_CONFIRM);
                let button = if armed {
                    egui::Button::new(RichText::new("Click again to reset").color(Color32::WHITE))
                        .fill(theme::ACCENT)
                } else {
                    egui::Button::new("Reset to defaults")
                };
                if ui.add(button).clicked() {
                    if armed {
                        self.cfg = Config::default();
                        self.reset_armed = None;
                        self.hotkey_error = None;
                        self.png_error = None;
                    } else {
                        self.reset_armed = Some(Instant::now());
                        ui.ctx().request_repaint_after(RESET_CONFIRM);
                    }
                }
            });
        });
    }
}

impl eframe::App for SettingsApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if SHOW_REQUESTED.swap(false, Ordering::AcqRel) {
            self.sync_from_app(true);
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        } else {
            self.sync_from_app(false);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) {
            // Closing only hides: the event loop can't be recreated later.
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
            self.binding = None;
        }
        self.poll_binding(&ctx);
        self.sync_png();

        egui::Panel::bottom("footer")
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(Margin::symmetric(22, 10)),
            )
            .show(ui, |ui| self.footer(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(Margin {
                left: 22,
                right: 22,
                top: 18,
                bottom: 0,
            }))
            .show(ui, |ui| {
                self.header(ui);
                ui.add_space(4.0);
                self.notice_banner(ui);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.columns(2, |cols| {
                            const GAP: f32 = 6.0;
                            self.magnifier_card(&mut cols[0]);
                            cols[0].add_space(GAP);
                            self.reticle_card(&mut cols[0]);
                            cols[0].add_space(GAP);
                            self.behavior_card(&mut cols[0]);
                            self.preview_card(&mut cols[1]);
                            cols[1].add_space(GAP);
                            self.hotkeys_card(&mut cols[1]);
                        });
                    });
            });

        self.push_changes();
    }
}
