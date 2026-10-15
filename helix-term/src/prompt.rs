//! Command prompt state and its sole command registry.
//! Input mutation, completion and execution resolve against the same commands.
//! This module does not own a renderer or an editor event loop.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use helix_core::graphemes::grapheme_width;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
}

struct CommandSpec {
    name: &'static str,
    action: Action,
}

// Every completion is an executable command. No sample command list.
const COMMANDS: &[CommandSpec] = &[CommandSpec {
    name: "q",
    action: Action::Quit,
}];

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Edited,
    Cancelled,
    Execute(Action),
}

/// The buffer cursor is always a UTF-8 byte boundary and a grapheme boundary.
/// A selected suggestion does not overwrite the typed prefix.
#[derive(Default)]
pub struct Prompt {
    line: String,
    cursor: usize,
    selected: Option<usize>,
    invalid: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Layout {
    pub columns: usize,
    pub rows: usize,
    pub cell_width: usize,
    pub first: usize,
}

impl Prompt {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> &str {
        &self.line
    }

    pub fn invalid(&self) -> bool {
        self.invalid
    }

    pub fn cursor_cells(&self) -> usize {
        1 + self.line[..self.cursor]
            .graphemes(true)
            .map(grapheme_width)
            .sum::<usize>()
    }


    /// Render the section of the input surrounding the cursor inside the
    /// terminal width. Clip only at grapheme boundaries.
    pub fn visible_input(&self, columns: usize) -> (&str, usize) {
        let mut start = 0;
        let mut cursor_x = self.cursor_cells();
        let max_x = columns.saturating_sub(1);
        for (index, grapheme) in self.line[..self.cursor].grapheme_indices(true) {
            if cursor_x <= max_x {
                break;
            }
            start = index + grapheme.len();
            cursor_x = cursor_x.saturating_sub(grapheme_width(grapheme));
        }
        (&self.line[start..], cursor_x)
    }

    fn matches(&self) -> impl Iterator<Item = &'static CommandSpec> + '_ {
        COMMANDS.iter().filter(move |cmd| cmd.name.starts_with(&self.line))
    }

