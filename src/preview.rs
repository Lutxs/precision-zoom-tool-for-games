//! Settings preview: a sample scene seen through the scope, composed the same
//! way as the real thing (magnified view over the unmagnified screen at the
//! chosen opacity, optional inversion, circular clip, then the real overlay).

use std::sync::OnceLock;

use tiny_skia::{
    Color, FillRule, FilterQuality, Mask, Paint, PathBuilder, Pattern, Pixmap, PixmapPaint, Rect,
    SpreadMode, Transform,
};

use crate::config::Config;
use crate::overlay;

const WORLD: u32 = 256;

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}

/// Sample "screen": sky, skyline, ground and a target at the cursor (center).
fn world() -> &'static Pixmap {
    static WORLD_PIXMAP: OnceLock<Pixmap> = OnceLock::new();
    WORLD_PIXMAP.get_or_init(|| {
        let mut pm = Pixmap::new(WORLD, WORLD).unwrap();
        let w = WORLD as f32;
        let horizon = w * 0.62;
        let fill = |pm: &mut Pixmap, x: f32, y: f32, ww: f32, hh: f32, c: Color| {
            let mut p = Paint::default();
            p.set_color(c);
            if let Some(r) = Rect::from_xywh(x, y, ww, hh) {
                pm.fill_rect(r, &p, Transform::identity(), None);
            }
        };
        // Sky gradient in bands (keeps the scene crisp when magnified).
        for i in 0..16 {
            let t = i as f32 / 15.0;
            let lerp = |a: f32, b: f32| (a + (b - a) * t) as u8;
            let band = horizon / 16.0;
            fill(
                &mut pm,
                0.0,
                i as f32 * band,
                w,
                band + 1.0,
                rgb(lerp(38.0, 92.0), lerp(56.0, 124.0), lerp(92.0, 168.0)),
            );
        }
        // Skyline.
        for (x, ww, hh, shade) in [
            (0.0, 34.0, 46.0, 30),
            (30.0, 22.0, 70.0, 36),
            (56.0, 40.0, 38.0, 28),
            (150.0, 30.0, 58.0, 34),
            (176.0, 44.0, 84.0, 30),
            (222.0, 34.0, 50.0, 38),
        ] {
            fill(
                &mut pm,
                x,
                horizon - hh,
                ww,
                hh,
                rgb(shade, shade + 6, shade + 16),
            );
            // Lit windows.
            for wy in (0..hh as i32 - 8).step_by(9) {
                for wx in (4..ww as i32 - 4).step_by(8) {
                    if (wx + wy + x as i32) % 3 == 0 {
                        fill(
                            &mut pm,
                            x + wx as f32,
                            horizon - hh + 5.0 + wy as f32,
                            3.0,
                            4.0,
                            rgb(240, 200, 120),
                        );
                    }
                }
            }
        }
        // Ground with perspective stripes.
        fill(&mut pm, 0.0, horizon, w, w - horizon, rgb(54, 66, 48));
        for i in 0..6 {
            let y = horizon + 4.0 + (i * i) as f32 * 3.2;
            fill(&mut pm, 0.0, y, w, 1.0 + i as f32 * 0.4, rgb(64, 78, 56));
        }
        // Target at the center: a plate on a post with rings.
        let c = w / 2.0;
        fill(
            &mut pm,
            c - 1.5,
            c + 12.0,
            3.0,
            horizon - c + 2.0,
            rgb(70, 52, 40),
        );
        let mut p = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        for (r, color) in [
            (14.0, rgb(235, 235, 235)),
            (10.5, rgb(200, 50, 50)),
            (7.0, rgb(235, 235, 235)),
            (3.5, rgb(200, 50, 50)),
        ] {
            p.set_color(color);
            if let Some(path) = PathBuilder::from_circle(c, c, r) {
                pm.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
            }
        }
        pm
    })
}

/// Draws the world so world pixel `WORLD / 2` lands at the preview center, scaled by `k`.
fn draw_world(dst: &mut Pixmap, k: f32) {
    let px = dst.width() as f32;
    let half = WORLD as f32 / 2.0;
    let t = Transform::from_row(k, 0.0, 0.0, k, px / 2.0 - half * k, px / 2.0 - half * k);
    let paint = Paint {
        shader: Pattern::new(
            world().as_ref(),
            SpreadMode::Pad,
            FilterQuality::Bilinear,
            1.0,
            t,
        ),
        ..Paint::default()
    };
    let r = Rect::from_xywh(0.0, 0.0, px, px).unwrap();
    dst.fill_rect(r, &paint, Transform::identity(), None);
}

