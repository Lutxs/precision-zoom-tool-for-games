//! User settings: defaults, validation and per-value registry storage.
//!
//! Each setting is its own registry value, so adding or removing a field never
//! corrupts settings saved by another version (the C++ tool stored the whole
//! struct as one binary blob). Missing or out-of-range values fall back to
//! their defaults. Settings from the C++ version are migrated on first load.

use std::fmt::Display;
use std::ops::RangeInclusive;
use std::str::FromStr;

use crate::keys;
use crate::win::wide;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, REG_DWORD, REG_ROUTINE_FLAGS, REG_SZ, RRF_RT_REG_BINARY,
    RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};

pub const REG_KEY: &str = r"Software\PrecisionZoomTool";
const CONFIG_VERSION: u32 = 2;

pub const ZOOM: RangeInclusive<f32> = 1.1..=16.0;
pub const SIZE: RangeInclusive<i32> = 64..=1600;
pub const REFRESH_MS: RangeInclusive<u32> = 1..=100;
pub const OPACITY_PCT: RangeInclusive<u32> = 10..=100;
pub const BORDER_PX: RangeInclusive<i32> = 0..=50;
pub const RETICLE_SIZE: RangeInclusive<i32> = 4..=1600;
pub const RETICLE_THICKNESS: RangeInclusive<i32> = 1..=25;

/// Colors are Win32 `COLORREF`s (`0x00BBGGRR`).
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub zoom: f32,
    pub size: i32,
    pub refresh_ms: u32,
    pub opacity_pct: u32,
    pub border_px: i32,
    pub border_color: u32,
    pub circular: bool,
    pub negative: bool,
    pub reticle: bool,
    pub reticle_size: i32,
    pub reticle_thickness: i32,
    pub reticle_color: u32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub png_path: String,
    pub trigger_key: u16,
    pub toggle_modifier: u16,
    pub zoom_in_key: u16,
    pub zoom_out_key: u16,
    pub hide_from_capture: bool,
    pub beep_on_toggle: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            zoom: 2.0,
            size: 350,
            refresh_ms: 10,
            opacity_pct: 100,
            border_px: 2,
            border_color: rgb(0, 0, 0),
            circular: false,
            negative: false,
            reticle: false,
            reticle_size: 32,
            reticle_thickness: 2,
            reticle_color: rgb(255, 0, 0),
            offset_x: 0,
            offset_y: 0,
            png_path: String::new(),
            trigger_key: keys::VK_RBUTTON,
            toggle_modifier: keys::VK_LSHIFT,
            zoom_in_key: keys::VK_OEM_PLUS,
            zoom_out_key: keys::VK_OEM_MINUS,
            hide_from_capture: false,
            beep_on_toggle: true,
        }
    }
}

/// Clamps `v` into `range`.
fn clamp<T: PartialOrd + Copy>(v: T, range: &RangeInclusive<T>) -> T {
    if v < *range.start() {
        *range.start()
    } else if v > *range.end() {
        *range.end()
    } else {
        v
    }
}

/// Rounds a zoom factor to two decimals and clamps it to [`ZOOM`]. NaN becomes the default.
pub fn clamp_zoom(z: f32) -> f32 {
    if !z.is_finite() {
        return Config::default().zoom;
    }
    clamp((z * 100.0).round() / 100.0, &ZOOM)
}

impl Config {
    /// Forces every field into a usable state, whatever its origin.
    pub fn sanitize(&mut self) {
        let d = Config::default();
        self.zoom = clamp_zoom(self.zoom);
        self.size = clamp(self.size, &SIZE);
        self.refresh_ms = clamp(self.refresh_ms, &REFRESH_MS);
        self.opacity_pct = clamp(self.opacity_pct, &OPACITY_PCT);
        self.border_px = clamp(self.border_px, &BORDER_PX);
        self.border_color &= 0x00FF_FFFF;
        self.reticle_size = clamp(self.reticle_size, &RETICLE_SIZE);
        self.reticle_thickness = clamp(self.reticle_thickness, &RETICLE_THICKNESS);
        self.reticle_color &= 0x00FF_FFFF;
        let max_offset = self.size / 2;
        self.offset_x = self.offset_x.clamp(-max_offset, max_offset);
        self.offset_y = self.offset_y.clamp(-max_offset, max_offset);

        for (key, default) in [
            (&mut self.trigger_key, d.trigger_key),
            (&mut self.toggle_modifier, d.toggle_modifier),
            (&mut self.zoom_in_key, d.zoom_in_key),
            (&mut self.zoom_out_key, d.zoom_out_key),
        ] {
            if !keys::is_known(*key) {
                *key = default;
            }
        }
        if self.trigger_key == keys::NONE {
            self.trigger_key = d.trigger_key;
        }
        // A modifier equal to the trigger would make zooming impossible.
        if self.toggle_modifier == self.trigger_key {
            self.toggle_modifier = keys::NONE;
        }
        if self.zoom_in_key != keys::NONE && self.zoom_in_key == self.zoom_out_key {
            self.zoom_out_key = keys::NONE;
        }
    }

