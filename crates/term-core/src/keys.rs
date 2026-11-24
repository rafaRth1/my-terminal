//! Keyboard input to terminal byte sequences (xterm conventions).
//!
//! UI-independent: the app maps its key events to [`Key`] and [`Modifiers`]
//! and writes the returned bytes to the session.

/// Modifier keys held during a key press.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Modifiers {
    fn any(self) -> bool {
        self.ctrl || self.alt || self.shift
    }

    /// xterm modifier parameter: 1 + shift + 2·alt + 4·ctrl.
    fn xterm_param(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }
}

/// Keys without a printable character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedKey {
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// Function key `F1`..=`F12`.
    F(u8),
}

/// A key press: typed text or a named key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key<'a> {
    /// Text the key produces (already shifted, e.g. `"A"`, `"ñ"`).
    Text(&'a str),
    Named(NamedKey),
}

/// Encodes a key press as the bytes a terminal program expects.
///
/// `app_cursor` is the DECCKM mode the program enabled (arrows send `ESC O`
/// instead of `ESC [`). Returns `None` for combinations with no encoding.
#[must_use]
pub fn encode(key: Key<'_>, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    match key {
        Key::Text(text) => encode_text(text, mods),
        Key::Named(named) => encode_named(named, mods, app_cursor),
    }
}

fn encode_text(text: &str, mods: Modifiers) -> Option<Vec<u8>> {
    if text.is_empty() {
        return None;
    }
    let mut bytes = if mods.ctrl {
        let mut chars = text.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            return None;
        };
        vec![control_byte(ch)?]
    } else {
        text.as_bytes().to_vec()
    };
    if mods.alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

/// Ctrl+key: letters map to 0x01..=0x1a, plus the classic symbol keys.
fn control_byte(ch: char) -> Option<u8> {
    match ch.to_ascii_lowercase() {
        c @ 'a'..='z' => Some(c as u8 - b'a' + 1),
        '@' | ' ' | '2' => Some(0x00),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '7' | '-' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

fn encode_named(key: NamedKey, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let alt_prefixed = |bytes: &[u8]| {
        let mut out = Vec::with_capacity(bytes.len() + 1);
        if mods.alt {
            out.push(0x1b);
        }
        out.extend_from_slice(bytes);
        out
    };

    let bytes = match key {
        NamedKey::Enter => alt_prefixed(b"\r"),
        NamedKey::Tab if mods.shift => b"\x1b[Z".to_vec(),
        NamedKey::Tab => alt_prefixed(b"\t"),
        NamedKey::Backspace if mods.ctrl => alt_prefixed(b"\x08"),
        NamedKey::Backspace => alt_prefixed(b"\x7f"),
        NamedKey::Escape => alt_prefixed(b"\x1b"),
        NamedKey::Up => cursor(b'A', mods, app_cursor),
        NamedKey::Down => cursor(b'B', mods, app_cursor),
        NamedKey::Right => cursor(b'C', mods, app_cursor),
        NamedKey::Left => cursor(b'D', mods, app_cursor),
        NamedKey::Home => cursor(b'H', mods, app_cursor),
        NamedKey::End => cursor(b'F', mods, app_cursor),
        NamedKey::Insert => tilde(2, mods),
        NamedKey::Delete => tilde(3, mods),
        NamedKey::PageUp => tilde(5, mods),
        NamedKey::PageDown => tilde(6, mods),
        NamedKey::F(n @ 1..=4) => {
            let letter = b'P' + (n - 1);
            if mods.any() {
                format!("\x1b[1;{}{}", mods.xterm_param(), letter as char).into_bytes()
            } else {
                vec![0x1b, b'O', letter]
            }
        }
        NamedKey::F(n @ 5..=12) => {
            const CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
            tilde(CODES[usize::from(n - 5)], mods)
        }
        NamedKey::F(_) => return None,
    };
    Some(bytes)
}

/// Arrow-style keys: `ESC [ X`, `ESC O X` in app mode, `ESC [ 1 ; m X` with modifiers.
fn cursor(letter: u8, mods: Modifiers, app_cursor: bool) -> Vec<u8> {
    if mods.any() {
        format!("\x1b[1;{}{}", mods.xterm_param(), letter as char).into_bytes()
    } else if app_cursor {
        vec![0x1b, b'O', letter]
    } else {
        vec![0x1b, b'[', letter]
    }
}

/// Editing keys: `ESC [ n ~`, `ESC [ n ; m ~` with modifiers.
fn tilde(code: u8, mods: Modifiers) -> Vec<u8> {
    if mods.any() {
        format!("\x1b[{code};{}~", mods.xterm_param()).into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        ctrl: false,
        alt: false,
        shift: false,
    };
    const CTRL: Modifiers = Modifiers { ctrl: true, ..NONE };
    const ALT: Modifiers = Modifiers { alt: true, ..NONE };
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ..NONE
    };

    fn enc(key: Key<'_>, mods: Modifiers) -> Vec<u8> {
        encode(key, mods, false).expect("key has an encoding")
    }

    #[test]
    fn plain_text_is_sent_as_utf8() {
        assert_eq!(enc(Key::Text("a"), NONE), b"a");
        assert_eq!(enc(Key::Text("ñ"), NONE), "ñ".as_bytes());
        assert_eq!(enc(Key::Text("A"), SHIFT), b"A");
    }

    #[test]
    fn ctrl_letters_map_to_control_codes() {
        assert_eq!(enc(Key::Text("c"), CTRL), [0x03]);
        assert_eq!(enc(Key::Text("C"), CTRL), [0x03]);
        assert_eq!(enc(Key::Text("d"), CTRL), [0x04]);
        assert_eq!(enc(Key::Text("["), CTRL), [0x1b]);
    }

    #[test]
    fn alt_prefixes_escape() {
        assert_eq!(enc(Key::Text("b"), ALT), b"\x1bb");
        assert_eq!(enc(Key::Named(NamedKey::Backspace), ALT), b"\x1b\x7f");
    }

    #[test]
    fn editing_keys() {
        assert_eq!(enc(Key::Named(NamedKey::Enter), NONE), b"\r");
        assert_eq!(enc(Key::Named(NamedKey::Backspace), NONE), [0x7f]);
        assert_eq!(enc(Key::Named(NamedKey::Tab), SHIFT), b"\x1b[Z");
        assert_eq!(enc(Key::Named(NamedKey::Delete), NONE), b"\x1b[3~");
        assert_eq!(enc(Key::Named(NamedKey::PageUp), CTRL), b"\x1b[5;5~");
    }

    #[test]
    fn arrows_follow_cursor_mode_and_modifiers() {
        assert_eq!(enc(Key::Named(NamedKey::Up), NONE), b"\x1b[A");
        assert_eq!(
            encode(Key::Named(NamedKey::Up), NONE, true).unwrap(),
            b"\x1bOA"
        );
        assert_eq!(enc(Key::Named(NamedKey::Left), CTRL), b"\x1b[1;5D");
    }

    #[test]
    fn function_keys() {
        assert_eq!(enc(Key::Named(NamedKey::F(1)), NONE), b"\x1bOP");
        assert_eq!(enc(Key::Named(NamedKey::F(5)), NONE), b"\x1b[15~");
        assert_eq!(enc(Key::Named(NamedKey::F(12)), SHIFT), b"\x1b[24;2~");
        assert_eq!(encode(Key::Named(NamedKey::F(13)), NONE, false), None);
    }

    #[test]
    fn unencodable_input_returns_none() {
        assert_eq!(encode(Key::Text(""), NONE, false), None);
        assert_eq!(encode(Key::Text("ab"), CTRL, false), None);
    }
}
