//! Colors and font settings.

use gpui::{Hsla, Rgba};
use term_core::alacritty_terminal::term::color::Colors;
use term_core::alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

pub const FONT_FAMILY: &str = "FiraCode Nerd Font Mono";
pub const FONT_SIZE: f32 = 14.0;
/// Line height as a multiple of the font size.
pub const LINE_HEIGHT: f32 = 1.3;
/// Space between the window edge and the grid, in pixels.
pub const PADDING: f32 = 12.0;

/// Terminal palette: the 16 ANSI colors plus defaults.
#[derive(Debug, Clone)]
pub struct Theme {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    /// Background of selected text.
    pub selection: Rgb,
    pub ansi: [Rgb; 16],
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "each shift keeps exactly one byte of the 0xRRGGBB value"
)]
const fn rgb(hex: u32) -> Rgb {
    Rgb {
        r: (hex >> 16) as u8,
        g: (hex >> 8) as u8,
        b: hex as u8,
    }
}

impl Default for Theme {
    /// Dark theme close to the reference screenshot: black background,
    /// saturated diff greens and reds.
    fn default() -> Self {
        Self {
            foreground: rgb(0x00d4_d4d4),
            background: rgb(0x000a_0a0a),
            cursor: rgb(0x0058_a6ff),
            selection: rgb(0x0026_4f78),
            ansi: [
                rgb(0x001a_1a1a), // black
                rgb(0x00f4_4747), // red
                rgb(0x003f_b950), // green
                rgb(0x00d2_9922), // yellow
                rgb(0x0058_a6ff), // blue
                rgb(0x00bc_8cff), // magenta
                rgb(0x0039_c5cf), // cyan
                rgb(0x00d4_d4d4), // white
                rgb(0x006e_7681), // bright black
                rgb(0x00ff_7b72), // bright red
                rgb(0x0056_d364), // bright green
                rgb(0x00e3_b341), // bright yellow
                rgb(0x0079_c0ff), // bright blue
                rgb(0x00d2_a8ff), // bright magenta
                rgb(0x0056_d4dd), // bright cyan
                rgb(0x00ff_ffff), // bright white
            ],
        }
    }
}

impl Theme {
    /// Resolves a cell color. Colors the program redefined (OSC 4) win.
    pub fn resolve(&self, color: Color, overrides: &Colors) -> Rgb {
        match color {
            Color::Spec(rgb) => rgb,
            Color::Indexed(index) => {
                overrides[usize::from(index)].unwrap_or_else(|| self.indexed(index))
            }
            Color::Named(named) => overrides[named as usize].unwrap_or_else(|| self.named(named)),
        }
    }

    fn named(&self, named: NamedColor) -> Rgb {
        match named {
            NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground => {
                self.foreground
            }
            NamedColor::Background => self.background,
            NamedColor::Cursor => self.cursor,
            other => {
                let index = other as usize;
                if index < 16 {
                    self.ansi[index]
                } else {
                    // Dim variants (`DimBlack`..) follow the 16 base colors.
                    self.foreground
                }
            }
        }
    }

    /// xterm 256-color palette: 16 ANSI, 6×6×6 cube, 24 grays.
    fn indexed(&self, index: u8) -> Rgb {
        match index {
            0..=15 => self.ansi[usize::from(index)],
            16..=231 => {
                let i = index - 16;
                let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                Rgb {
                    r: level(i / 36),
                    g: level((i / 6) % 6),
                    b: level(i % 6),
                }
            }
            232..=255 => {
                let gray = 8 + (index - 232) * 10;
                Rgb {
                    r: gray,
                    g: gray,
                    b: gray,
                }
            }
        }
    }
}

/// Converts a terminal color to a gpui color.
pub fn to_hsla(color: Rgb) -> Hsla {
    Rgba {
        r: f32::from(color.r) / 255.0,
        g: f32::from(color.g) / 255.0,
        b: f32::from(color.b) / 255.0,
        a: 1.0,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_colors_follow_xterm_palette() {
        let theme = Theme::default();
        assert_eq!(theme.indexed(1), theme.ansi[1]);
        assert_eq!(theme.indexed(16), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(
            theme.indexed(231),
            Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert_eq!(theme.indexed(232), Rgb { r: 8, g: 8, b: 8 });
        assert_eq!(
            theme.indexed(255),
            Rgb {
                r: 238,
                g: 238,
                b: 238
            }
        );
    }

    #[test]
    fn program_overrides_win_over_theme() {
        let theme = Theme::default();
        let mut overrides = Colors::default();
        let custom = Rgb { r: 1, g: 2, b: 3 };
        overrides[2] = Some(custom);
        assert_eq!(theme.resolve(Color::Indexed(2), &overrides), custom);
        assert_eq!(
            theme.resolve(Color::Named(NamedColor::Red), &overrides),
            theme.ansi[1]
        );
    }
}