    /// Rejects hotkey combinations that can't work, with a message for the settings window.
    pub fn check_hotkeys(&self) -> Result<(), String> {
        if self.trigger_key == keys::NONE {
            return Err("Pick a key for \"Hold to zoom\".".into());
        }
        let mut bound = vec![("Hold to zoom", self.trigger_key)];
        for (name, vk) in [
            ("Toggle modifier", self.toggle_modifier),
            ("Zoom in", self.zoom_in_key),
            ("Zoom out", self.zoom_out_key),
        ] {
            if vk == keys::NONE {
                continue;
            }
            if let Some((other, _)) = bound.iter().find(|(_, b)| *b == vk) {
                return Err(format!(
                    "\"{name}\" and \"{other}\" can't use the same key ({}).",
                    keys::name_of(vk)
                ));
            }
            bound.push((name, vk));
        }
        Ok(())
    }

    pub fn load() -> Self {
        Self::load_from(REG_KEY)
    }

    pub fn save(&self) {
        self.save_to(REG_KEY);
    }

    pub fn load_from(subkey: &str) -> Self {
        let reg = Reg::new(subkey);
        if reg.get_dword("Version").is_none()
            && let Some(mut c) = reg
                .get_binary(LEGACY_VALUE)
                .and_then(|blob| from_legacy_blob(&blob, ansi_to_string))
        {
            c.sanitize();
            c.save_to(subkey);
            return c;
        }
        let d = Config::default();
        let dword = |name, default: u32| reg.get_dword(name).unwrap_or(default);
        let int = |name, default: i32| reg.get_dword(name).map_or(default, |v| v as i32);
        let flag = |name, default: bool| reg.get_dword(name).map_or(default, |v| v != 0);
        let vk = |name, default: u16| reg.get_dword(name).map_or(default, |v| v as u16);

        let mut c = Config {
            zoom: reg
                .get_dword("ZoomX100")
                .map_or(d.zoom, |v| v as f32 / 100.0),
            size: int("Size", d.size),
            refresh_ms: dword("RefreshMs", d.refresh_ms),
            opacity_pct: dword("OpacityPercent", d.opacity_pct),
            border_px: int("BorderPx", d.border_px),
            border_color: dword("BorderColor", d.border_color),
            circular: flag("Circular", d.circular),
            negative: flag("Negative", d.negative),
            reticle: flag("Reticle", d.reticle),
            reticle_size: int("ReticleSize", d.reticle_size),
            reticle_thickness: int("ReticleThickness", d.reticle_thickness),
            reticle_color: dword("ReticleColor", d.reticle_color),
            offset_x: int("OffsetX", d.offset_x),
            offset_y: int("OffsetY", d.offset_y),
            png_path: reg.get_string("PngPath").unwrap_or_default(),
            trigger_key: vk("TriggerKey", d.trigger_key),
            toggle_modifier: vk("ToggleModifier", d.toggle_modifier),
            zoom_in_key: vk("ZoomInKey", d.zoom_in_key),
            zoom_out_key: vk("ZoomOutKey", d.zoom_out_key),
            hide_from_capture: flag("HideFromCapture", d.hide_from_capture),
            beep_on_toggle: flag("BeepOnToggle", d.beep_on_toggle),
        };
        c.sanitize();
        c
    }

