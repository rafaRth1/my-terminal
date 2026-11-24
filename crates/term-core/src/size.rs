use alacritty_terminal::event::WindowSize;
use alacritty_terminal::grid::Dimensions;

/// Size of a terminal in cells, plus the pixel size of one cell.
///
/// The cell size is forwarded to the PTY so programs that query pixel
/// dimensions (for example image protocols) get correct values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl TerminalSize {
    /// Creates a size; `cols` and `rows` are clamped to at least 1.
    #[must_use]
    pub fn new(cols: u16, rows: u16, cell_width: u16, cell_height: u16) -> Self {
        Self {
            cols: cols.max(1),
            rows: rows.max(1),
            cell_width,
            cell_height,
        }
    }
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self::new(80, 24, 8, 16)
    }
}

impl From<TerminalSize> for WindowSize {
    fn from(size: TerminalSize) -> Self {
        Self {
            num_cols: size.cols,
            num_lines: size.rows,
            cell_width: size.cell_width,
            cell_height: size.cell_height,
        }
    }
}

impl Dimensions for TerminalSize {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn columns(&self) -> usize {
        usize::from(self.cols)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_clamps_zero_dimensions_to_one() {
        let size = TerminalSize::new(0, 0, 8, 16);
        assert_eq!((size.cols, size.rows), (1, 1));
    }

    #[test]
    fn converts_to_window_size() {
        let window: WindowSize = TerminalSize::new(120, 40, 9, 18).into();
        assert_eq!(
            (
                window.num_cols,
                window.num_lines,
                window.cell_width,
                window.cell_height
            ),
            (120, 40, 9, 18)
        );
    }
}
