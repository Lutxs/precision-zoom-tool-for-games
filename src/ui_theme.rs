//! Visual design of the settings window: palette, fonts and custom widgets.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Response,
    RichText, Role, Sense, Stroke, StrokeKind, TextStyle, Theme, Ui, Vec2, WidgetInfo,
};

pub const BG: Color32 = Color32::from_rgb(0x0F, 0x11, 0x15);
pub const CARD: Color32 = Color32::from_rgb(0x17, 0x1A, 0x21);
pub const CARD_STROKE: Color32 = Color32::from_rgb(0x25, 0x29, 0x33);
pub const WIDGET: Color32 = Color32::from_rgb(0x24, 0x28, 0x32);
pub const WIDGET_HOVER: Color32 = Color32::from_rgb(0x2F, 0x34, 0x41);
pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEE);
pub const MUTED: Color32 = Color32::from_rgb(0x8B, 0x93, 0xA7);
pub const ACCENT: Color32 = Color32::from_rgb(0xEF, 0x44, 0x44);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x5C, 0x23, 0x27);
pub const WARN: Color32 = Color32::from_rgb(0xF5, 0xB0, 0x41);

const SEMIBOLD: &str = "semibold";

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// Installs Segoe UI (from the Windows fonts folder, falling back to egui's
/// built-in font) and the dark palette.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let load = |file: &str| std::fs::read(format!(r"{windir}\Fonts\{file}")).ok();
    let mut proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    if let Some(bytes) = load("segoeui.ttf") {
        fonts
            .font_data
            .insert("segoe".into(), Arc::new(FontData::from_owned(bytes)));
        proportional.insert(0, "segoe".into());
    }
    let mut strong = proportional.clone();
    if let Some(bytes) = load("seguisb.ttf") {
        fonts.font_data.insert(
            "segoe-semibold".into(),
            Arc::new(FontData::from_owned(bytes)),
        );
        strong.insert(0, "segoe-semibold".into());
    }
    fonts
        .families
        .insert(FontFamily::Proportional, proportional);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), strong);
    ctx.set_fonts(fonts);

    ctx.set_theme(Theme::Dark);
    let mut v = egui::Visuals::dark();
    let radius = CornerRadius::same(7);
    v.panel_fill = BG;
    v.window_fill = CARD;
    v.extreme_bg_color = Color32::from_rgb(0x0B, 0x0D, 0x11);
    v.faint_bg_color = CARD;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = WARN;
    v.slider_trailing_fill = true;
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    for (w, fill) in [
        (&mut v.widgets.noninteractive, CARD),
        (&mut v.widgets.inactive, WIDGET),
        (&mut v.widgets.hovered, WIDGET_HOVER),
        (&mut v.widgets.active, WIDGET_HOVER),
        (&mut v.widgets.open, WIDGET_HOVER),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.corner_radius = radius;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, CARD_STROKE);
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x3A, 0x40, 0x4F));
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.inactive.fg_stroke = Stroke::new(1.5, TEXT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    v.widgets.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    v.widgets.hovered.expansion = 0.0;
    v.widgets.active.expansion = 0.0;
    ctx.set_visuals_of(Theme::Dark, v);

    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = Vec2::new(10.0, 8.0);
        s.spacing.button_padding = Vec2::new(12.0, 5.0);
        s.spacing.interact_size = Vec2::new(40.0, 28.0);
        s.spacing.slider_width = 150.0;
        s.spacing.slider_rail_height = 5.0;
        s.text_styles
            .insert(TextStyle::Body, FontId::proportional(14.5));
        s.text_styles
            .insert(TextStyle::Button, FontId::proportional(14.0));
        s.text_styles
            .insert(TextStyle::Small, FontId::proportional(12.5));
        s.text_styles.insert(TextStyle::Heading, semibold(21.0));
    });
}

/// A rounded panel with a title.
pub fn card<R>(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, CARD_STROKE))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(title.to_uppercase())
                    .font(semibold(12.0))
                    .color(MUTED)
                    .extra_letter_spacing(1.2),
            );
            ui.add_space(2.0);
            add_contents(ui)
        })
        .inner
}

/// A labelled row: label (with optional hint below) on the left, controls right-aligned.
pub fn row<R>(
    ui: &mut Ui,
    label: &str,
    hint: Option<&str>,
    controls: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(label);
            if let Some(h) = hint {
                ui.label(RichText::new(h).small().color(MUTED));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), controls)
            .inner
    })
    .inner
}

/// iOS-style switch.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = Vec2::new(40.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| WidgetInfo::selected(Role::Switch, ui.is_enabled(), *on, ""));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let off_fill = if response.hovered() {
            WIDGET_HOVER
        } else {
            WIDGET
        };
        let fill = lerp_color(off_fill, ACCENT, t);
        let radius = rect.height() / 2.0;
        ui.painter()
            .rect(rect, radius, fill, Stroke::NONE, StrokeKind::Inside);
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
        let knob = egui::pos2(x, rect.center().y);
        ui.painter()
            .circle_filled(knob, radius - 3.5, Color32::WHITE);
    }
    response
}

/// Pill-shaped segmented choice; returns true when the selection changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .fill(WIDGET)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for &(opt, label) in options {
                    let selected = *value == opt;
                    let text =
                        RichText::new(label).color(if selected { Color32::WHITE } else { MUTED });
                    let button = egui::Button::new(text)
                        .fill(if selected {
                            ACCENT
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(6));
                    if ui.add(button).clicked() && !selected {
                        *value = opt;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// Color swatch button that opens egui's color picker.
pub fn color_button(ui: &mut Ui, colorref: &mut u32) -> Response {
    let [r, g, b, _] = colorref.to_le_bytes();
    let mut rgb = [r, g, b];
    let response = ui.color_edit_button_srgb(&mut rgb);
    if response.changed() {
        *colorref = crate::config::rgb(rgb[0], rgb[1], rgb[2]);
    }
    response
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}
