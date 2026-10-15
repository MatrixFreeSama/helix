use anyhow::Error;

use std::{borrow::Cow, path::PathBuf};

use crate::theme::Theme;
use helix_core::{
    graphemes::{grapheme_width, RopeGraphemes},
    Position, State,
};

pub const TAB_WIDTH: usize = 4;

/// Same display-cell advance for both terminal rendering and cursor placement.
/// Tabs move to the next column aligned to TAB_WIDTH, not four cells forward.
#[inline]
pub fn display_grapheme_width(grapheme: &str, col: usize) -> usize {
    if grapheme == "\t" {
        TAB_WIDTH - col % TAB_WIDTH
    } else {
        grapheme_width(grapheme)
    }
}

pub struct View {
    pub state: State,
    pub first_line: usize,
    pub size: (u16, u16),
    pub theme: Theme, // TODO: share one instance
}

impl View {
    pub fn empty(size: (u16, u16)) -> Self {
        Self {
            state: State::new(helix_core::Rope::new()),
            first_line: 0,
            size,
            theme: Theme::default(),
        }
    }

    pub fn open(path: PathBuf, size: (u16, u16)) -> Result<Self, Error> {
        let theme = Theme::default();
        let state = State::load(path, theme.scopes())?;

        let view = Self {
            state,
            first_line: 0,
            size, // TODO: pass in from term
            theme,
        };

        Ok(view)
    }

    /// Text rows, excluding status bar. One logical row is retained when
    /// terminal height is zero so navigation arithmetic stays well-defined.
    pub fn text_rows(&self) -> usize {
        (self.size.1 as usize).saturating_sub(1).max(1)
    }

    pub fn cursor_padding(&self) -> usize {
        5usize.min((self.text_rows() - 1) / 2)
    }

    pub fn ensure_cursor_in_view(&mut self) {
        let line = self
            .state
            .doc()
            .char_to_line(self.state.selection().cursor());
        let visible = self.text_rows();
        let max_first = self.state.doc().len_lines().saturating_sub(visible);
        self.first_line = self.first_line.min(max_first);

        // Padding can never push the viewport beyond either document edge.
        let padding = self.cursor_padding();
        let last_comfortable = self.first_line.saturating_add(visible - 1 - padding);
        if line > last_comfortable {
            self.first_line = line.saturating_sub(visible - 1 - padding).min(max_first);
        } else if line < self.first_line.saturating_add(padding) {
            self.first_line = line.saturating_sub(padding).min(max_first);
        }
        // Keep the cursor on-screen even when a tiny viewport has no padding.
        if line >= self.first_line.saturating_add(visible) {
            self.first_line = line.saturating_sub(visible - 1).min(max_first);
        }
    }

    /// Viewport-relative row and display-cell column. Return None if the
    /// document cursor is not visible, rather than crashing or wrapping.
    pub fn visible_cursor_position(&self, pos: usize, viewport_height: u16) -> Option<Position> {
        let text = self.state.doc();
        if viewport_height == 0 || pos > text.len_chars() {
            return None;
        }
        let row = text.char_to_line(pos);
        let first = self.first_line;
        if row < first || row >= first + viewport_height as usize {
            return None;
        }

        let prefix = text.slice(text.line_to_char(row)..pos);
        let mut col = 0;
        for grapheme in RopeGraphemes::new(&prefix) {
            let grapheme: Cow<str> = Cow::from(grapheme);
            col += display_grapheme_width(&grapheme, col);
        }
        Some(Position::new(row - first, col))
    }
}

#[cfg(test)]
mod tab_position_tests {
    use super::*;
    use helix_core::Rope;

    fn view(text: &str) -> View {
        View {
            state: State::new(Rope::from(text)),
            first_line: 0,
            size: (80, 25),
            theme: Theme::default(),
        }
    }

    #[test]
    fn tabs_align_to_stops_not_fixed_increments() {
        assert_eq!(display_grapheme_width("\t", 0), 4);
        assert_eq!(display_grapheme_width("\t", 1), 3);
        assert_eq!(display_grapheme_width("\t", 2), 2);
        assert_eq!(display_grapheme_width("\t", 3), 1);
        assert_eq!(display_grapheme_width("\t", 4), 4);
        assert_eq!(display_grapheme_width("\t", 7), 1);
        assert_eq!(display_grapheme_width("你", 0), 2);
        assert_eq!(display_grapheme_width("a", 0), 1);
    }

    #[test]
    fn cursor_column_agrees_with_renderer_for_tabs_and_wide_chars() {
        let v = view("a\tb\nabcd\te\n你\tq");
        assert_eq!(v.visible_cursor_position(2, 15), Some(Position::new(0, 4)));
        assert_eq!(v.visible_cursor_position(3, 15), Some(Position::new(0, 5)));
        assert_eq!(v.visible_cursor_position(9, 15), Some(Position::new(1, 8)));
        assert_eq!(v.visible_cursor_position(13, 15), Some(Position::new(2, 4)));
    }

    #[test]
    fn scroll_offsets_are_subtracted_once_and_out_of_view_is_safe() {
        let mut v = view("zero\none\ntwo\nthree\nfour\nfive");
        v.first_line = 2;
        assert_eq!(v.visible_cursor_position(0, 3), None);
        assert_eq!(v.visible_cursor_position(9, 3), Some(Position::new(0, 0)));
        assert_eq!(v.visible_cursor_position(13, 3), Some(Position::new(1, 0)));
        assert_eq!(v.visible_cursor_position(0, 0), None);
        assert_eq!(v.visible_cursor_position(100_000, 3), None);
    }

    #[test]
    fn tiny_terminal_scroll_does_not_underflow() {
        let mut v = view("hello\nworld");
        v.size = (8, 0);
        v.ensure_cursor_in_view();
        v.size = (8, 1);
        v.ensure_cursor_in_view();
        assert_eq!(v.first_line, 0);
    }
}