    pub fn save_to(&self, subkey: &str) {
        let reg = Reg::new(subkey);
        reg.set_dword("Version", CONFIG_VERSION);
        reg.set_dword("ZoomX100", (self.zoom * 100.0).round() as u32);
        reg.set_dword("Size", self.size as u32);
        reg.set_dword("RefreshMs", self.refresh_ms);
        reg.set_dword("OpacityPercent", self.opacity_pct);
        reg.set_dword("BorderPx", self.border_px as u32);
        reg.set_dword("BorderColor", self.border_color);
        reg.set_dword("Circular", self.circular as u32);
        reg.set_dword("Negative", self.negative as u32);
        reg.set_dword("Reticle", self.reticle as u32);
        reg.set_dword("ReticleSize", self.reticle_size as u32);
        reg.set_dword("ReticleThickness", self.reticle_thickness as u32);
        reg.set_dword("ReticleColor", self.reticle_color);
        reg.set_dword("OffsetX", self.offset_x as u32);
        reg.set_dword("OffsetY", self.offset_y as u32);
        reg.set_string("PngPath", &self.png_path);
        reg.set_dword("TriggerKey", self.trigger_key as u32);
        reg.set_dword("ToggleModifier", self.toggle_modifier as u32);
        reg.set_dword("ZoomInKey", self.zoom_in_key as u32);
        reg.set_dword("ZoomOutKey", self.zoom_out_key as u32);
        reg.set_dword("HideFromCapture", self.hide_from_capture as u32);
        reg.set_dword("BeepOnToggle", self.beep_on_toggle as u32);
        // Migrated by `load_from`; keeping it would only shadow future changes.
        reg.delete(LEGACY_VALUE);
    }
}

/// Registry value holding the C++ version's settings struct as raw bytes.
const LEGACY_VALUE: &str = "Settings";
const LEGACY_LEN: usize = 320;

