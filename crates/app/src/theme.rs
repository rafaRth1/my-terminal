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

/// Font and colors every terminal pane uses, from `config.toml`; the font
/// size changes with Ctrl+= / Ctrl+- / Ctrl+0.
#[derive(Debug, Clone)]
pub struct TerminalSettings {
    pub font_family: gpui::SharedString,
    pub font_size: f32,
    /// Size from the config file, restored by Ctrl+0.
    pub base_font_size: f32,
    pub theme_name: crate::config::ThemeName,
    pub theme: Theme,
}

impl gpui::Global for TerminalSettings {}

impl TerminalSettings {
    pub fn from_config(config: &crate::config::Config) -> Self {
        Self {
            font_family: config.font_family.clone().into(),
            font_size: config.font_size,
            base_font_size: config.font_size,
            theme_name: config.theme,
            theme: Theme::from_name(config.theme),
        }
    }

    /// The settings as written to `config.toml` (the zoom is not saved).
    pub fn to_config(&self) -> crate::config::Config {
        crate::config::Config {
            font_family: self.font_family.to_string(),
            font_size: self.base_font_size,
            theme: self.theme_name,
        }
    }

    pub fn line_height(&self) -> f32 {
        self.font_size * LINE_HEIGHT
    }
}

/// Terminal palette: the 16 ANSI colors plus defaults.
#[derive(Debug, Clone)]
pub struct Theme {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    /// Background of selected text.
    pub selection: Rgb,
    /// Background of search matches, and of the match being shown.
    pub search_match: Rgb,
    pub search_current: Rgb,
    /// Text on `search_current`.
    pub search_current_fg: Rgb,
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
            search_match: rgb(0x004d_3d10),
            search_current: rgb(0x00e3_b341),
            search_current_fg: rgb(0x000a_0a0a),
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
    /// The built-in theme called `name`.
    pub fn from_name(name: crate::config::ThemeName) -> Self {
        match name {
            crate::config::ThemeName::Dark => Self::default(),
            crate::config::ThemeName::Light => Self::light(),
        }
    }

    /// Light theme: white background, GitHub-like palette.
    fn light() -> Self {
        Self {
            foreground: rgb(0x001f_2328),
            background: rgb(0x00ff_ffff),
            cursor: rgb(0x0009_69da),
            selection: rgb(0x00b6_d7ff),
            search_match: rgb(0x00ff_f2b3),
            search_current: rgb(0x00f2_cc60),
            search_current_fg: rgb(0x001f_2328),
            ansi: [
                rgb(0x0024_292f), // black
                rgb(0x00cf_222e), // red
                rgb(0x0011_6329), // green
                rgb(0x004d_2d00), // yellow
                rgb(0x0009_69da), // blue
                rgb(0x0082_50df), // magenta
                rgb(0x001b_7c83), // cyan
                rgb(0x006e_7781), // white
                rgb(0x0057_606a), // bright black
                rgb(0x00a4_0e26), // bright red
                rgb(0x001a_7f37), // bright green
                rgb(0x0063_3c01), // bright yellow
                rgb(0x0021_8bff), // bright blue
                rgb(0x00a4_75f9), // bright magenta
                rgb(0x0031_92aa), // bright cyan
                rgb(0x008c_959f), // bright white
            ],
        }
    }

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

    #[test]
    fn named_themes() {
        use crate::config::ThemeName;
        let dark = Theme::from_name(ThemeName::Dark);
        assert_eq!(dark.background, Theme::default().background);
        let light = Theme::from_name(ThemeName::Light);
        assert_eq!(
            light.background,
            Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        // Text stays readable on the current-match highlight in both.
        assert_ne!(light.search_current_fg, light.search_current);
        assert_ne!(dark.search_current_fg, dark.search_current);
    }
}
