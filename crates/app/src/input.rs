//! Maps gpui keystrokes to `term_core::keys`.

use gpui::Keystroke;
use term_core::keys::{self, Key, Modifiers, NamedKey};

/// Bytes to send to the terminal for a keystroke, if any.
pub fn keystroke_to_bytes(keystroke: &Keystroke, app_cursor: bool) -> Option<Vec<u8>> {
    let mods = Modifiers {
        ctrl: keystroke.modifiers.control,
        alt: keystroke.modifiers.alt,
        shift: keystroke.modifiers.shift,
    };
    // Platform (Super) shortcuts belong to the app, not the program.
    if keystroke.modifiers.platform {
        return None;
    }

    let key = if let Some(named) = named_key(&keystroke.key) {
        Key::Named(named)
    } else if mods.ctrl || mods.alt {
        // With Ctrl/Alt the typed char may be missing; the key name is enough.
        Key::Text(if keystroke.key == "space" {
            " "
        } else {
            &keystroke.key
        })
    } else if let Some(text) = keystroke.key_char.as_deref() {
        Key::Text(text)
    } else if keystroke.key == "space" {
        Key::Text(" ")
    } else {
        return None;
    };
    keys::encode(key, mods, app_cursor)
}

fn named_key(key: &str) -> Option<NamedKey> {
    Some(match key {
        "enter" => NamedKey::Enter,
        "tab" => NamedKey::Tab,
        "backspace" => NamedKey::Backspace,
        "escape" => NamedKey::Escape,
        "up" => NamedKey::Up,
        "down" => NamedKey::Down,
        "left" => NamedKey::Left,
        "right" => NamedKey::Right,
        "home" => NamedKey::Home,
        "end" => NamedKey::End,
        "pageup" => NamedKey::PageUp,
        "pagedown" => NamedKey::PageDown,
        "insert" => NamedKey::Insert,
        "delete" => NamedKey::Delete,
        f if f.len() > 1 && f.starts_with('f') => NamedKey::F(f[1..].parse().ok()?),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(spec: &str) -> Option<Vec<u8>> {
        let keystroke = Keystroke::parse(spec).expect("valid keystroke");
        keystroke_to_bytes(&keystroke, false)
    }

    #[test]
    fn maps_named_and_control_keys() {
        assert_eq!(bytes("enter").unwrap(), b"\r");
        assert_eq!(bytes("ctrl-c").unwrap(), [0x03]);
        assert_eq!(bytes("alt-b").unwrap(), b"\x1bb");
        assert_eq!(bytes("f5").unwrap(), b"\x1b[15~");
        assert_eq!(bytes("ctrl-space").unwrap(), [0x00]);
    }

    #[test]
    fn platform_shortcuts_are_not_sent() {
        assert_eq!(bytes("super-t"), None);
    }
}