/// Converts the C++ version's `Config` struct (x64 MSVC layout) to the new settings.
fn from_legacy_blob(b: &[u8], decode_ansi: impl Fn(&[u8]) -> String) -> Option<Config> {
    if b.len() != LEGACY_LEN {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let i32_at = |o: usize| u32_at(o) as i32;
    let path = &b[28..28 + 260];
    let path = decode_ansi(&path[..path.iter().position(|&c| c == 0).unwrap_or(path.len())]);
    let d = Config::default();
    Some(Config {
        zoom: f32::from_bits(u32_at(0)),
        size: i32_at(4),
        refresh_ms: u32_at(12),
        // The C++ tool drew a loaded PNG even with the crosshair box unchecked.
        reticle: b[16] != 0 || !path.is_empty(),
        reticle_size: i32_at(20),
        reticle_color: u32_at(24),
        png_path: path,
        trigger_key: u32_at(288) as u16,
        toggle_modifier: u32_at(292) as u16,
        circular: b[296] != 0,
        negative: b[297] != 0,
        opacity_pct: (u32_at(300).min(255) * 100 + 127) / 255,
        offset_x: i32_at(304),
        offset_y: i32_at(308),
        border_px: i32_at(312),
        border_color: u32_at(316),
        ..d
    })
}

fn ansi_to_string(bytes: &[u8]) -> String {
    use windows_sys::Win32::Globalization::{CP_ACP, MultiByteToWideChar};
    if bytes.is_empty() {
        return String::new();
    }
    let mut buf = vec![0u16; bytes.len() * 2];
    let n = unsafe {
        MultiByteToWideChar(
            CP_ACP,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            buf.as_mut_ptr(),
            buf.len() as i32,
        )
    };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Parses a settings form field, returning a message naming the field and its valid range on error.
pub fn parse_field<T>(label: &str, text: &str, range: RangeInclusive<T>) -> Result<T, String>
where
    T: FromStr + PartialOrd + Display + Copy,
{
    let err = || {
        format!(
            "{label} must be a number from {} to {}.",
            range.start(),
            range.end()
        )
    };
    let v: T = text.trim().parse().map_err(|_| err())?;
    if range.contains(&v) {
        Ok(v)
    } else {
        Err(err())
    }
}

struct Reg {
    subkey: Vec<u16>,
}

impl Reg {
    fn new(subkey: &str) -> Self {
        Self {
            subkey: wide(subkey),
        }
    }

    fn root() -> HKEY {
        HKEY_CURRENT_USER
    }

    fn get_dword(&self, name: &str) -> Option<u32> {
        let name = wide(name);
        let mut value = 0u32;
        let mut len = 4u32;
        let rc = unsafe {
            RegGetValueW(
                Self::root(),
                self.subkey.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut value as *mut u32).cast(),
                &mut len,
            )
        };
        (rc == ERROR_SUCCESS).then_some(value)
    }

    fn get_bytes(&self, name: &str, flags: REG_ROUTINE_FLAGS) -> Option<Vec<u8>> {
        let name = wide(name);
        let mut len = 0u32;
        let get = |buf: *mut u8, len: &mut u32| unsafe {
            RegGetValueW(
                Self::root(),
                self.subkey.as_ptr(),
                name.as_ptr(),
                flags,
                std::ptr::null_mut(),
                buf.cast(),
                len,
            )
        };
        if get(std::ptr::null_mut(), &mut len) != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        if get(buf.as_mut_ptr(), &mut len) != ERROR_SUCCESS {
            return None;
        }
        buf.truncate(len as usize);
        Some(buf)
    }

    fn get_string(&self, name: &str) -> Option<String> {
        let bytes = self.get_bytes(name, RRF_RT_REG_SZ)?;
        let utf16: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        Some(crate::win::from_wide(&utf16))
    }

    fn get_binary(&self, name: &str) -> Option<Vec<u8>> {
        self.get_bytes(name, RRF_RT_REG_BINARY)
    }

    fn set_dword(&self, name: &str, value: u32) {
        let name = wide(name);
        unsafe {
            RegSetKeyValueW(
                Self::root(),
                self.subkey.as_ptr(),
                name.as_ptr(),
                REG_DWORD,
                (&value as *const u32).cast(),
                4,
            );
        }
    }

    fn set_string(&self, name: &str, value: &str) {
        let name = wide(name);
        let value = wide(value);
        unsafe {
            RegSetKeyValueW(
                Self::root(),
                self.subkey.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            );
        }
    }

    fn delete(&self, name: &str) {
        let name = wide(name);
        unsafe {
            RegDeleteKeyValueW(Self::root(), self.subkey.as_ptr(), name.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_already_sane() {
        let mut c = Config::default();
        c.sanitize();
        assert_eq!(c, Config::default());
    }

    #[test]
    fn sanitize_clamps_everything() {
        let mut c = Config {
            zoom: 0.0,
            size: 0,
            refresh_ms: 0,
            opacity_pct: 0,
            border_px: -3,
            reticle_size: 100_000,
            reticle_thickness: 0,
            offset_x: 9999,
            offset_y: -9999,
            trigger_key: keys::NONE,
            ..Config::default()
        };
        c.sanitize();
        assert_eq!(c.zoom, 1.1);
        assert_eq!(c.size, 64);
        assert_eq!(c.refresh_ms, 1);
        assert_eq!(c.opacity_pct, 10);
        assert_eq!(c.border_px, 0);
        assert_eq!(c.reticle_size, 1600);
        assert_eq!(c.reticle_thickness, 1);
        assert_eq!((c.offset_x, c.offset_y), (32, -32));
        assert_eq!(c.trigger_key, keys::VK_RBUTTON);
    }

    #[test]
    fn sanitize_handles_nan_zoom_and_bad_keys() {
        let mut c = Config {
            zoom: f32::NAN,
            trigger_key: 0xFF,
            ..Config::default()
        };
        c.sanitize();
        assert_eq!(c.zoom, 2.0);
        assert_eq!(c.trigger_key, keys::VK_RBUTTON);
    }

    #[test]
    fn sanitize_resolves_key_conflicts() {
        let mut c = Config {
            toggle_modifier: keys::VK_RBUTTON,
            zoom_out_key: keys::VK_OEM_PLUS,
            ..Config::default()
        };
        c.sanitize();
        assert_eq!(c.toggle_modifier, keys::NONE);
        assert_eq!(c.zoom_out_key, keys::NONE);
    }

    #[test]
    fn hotkey_conflicts_are_rejected() {
        assert!(Config::default().check_hotkeys().is_ok());
        let none_trigger = Config {
            trigger_key: keys::NONE,
            ..Config::default()
        };
        assert!(none_trigger.check_hotkeys().is_err());
        let clash = Config {
            zoom_in_key: keys::VK_RBUTTON,
            ..Config::default()
        };
        let msg = clash.check_hotkeys().unwrap_err();
        assert!(
            msg.contains("Zoom in") && msg.contains("Hold to zoom"),
            "{msg}"
        );
        let unbound = Config {
            toggle_modifier: keys::NONE,
            zoom_out_key: keys::NONE,
            ..Config::default()
        };
        assert!(unbound.check_hotkeys().is_ok());
    }

    #[test]
    fn parse_field_validates() {
        assert_eq!(parse_field("Zoom", " 2.5 ", ZOOM), Ok(2.5));
        assert!(
            parse_field("Zoom", "0", ZOOM)
                .unwrap_err()
                .contains("1.1 to 16")
        );
        assert!(parse_field("Zoom", "", ZOOM).is_err());
        assert!(parse_field("Zoom", "abc", ZOOM).is_err());
        assert!(parse_field("Size", "-5", SIZE).is_err());
        assert_eq!(parse_field("Size", "400", SIZE), Ok(400));
    }

    #[test]
    fn registry_round_trip() {
        let subkey = format!(r"{REG_KEY}\UnitTest-{}", std::process::id());
        let c = Config {
            zoom: 3.25,
            size: 500,
            circular: true,
            offset_x: -7,
            png_path: r"C:\Users\Zoë\reticle 😀.png".into(),
            trigger_key: keys::VK_XBUTTON2,
            ..Config::default()
        };
        c.save_to(&subkey);
        let loaded = Config::load_from(&subkey);
        unsafe {
            let k = wide(&subkey);
            windows_sys::Win32::System::Registry::RegDeleteTreeW(HKEY_CURRENT_USER, k.as_ptr());
            windows_sys::Win32::System::Registry::RegDeleteKeyW(HKEY_CURRENT_USER, k.as_ptr());
        }
        assert_eq!(loaded, c);
    }

    #[test]
    fn missing_key_loads_defaults() {
        assert_eq!(
            Config::load_from(r"Software\PrecisionZoomTool\DoesNotExist-xyz"),
            Config::default()
        );
    }

    /// A real settings blob saved by the C++ version.
    fn legacy_blob() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&2.0f32.to_le_bytes());
        for v in [350u32, 350, 10, 1, 150, 0xFF] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let mut path = br"C:\Users\Luis\Downloads\image-51193743.png".to_vec();
        path.resize(260, 0);
        b.extend_from_slice(&path);
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&0xA0u32.to_le_bytes());
        b.extend_from_slice(&[1, 0, 0, 0]);
        for v in [255u32, 0, 0, 2, 0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }

    #[test]
    fn legacy_blob_is_migrated() {
        let ascii = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        let c = from_legacy_blob(&legacy_blob(), ascii).unwrap();
        assert_eq!(c.zoom, 2.0);
        assert_eq!(c.size, 350);
        assert_eq!(c.refresh_ms, 10);
        assert!(c.reticle && c.circular && !c.negative);
        assert_eq!(c.reticle_size, 150);
        assert_eq!(c.reticle_color, rgb(255, 0, 0));
        assert_eq!(c.png_path, r"C:\Users\Luis\Downloads\image-51193743.png");
        assert_eq!(
            (c.trigger_key, c.toggle_modifier),
            (keys::VK_RBUTTON, keys::VK_LSHIFT)
        );
        assert_eq!(c.opacity_pct, 100);
        assert_eq!((c.border_px, c.border_color), (2, 0));
        assert!(from_legacy_blob(&[0; 10], ascii).is_none());
    }

    #[test]
    fn legacy_registry_value_is_migrated_once() {
        let subkey = format!(r"{REG_KEY}\UnitTestLegacy-{}", std::process::id());
        let k = wide(&subkey);
        let name = wide(LEGACY_VALUE);
        let blob = legacy_blob();
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                k.as_ptr(),
                name.as_ptr(),
                windows_sys::Win32::System::Registry::REG_BINARY,
                blob.as_ptr().cast(),
                blob.len() as u32,
            );
        }
        let migrated = Config::load_from(&subkey);
        let reg = Reg::new(&subkey);
        let blob_gone = reg.get_binary(LEGACY_VALUE).is_none();
        let reloaded = Config::load_from(&subkey);
        unsafe {
            windows_sys::Win32::System::Registry::RegDeleteKeyW(HKEY_CURRENT_USER, k.as_ptr());
        }
        assert_eq!(migrated.size, 350);
        assert!(migrated.circular);
        assert!(blob_gone);
        assert_eq!(reloaded, migrated);
    }
}