    pub fn candidates(&self) -> impl Iterator<Item = (usize, &'static str)> + '_ {
        self.matches().enumerate().map(|(index, cmd)| (index, cmd.name))
    }

    pub fn selection(&self) -> Option<usize> {
        self.selected
    }

    fn previous(&self) -> usize {
        self.line[..self.cursor]
            .grapheme_indices(true)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn next(&self) -> usize {
        self.line[self.cursor..]
            .graphemes(true)
            .next()
            .map(|g| self.cursor + g.len())
            .unwrap_or(self.line.len())
    }

    fn changed(&mut self) {
        self.selected = None;
        self.invalid = false;
    }

    fn request(&self) -> Option<Action> {
        if let Some(selected) = self.selected {
            self.matches().nth(selected).map(|command| command.action)
        } else {
            COMMANDS
                .iter()
                .find(|command| command.name == self.line)
                .map(|command| command.action)
        }
    }

    /// Number of columns and rows are derived from terminal bounds, not fixed
    /// widths. The selected candidate is paged into the visible area.
    pub fn layout(&self, size: (u16, u16)) -> Option<Layout> {
        let (width, height) = (size.0 as usize, size.1 as usize);
        let available_rows = height.saturating_sub(1);
        let count = self.matches().count();
        if width == 0 || available_rows == 0 || count == 0 {
            return None;
        }
        let longest = self.matches().map(|cmd| cmd.name.len()).max().unwrap_or(0);
        let cell_width = longest.saturating_add(2).min(width).max(1);
        let columns = (width / cell_width).max(1);
        let rows = (1 + (count - 1) / columns).min(available_rows);
        let capacity = rows * columns;
        let first = self.selected.unwrap_or(0) / capacity * capacity;
        Some(Layout {
            columns,
            rows,
            cell_width,
            first,
        })
    }

    pub fn handle(&mut self, key: KeyEvent) -> Outcome {
        match key.code {
            KeyCode::Esc => return Outcome::Cancelled,
            KeyCode::Enter => {
                return match self.request() {
                    Some(action) => Outcome::Execute(action),
                    None => {
                        self.invalid = true;
                        Outcome::Edited
                    }
                };
            }
            KeyCode::Tab => {
                let count = self.matches().count();
                self.selected = if count == 0 {
                    None
                } else {
                    Some(self.selected.map_or(0, |i| (i + 1) % count))
                };
            }
            KeyCode::Left => self.cursor = self.previous(),
            KeyCode::Right => self.cursor = self.next(),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.line.len(),
            KeyCode::Backspace => {
                let prev = self.previous();
                self.line.drain(prev..self.cursor);
                self.cursor = prev;
                self.changed();
            }
            KeyCode::Delete => {
                let next = self.next();
                self.line.drain(self.cursor..next);
                self.changed();
            }
            KeyCode::Char('a') if key.modifiers == KeyModifiers::CONTROL => self.cursor = 0,
            KeyCode::Char('e') if key.modifiers == KeyModifiers::CONTROL => {
                self.cursor = self.line.len()
            }
            KeyCode::Char(c)
                if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.line.insert(self.cursor, c);
                self.cursor += c.len_utf8();
                self.changed();
            }
            _ => (),
        }
        Outcome::Edited
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn actual_command_table_drives_completion_and_execution() {
        let mut prompt = Prompt::new();
        assert_eq!(prompt.candidates().map(|(_, s)| s).collect::<Vec<_>>(), vec!["q"]);
        prompt.handle(key(KeyCode::Tab));
        assert_eq!(prompt.selection(), Some(0));
        assert_eq!(prompt.text(), "");
        assert_eq!(prompt.handle(key(KeyCode::Enter)), Outcome::Execute(Action::Quit));
        let mut prompt = Prompt::new();
        prompt.handle(key(KeyCode::Char('q')));
        assert_eq!(prompt.handle(key(KeyCode::Enter)), Outcome::Execute(Action::Quit));
    }

    #[test]
    fn unknown_commands_do_not_execute_or_quit() {
        let mut prompt = Prompt::new();
        prompt.handle(key(KeyCode::Char('x')));
        assert_eq!(prompt.handle(key(KeyCode::Enter)), Outcome::Edited);
        assert!(prompt.invalid());
        prompt.handle(key(KeyCode::Backspace));
        assert!(!prompt.invalid());
        assert_eq!(prompt.handle(key(KeyCode::Esc)), Outcome::Cancelled);
    }

    #[test]
    fn unicode_and_combining_marks_keep_grapheme_boundaries() {
        let mut prompt = Prompt::new();
        for c in "中🙂e\u{301}x".chars() {
            prompt.handle(key(KeyCode::Char(c)));
        }
        assert_eq!(prompt.text(), "中🙂e\u{301}x");
        assert_eq!(prompt.cursor_cells(), 7); // ':', CJK(2), emoji(2), e-accent(1), x(1)
        prompt.handle(key(KeyCode::Left));
        prompt.handle(key(KeyCode::Backspace));
        assert_eq!(prompt.text(), "中🙂x");
        prompt.handle(key(KeyCode::Home));
        prompt.handle(key(KeyCode::Delete));
        assert_eq!(prompt.text(), "🙂x");
        prompt.handle(key(KeyCode::End));
        prompt.handle(key(KeyCode::Char('界')));
        assert_eq!(prompt.text(), "🙂x界");
    }

    #[test]
    fn cursor_and_selection_remain_independent() {
        let mut prompt = Prompt::new();
        prompt.handle(key(KeyCode::Tab));
        assert_eq!(prompt.selection(), Some(0));
        prompt.handle(key(KeyCode::Char('q')));
        assert_eq!(prompt.selection(), None);
        assert_eq!(prompt.text(), "q");
        prompt.handle(key(KeyCode::Left));
        prompt.handle(key(KeyCode::Char('x')));
        assert_eq!(prompt.text(), "xq");
        assert_eq!(prompt.handle(key(KeyCode::Enter)), Outcome::Edited);
    }

    #[test]
    fn long_unicode_prompt_is_clipped_at_grapheme_boundaries() {
        let mut prompt = Prompt::new();
        for c in "天地玄黄🙂".chars() {
            prompt.handle(key(KeyCode::Char(c)));
        }
        for width in 0..12 {
            let (visible, x) = prompt.visible_input(width);
            assert!(std::str::from_utf8(visible.as_bytes()).is_ok());
            assert!(x < width || width < 2);
        }
    }

    #[test]
    fn layout_is_bounded_for_all_small_terminal_dimensions() {
        for width in 0..35 {
            for height in 0..5 {
                let prompt = Prompt::new();
                if let Some(layout) = prompt.layout((width, height)) {
                    assert!(width > 0 && height > 1);
                    assert!(layout.columns >= 1 && layout.rows >= 1);
                    assert!(layout.cell_width * layout.columns <= width as usize);
                    assert!(layout.rows < height as usize);
                }
            }
        }
    }

    #[test]
    fn repeated_editing_and_navigation_cannot_invalidate_cursor() {
        let mut prompt = Prompt::new();
        for _ in 0..10_000 {
            prompt.handle(key(KeyCode::Char('你')));
            prompt.handle(key(KeyCode::Left));
            prompt.handle(key(KeyCode::Right));
            prompt.handle(key(KeyCode::Backspace));
            assert_eq!(prompt.text(), "");
            assert_eq!(prompt.cursor, 0);
        }
    }
}
