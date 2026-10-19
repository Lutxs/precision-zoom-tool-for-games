//! Generates the app icons and embeds them, the manifest and version info as Windows resources.

use std::fmt::Write as _;
use std::path::Path;

#[path = "src/icon_art.rs"]
mod icon_art;

use icon_art::{ACCENT_OFF, ACCENT_ON, render};

const SIZES: &[u32] = &[16, 20, 24, 32, 40, 48, 64, 256];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=res/app.manifest");
    println!("cargo:rerun-if-changed=src/icon_art.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let icon_on = out.join("app.ico");
    let icon_off = out.join("app_off.ico");
    std::fs::write(&icon_on, ico(ACCENT_ON)).unwrap();
    std::fs::write(&icon_off, ico(ACCENT_OFF)).unwrap();
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("res")
        .join("app.manifest");

    let rc_path = out.join("app.rc");
    std::fs::write(&rc_path, rc(&icon_on, &icon_off, &manifest)).unwrap();
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_required()
        .unwrap();
}

fn rc_path(p: &Path) -> String {
    p.display().to_string().replace('\\', "\\\\")
}

fn rc(icon_on: &Path, icon_off: &Path, manifest: &Path) -> String {
    let v = |k: &str| std::env::var(k).unwrap();
    let (major, minor, patch) = (
        v("CARGO_PKG_VERSION_MAJOR"),
        v("CARGO_PKG_VERSION_MINOR"),
        v("CARGO_PKG_VERSION_PATCH"),
    );
    let version = v("CARGO_PKG_VERSION");
    let mut s = String::new();
    writeln!(s, "1 ICON \"{}\"", rc_path(icon_on)).unwrap();
    writeln!(s, "2 ICON \"{}\"", rc_path(icon_off)).unwrap();
    writeln!(s, "1 24 \"{}\"", rc_path(manifest)).unwrap();
    write!(
        s,
        r#"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "Lutxs"
      VALUE "FileDescription", "Precision Zoom Tool"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "precision-zoom"
      VALUE "LegalCopyright", "Copyright (c) 2026 Lutxs. MIT License."
      VALUE "OriginalFilename", "precision-zoom.exe"
      VALUE "ProductName", "Precision Zoom Tool"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    )
    .unwrap();
    s
}

/// Builds a multi-resolution .ico with 32-bit BMP images.
fn ico(accent: [u8; 3]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = SIZES.iter().map(|&s| bmp(s, &render(s, accent))).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len() as u32;
    for (&s, img) in SIZES.iter().zip(&images) {
        let dim = if s >= 256 { 0 } else { s as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bit count
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        out.extend_from_slice(&img);
    }
    out
}

fn bmp(size: u32, px: &[[u8; 4]]) -> Vec<u8> {
    let mask_row = size.div_ceil(32) * 4;
    let mut out = Vec::new();
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(size as i32).to_le_bytes());
    out.extend_from_slice(&(size as i32 * 2).to_le_bytes()); // color + mask
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(size * size * 4 + mask_row * size).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]);
    for y in (0..size).rev() {
        for x in 0..size {
            let [r, g, b, a] = px[(y * size + x) as usize];
            out.extend_from_slice(&[b, g, r, a]);
        }
    }
    // AND mask unused with 32-bit alpha, but must be present.
    out.resize(out.len() + (mask_row * size) as usize, 0);
    out
}
