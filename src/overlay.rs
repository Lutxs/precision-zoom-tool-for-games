//! Border and reticle overlay: rendered with tiny-skia into a per-pixel-alpha
//! layered window (no color-key, so PNG reticles keep their soft edges).

use tiny_skia::{
    Color, FillRule, FilterQuality, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke,
    Transform,
};
use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow};

use crate::config::Config;

pub fn load_reticle(path: &str) -> Result<Pixmap, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read {path}:\n{e}"))?;
    Pixmap::decode_png(&bytes).map_err(|e| format!("{path} is not a valid PNG image:\n{e}"))
}

fn color(colorref: u32) -> Color {
    let [r, g, b, _] = colorref.to_le_bytes();
    Color::from_rgba8(r, g, b, 255)
}

pub fn render(cfg: &Config, reticle_png: Option<&Pixmap>) -> Pixmap {
    let size = cfg.size as u32;
    let s = cfg.size as f32;
    let mut pm = Pixmap::new(size, size).expect("overlay size is clamped to a valid range");

    if cfg.border_px > 0 {
        let bw = cfg.border_px as f32;
        let mut paint = Paint::default();
        paint.set_color(color(cfg.border_color));
        let path = if cfg.circular {
            paint.anti_alias = true;
            PathBuilder::from_circle(s / 2.0, s / 2.0, (s - bw) / 2.0)
        } else {
            paint.anti_alias = false;
            Rect::from_xywh(bw / 2.0, bw / 2.0, s - bw, s - bw).map(PathBuilder::from_rect)
        };
        if let Some(path) = path {
            pm.stroke_path(
                &path,
                &paint,
                &Stroke {
                    width: bw,
                    ..Stroke::default()
                },
                Transform::identity(),
                None,
            );
        }
    }

    if cfg.reticle {
        // The cursor pixel sits at window pixel `size / 2`.
        let cx = cfg.size / 2 + cfg.offset_x;
        let cy = cfg.size / 2 + cfg.offset_y;
        match reticle_png {
            Some(img) => {
                let scale = cfg.reticle_size as f32 / img.width().max(img.height()) as f32;
                let w = img.width() as f32 * scale;
                let h = img.height() as f32 * scale;
                // Center the image on the middle of the cursor pixel.
                let tx = cx as f32 + 0.5 - w / 2.0;
                let ty = cy as f32 + 0.5 - h / 2.0;
                let paint = PixmapPaint {
                    quality: FilterQuality::Bicubic,
                    ..PixmapPaint::default()
                };
                pm.draw_pixmap(
                    0,
                    0,
                    img.as_ref(),
                    &paint,
                    Transform::from_row(scale, 0.0, 0.0, scale, tx, ty),
                    None,
                );
            }
            None => draw_cross(&mut pm, cfg, cx, cy),
        }
    }
    pm
}

/// Pixel-aligned cross, symmetric around the cursor pixel.
fn draw_cross(pm: &mut Pixmap, cfg: &Config, cx: i32, cy: i32) {
    let arm = cfg.reticle_size / 2;
    let t = cfg.reticle_thickness;
    let lo = -(t - 1) / 2;
    let mut paint = Paint::default();
    paint.set_color(color(cfg.reticle_color));
    paint.anti_alias = false;
    let mut pb = PathBuilder::new();
    let mut rect = |x: i32, y: i32, w: i32, h: i32| {
        if let Some(r) = Rect::from_xywh(x as f32, y as f32, w as f32, h as f32) {
            pb.push_rect(r);
        }
    };
    rect(cx - arm, cy + lo, 2 * arm + 1, t);
    rect(cx + lo, cy - arm, t, 2 * arm + 1);
    if let Some(path) = pb.finish() {
        // Winding fill, so the overlapping center isn't punched out.
        pm.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

/// Uploads `pm` as the layered window's content.
pub fn present(hwnd: HWND, pm: &Pixmap) {
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        let mem = CreateCompatibleDC(screen);
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut bits = std::ptr::null_mut();
        let bmp = CreateDIBSection(
            mem,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if !bmp.is_null() && !bits.is_null() {
            // tiny-skia is premultiplied RGBA; GDI wants premultiplied BGRA.
            let dst = std::slice::from_raw_parts_mut(bits.cast::<u8>(), pm.data().len());
            for (d, s) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(pm.data().as_chunks::<4>().0)
            {
                *d = [s[2], s[1], s[0], s[3]];
            }
            let old = SelectObject(mem, bmp);
            let size = SIZE { cx: w, cy: h };
            let origin = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            UpdateLayeredWindow(
                hwnd,
                screen,
                std::ptr::null(),
                &size,
                mem,
                &origin,
                0,
                &blend,
                ULW_ALPHA,
            );
            SelectObject(mem, old);
            DeleteObject(bmp);
        }
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_at(pm: &Pixmap, x: u32, y: u32) -> u8 {
        pm.pixel(x, y).unwrap().alpha()
    }

    #[test]
    fn cross_is_centered_on_cursor_pixel() {
        let cfg = Config {
            size: 101,
            reticle: true,
            reticle_size: 20,
            reticle_thickness: 1,
            border_px: 0,
            ..Config::default()
        };
        let pm = render(&cfg, None);
        let c = 50; // 101 / 2
        assert_eq!(alpha_at(&pm, c, c), 255);
        assert_eq!(alpha_at(&pm, c - 10, c), 255);
        assert_eq!(alpha_at(&pm, c + 10, c), 255);
        assert_eq!(alpha_at(&pm, c - 11, c), 0);
        assert_eq!(alpha_at(&pm, c + 11, c), 0);
        assert_eq!(alpha_at(&pm, c + 1, c + 1), 0);
        assert_eq!(alpha_at(&pm, c, c - 10), 255);
    }

    #[test]
    fn offsets_move_the_cross() {
        let cfg = Config {
            size: 100,
            reticle: true,
            offset_x: 5,
            offset_y: -3,
            border_px: 0,
            ..Config::default()
        };
        let pm = render(&cfg, None);
        assert_eq!(alpha_at(&pm, 55, 47), 255);
    }

    #[test]
    fn square_border_and_transparent_center() {
        let cfg = Config {
            size: 100,
            border_px: 3,
            ..Config::default()
        };
        let pm = render(&cfg, None);
        assert_eq!(alpha_at(&pm, 0, 50), 255);
        assert_eq!(alpha_at(&pm, 2, 50), 255);
        assert_eq!(alpha_at(&pm, 3, 50), 0);
        assert_eq!(alpha_at(&pm, 50, 50), 0, "no reticle requested");
    }

    #[test]
    fn png_reticle_is_scaled_to_reticle_size() {
        let mut img = Pixmap::new(10, 10).unwrap();
        img.fill(Color::from_rgba8(0, 255, 0, 255));
        let cfg = Config {
            size: 100,
            reticle: true,
            reticle_size: 40,
            border_px: 0,
            ..Config::default()
        };
        let pm = render(&cfg, Some(&img));
        assert_eq!(alpha_at(&pm, 50, 50), 255);
        assert_eq!(alpha_at(&pm, 32, 50), 255);
        assert_eq!(alpha_at(&pm, 25, 50), 0);
    }
}