/// Scales size-dependent settings so the overlay can be rendered at preview size.
fn scaled(cfg: &Config, px: u32) -> Config {
    let s = px as f32 / cfg.size as f32;
    let scale = |v: i32, min: i32| ((v as f32 * s).round() as i32).max(min);
    Config {
        size: px as i32,
        border_px: if cfg.border_px > 0 {
            scale(cfg.border_px, 1)
        } else {
            0
        },
        reticle_size: scale(cfg.reticle_size, 3),
        reticle_thickness: scale(cfg.reticle_thickness, 1),
        offset_x: (cfg.offset_x as f32 * s).round() as i32,
        offset_y: (cfg.offset_y as f32 * s).round() as i32,
        ..cfg.clone()
    }
}

/// Renders a `px`×`px` preview (premultiplied RGBA).
pub fn render(cfg: &Config, reticle_png: Option<&Pixmap>, px: u32) -> Pixmap {
    let px = px.max(16);
    // One screen pixel is `px / size` preview pixels.
    let base_scale = px as f32 / cfg.size as f32;
    let mut view = Pixmap::new(px, px).unwrap();
    draw_world(&mut view, base_scale);

    let mut magnified = Pixmap::new(px, px).unwrap();
    draw_world(&mut magnified, base_scale * cfg.zoom);
    if cfg.negative {
        for p in magnified.data_mut().as_chunks_mut::<4>().0 {
            // Opaque pixels, so premultiplied == straight: invert RGB.
            p[0] = p[3] - p[0];
            p[1] = p[3] - p[1];
            p[2] = p[3] - p[2];
        }
    }
    let opacity = cfg.opacity_pct as f32 / 100.0;
    view.draw_pixmap(
        0,
        0,
        magnified.as_ref(),
        &PixmapPaint {
            opacity,
            ..PixmapPaint::default()
        },
        Transform::identity(),
        None,
    );

    let mut out = Pixmap::new(px, px).unwrap();
    let mask = if cfg.circular {
        let mut m = Mask::new(px, px).unwrap();
        let r = px as f32 / 2.0;
        if let Some(path) = PathBuilder::from_circle(r, r, r) {
            m.fill_path(&path, FillRule::Winding, true, Transform::identity());
        }
        Some(m)
    } else {
        None
    };
    out.draw_pixmap(
        0,
        0,
        view.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        mask.as_ref(),
    );

    let overlay = overlay::render(&scaled(cfg, px), reticle_png);
    out.draw_pixmap(
        0,
        0,
        overlay.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(pm: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = pm.pixel(x, y).unwrap();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    #[test]
    fn circular_clips_corners_square_does_not() {
        let round = render(
            &Config {
                circular: true,
                ..Config::default()
            },
            None,
            100,
        );
        assert_eq!(rgba(&round, 1, 1)[3], 0);
        assert_eq!(rgba(&round, 50, 20)[3], 255);
        let square = render(&Config::default(), None, 100);
        assert_eq!(rgba(&square, 3, 3)[3], 255);
    }

    #[test]
    fn reticle_is_drawn_at_the_center() {
        let cfg = Config {
            reticle: true,
            reticle_color: crate::config::rgb(0, 255, 0),
            border_px: 0,
            ..Config::default()
        };
        let pm = render(&cfg, None, 120);
        assert_eq!(rgba(&pm, 60, 60), [0, 255, 0, 255]);
    }

    #[test]
    fn negative_mode_inverts_the_view() {
        let cfg = Config {
            border_px: 0,
            ..Config::default()
        };
        let normal = rgba(&render(&cfg, None, 100), 50, 10);
        let inverted = rgba(
            &render(
                &Config {
                    negative: true,
                    ..cfg
                },
                None,
                100,
            ),
            50,
            10,
        );
        for i in 0..3 {
            assert_eq!(inverted[i], 255 - normal[i]);
        }
    }

    #[test]
    fn zoom_magnifies_the_target() {
        // The target's white outer ring sits 14 px from the center. At 2x on a
        // 100 px preview of a 100 px scope it is 28 px out; at 4x it is past 50.
        let base = Config {
            size: 100,
            border_px: 0,
            ..Config::default()
        };
        let at2 = render(
            &Config {
                zoom: 2.0,
                ..base.clone()
            },
            None,
            100,
        );
        let at4 = render(&Config { zoom: 4.0, ..base }, None, 100);
        assert_ne!(rgba(&at2, 50, 50), rgba(&at2, 50, 85));
        assert_eq!(rgba(&at4, 50, 50)[..3], [200, 50, 50]);
    }
}
