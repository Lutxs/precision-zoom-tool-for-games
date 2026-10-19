//! Virtual keys that can be bound to the tool's hotkeys.

use std::sync::OnceLock;

/// `vk` value meaning "no key bound".
pub const NONE: u16 = 0;

pub const VK_LBUTTON: u16 = 0x01;
pub const VK_RBUTTON: u16 = 0x02;
pub const VK_MBUTTON: u16 = 0x04;
pub const VK_XBUTTON1: u16 = 0x05;
pub const VK_XBUTTON2: u16 = 0x06;
pub const VK_LSHIFT: u16 = 0xA0;
pub const VK_OEM_PLUS: u16 = 0xBB;
pub const VK_OEM_MINUS: u16 = 0xBD;

pub struct Key {
    pub vk: u16,
    pub name: String,
}

/// Every bindable key, in the order shown in the settings combo boxes.
pub fn all() -> &'static [Key] {
    static KEYS: OnceLock<Vec<Key>> = OnceLock::new();
    KEYS.get_or_init(|| {
        let mut keys: Vec<Key> = [
            (NONE, "(none)"),
            (VK_RBUTTON, "Right mouse button"),
            (VK_MBUTTON, "Middle mouse button"),
            (VK_XBUTTON1, "Mouse button 4 (Back)"),
            (VK_XBUTTON2, "Mouse button 5 (Forward)"),
            (VK_LBUTTON, "Left mouse button"),
            (VK_LSHIFT, "Left Shift"),
            (0xA1, "Right Shift"),
            (0xA2, "Left Ctrl"),
            (0xA3, "Right Ctrl"),
            (0xA4, "Left Alt"),
            (0xA5, "Right Alt"),
            (0x14, "Caps Lock"),
            (0x09, "Tab"),
            (0x20, "Space"),
            (VK_OEM_PLUS, "= / +"),
            (VK_OEM_MINUS, "- / _"),
            (0x6B, "Numpad +"),
            (0x6D, "Numpad -"),
            (0x21, "Page Up"),
            (0x22, "Page Down"),
            (0x24, "Home"),
            (0x23, "End"),
            (0x2D, "Insert"),
            (0x2E, "Delete"),
            (0x26, "Up arrow"),
            (0x28, "Down arrow"),
            (0x25, "Left arrow"),
            (0x27, "Right arrow"),
        ]
        .into_iter()
        .map(|(vk, name)| Key {
            vk,
            name: name.to_string(),
        })
        .collect();
        keys.extend((1..=12u16).map(|n| Key {
            vk: 0x6F + n,
            name: format!("F{n}"),
        }));
        keys.extend((b'0'..=b'9').map(|c| Key {
            vk: c as u16,
            name: (c as char).to_string(),
        }));
        keys.extend((b'A'..=b'Z').map(|c| Key {
            vk: c as u16,
            name: (c as char).to_string(),
        }));
        keys
    })
}

pub fn is_known(vk: u16) -> bool {
    all().iter().any(|k| k.vk == vk)
}

pub fn name_of(vk: u16) -> &'static str {
    all()
        .iter()
        .find(|k| k.vk == vk)
        .map_or("?", |k| k.name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vk_codes_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for k in all() {
            assert!(seen.insert(k.vk), "duplicate vk {:#x} ({})", k.vk, k.name);
        }
    }

    #[test]
    fn lookups() {
        assert_eq!(name_of(VK_RBUTTON), "Right mouse button");
        assert_eq!(name_of(0x70), "F1");
        assert_eq!(name_of(0x5A), "Z");
        assert!(!is_known(0xFF));
    }
}
