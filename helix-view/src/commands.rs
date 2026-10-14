use helix_core::{
    graphemes,
    regex::Regex,
    selection,
    state::{Direction, Granularity, Mode, State},
    Range, Selection, Tendril, Transaction,
};
use once_cell::sync::Lazy;

use crate::view::View;

/// A command is a function that takes the current state and a count, and does a side-effect on the
/// state (usually by creating and applying a transaction).
pub type Command = fn(view: &mut View, count: usize);

pub fn move_char_left(view: &mut View, count: usize) {
    // TODO: use a transaction
    let selection = view
        .state
        .move_selection(Direction::Backward, Granularity::Character, count);
    view.state.selection = selection;
}

pub fn move_char_right(view: &mut View, count: usize) {
    // TODO: use a transaction
    view.state.selection =
        view.state
            .move_selection(Direction::Forward, Granularity::Character, count);
}

pub fn move_line_up(view: &mut View, count: usize) {
    // TODO: use a transaction
    view.state.selection = view
        .state
        .move_selection(Direction::Backward, Granularity::Line, count);
}

pub fn move_line_down(view: &mut View, count: usize) {
    // TODO: use a transaction
    view.state.selection = view
        .state
        .move_selection(Direction::Forward, Granularity::Line, count);
}

pub fn move_line_end(view: &mut View, _count: usize) {
    // TODO: use a transaction
    let lines = selection_lines(&view.state);

    let positions = lines
        .into_iter()
        .map(|index| {
            // adjust all positions to the end of the line.

            // Line end is pos at the start of next line - 1
            // subtract another 1 because the line ends with \n
            view.state.doc.line_to_char(index + 1).saturating_sub(2)
        })
        .map(|pos| Range::new(pos, pos));

    let selection = Selection::new(positions.collect(), 0);

    let transaction = Transaction::new(&mut view.state).with_selection(selection);

    transaction.apply(&mut view.state);
}

pub fn move_line_start(view: &mut View, _count: usize) {
    let lines = selection_lines(&view.state);

    let positions = lines
        .into_iter()
        .map(|index| {
            // adjust all positions to the start of the line.
            view.state.doc.line_to_char(index)
        })
        .map(|pos| Range::new(pos, pos));

    let selection = Selection::new(positions.collect(), 0);

    let transaction = Transaction::new(&mut view.state).with_selection(selection);

    transaction.apply(&mut view.state);
}

pub fn move_next_word_start(view: &mut View, count: usize) {
    let pos = view.state.move_pos(
        view.state.selection.cursor(),
        Direction::Forward,
        Granularity::Word,
        count,
    );

    // TODO: use a transaction
    view.state.selection = Selection::single(pos, pos);
}

pub fn move_prev_word_start(view: &mut View, count: usize) {
    let pos = view.state.move_pos(
        view.state.selection.cursor(),
        Direction::Backward,
        Granularity::Word,
        count,
    );

    // TODO: use a transaction
    view.state.selection = Selection::single(pos, pos);
}

pub fn move_next_word_end(view: &mut View, count: usize) {
    let pos = State::move_next_word_end(
        &view.state.doc().slice(..),
        view.state.selection.cursor(),
        count,
    );

    // TODO: use a transaction
    view.state.selection = Selection::single(pos, pos);
}

// File navigation and scrolling use document lines and a single clamped
// viewport operation. There is no separate event loop or key allocation.
pub fn goto_mode(view: &mut View, _count: usize) {
    view.state.mode = Mode::Goto;
}

pub fn move_file_start(view: &mut View, _count: usize) {
    view.state.selection = Selection::single(0, 0);
    view.state.mode = Mode::Normal;
}

pub fn move_file_end(view: &mut View, _count: usize) {
    let doc = view.state.doc();
    let last = doc.len_lines().saturating_sub(1);
    // A trailing newline produces a final empty sentinel line.
    let last = if doc.len_chars() > 0 && doc.char(doc.len_chars() - 1) == '\n' {
        last.saturating_sub(1)
    } else {
        last
    };
    let pos = doc.line_to_char(last);
    view.state.selection = Selection::single(pos, pos);
    view.state.mode = Mode::Normal;
}

fn scroll(view: &mut View, rows: usize, down: bool) {
    let visible = view.text_rows();
    let total = view.state.doc().len_lines();
    let max_first = total.saturating_sub(visible);
    let next = if down {
        view.first_line.saturating_add(rows)
    } else {
        view.first_line.saturating_sub(rows)
    };
    view.first_line = next.min(max_first);
    let cursor = view.state.selection().cursor();
    let line = view.state.doc().char_to_line(cursor);
    let first = view.first_line;
    let last = first.saturating_add(visible - 1).min(total.saturating_sub(1));
    let target = line.max(first).min(last);
    if target != line {
        let pos = view.state.doc().line_to_char(target);
        view.state.selection = Selection::single(pos, pos);
    }
}

pub fn page_up(view: &mut View, _count: usize) {
    scroll(view, view.text_rows(), false);
}

pub fn page_down(view: &mut View, _count: usize) {
    scroll(view, view.text_rows(), true);
}

pub fn half_page_up(view: &mut View, _count: usize) {
    scroll(view, (view.text_rows() / 2).max(1), false);
}

pub fn half_page_down(view: &mut View, _count: usize) {
    scroll(view, (view.text_rows() / 2).max(1), true);
}

// avoid select by default by having a visual mode switch that makes movements into selects

pub fn extend_char_left(view: &mut View, count: usize) {
    // TODO: use a transaction
    let selection = view
        .state
        .extend_selection(Direction::Backward, Granularity::Character, count);
    view.state.selection = selection;
}

pub fn extend_char_right(view: &mut View, count: usize) {
    // TODO: use a transaction
    view.state.selection =
        view.state
            .extend_selection(Direction::Forward, Granularity::Character, count);
}

pub fn extend_line_up(view: &mut View, count: usize) {
    // TODO: use a transaction
    view.state.selection =
        view.state
            .extend_selection(Direction::Backward, Granularity::Line, count);
}

pub fn extend_line_down(view: &mut View, count: usize) {
    // TODO: use a transaction
    view.state.selection =
        view.state
            .extend_selection(Direction::Forward, Granularity::Line, count);
}

pub fn split_selection_on_newline(view: &mut View, _count: usize) {
    let text = &view.state.doc.slice(..);
    // only compile the regex once
    static REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n").unwrap());
    // TODO: use a transaction
    view.state.selection = selection::split_on_matches(text, view.state.selection(), &REGEX)
}

pub fn delete_selection(view: &mut View, _count: usize) {
    let transaction =
        Transaction::change_by_selection(&view.state, |range| (range.from(), range.to(), None));
    transaction.apply(&mut view.state);
}

pub fn change_selection(view: &mut View, count: usize) {
    delete_selection(view, count);
    insert_mode(view, count);
}

// insert mode:
// first we calculate the correct cursors/selections
// then we just append at each cursor
// lastly, if it was append mode we shift cursor by 1?

// inserts at the start of each selection
pub fn insert_mode(view: &mut View, _count: usize) {
    view.state.mode = Mode::Insert;

    view.state.selection = view
        .state
        .selection
        .transform(|range| Range::new(range.to(), range.from()))
}

// inserts at the end of each selection
pub fn append_mode(view: &mut View, _count: usize) {
    view.state.mode = Mode::Insert;

    // TODO: as transaction
    let text = &view.state.doc.slice(..);
    view.state.selection = view.state.selection.transform(|range| {
        // TODO: to() + next char
        Range::new(
            range.from(),
            graphemes::next_grapheme_boundary(text, range.to()),
        )
    })
}

// TODO: I, A, o and O can share a lot of the primitives.

// calculate line numbers for each selection range
fn selection_lines(state: &State) -> Vec<usize> {
    let mut lines = state
        .selection
        .ranges()
        .iter()
        .map(|range| state.doc.char_to_line(range.head))
        .collect::<Vec<_>>();

    lines.sort_unstable(); // sorting by usize so _unstable is preferred
    lines.dedup();

    lines
}

// I inserts at the start of each line with a selection
pub fn prepend_to_line(view: &mut View, count: usize) {
    view.state.mode = Mode::Insert;

    move_line_start(view, count);
}

// A inserts at the end of each line with a selection
pub fn append_to_line(view: &mut View, count: usize) {
    view.state.mode = Mode::Insert;

    move_line_end(view, count);
}

// o inserts a new line after each line with a selection
pub fn open_below(view: &mut View, _count: usize) {
    view.state.mode = Mode::Insert;

    let lines = selection_lines(&view.state);

    let positions: Vec<_> = lines
        .into_iter()
        .map(|index| {
            // adjust all positions to the end of the line/start of the next one.
            view.state.doc.line_to_char(index + 1)
        })
        .collect();

    let changes = positions.iter().copied().map(|index|
        // generate changes
        (index, index, Some(Tendril::from_char('\n'))));

    // TODO: count actually inserts "n" new lines and starts editing on all of them.
    // TODO: append "count" newlines and modify cursors to those lines

    let selection = Selection::new(
        positions
            .iter()
            .copied()
            .map(|pos| Range::new(pos, pos))
            .collect(),
        0,
    );

    let transaction = Transaction::change(&view.state, changes).with_selection(selection);

    transaction.apply(&mut view.state);
    // TODO: need to store into history if successful
}

// O inserts a new line before each line with a selection

pub fn normal_mode(view: &mut View, _count: usize) {
    // TODO: if leaving append mode, move cursor back by 1
    view.state.mode = Mode::Normal;
}

// TODO: insert means add text just before cursor, on exit we should be on the last letter.
pub fn insert_char(view: &mut View, c: char) {
    let c = Tendril::from_char(c);
    let transaction = Transaction::insert(&view.state, c);

    transaction.apply(&mut view.state);
    // TODO: need to store into history if successful
}

pub fn insert_newline(view: &mut View, _count: usize) {
    insert_char(view, '\n');
}

pub fn insert_tab(view: &mut View, _count: usize) {
    insert_char(view, '\t');
}

// TODO: handle indent-aware delete
pub fn delete_char_backward(view: &mut View, count: usize) {
    let text = &view.state.doc.slice(..);
    let transaction = Transaction::change_by_selection(&view.state, |range| {
        (
            graphemes::nth_prev_grapheme_boundary(text, range.head, count),
            range.head,
            None,
        )
    });
    transaction.apply(&mut view.state);
    // TODO: need to store into history if successful
}

pub fn delete_char_forward(view: &mut View, count: usize) {
    let text = &view.state.doc.slice(..);
    let transaction = Transaction::change_by_selection(&view.state, |range| {
        (
            graphemes::nth_next_grapheme_boundary(text, range.head, count),
            range.head,
            None,
        )
    });
    transaction.apply(&mut view.state);
    // TODO: need to store into history if successful
}

#[cfg(test)]
mod navigation_tests {
    use super::*;
    use helix_core::Rope;
    use crate::theme::Theme;

    fn view(doc: &str, height: u16) -> View {
        View {
            state: State::new(Rope::from(doc)),
            first_line: 0,
            size: (80, height),
            theme: Theme::default(),
        }
    }

    #[test]
    fn file_ends_respect_trailing_newline_and_empty_files() {
        for (text, expected) in [
            ("", 0usize),
            ("a", 0),
            ("a\nb\nc", 4),
            ("a\nb\nc\n", 4),
            ("one\n", 0),
        ] {
            let mut v = view(text, 25);
            goto_mode(&mut v, 1);
            move_file_end(&mut v, 1);
            assert_eq!(v.state.selection().cursor(), expected, "text: {:?}", text);
            assert!(v.state.mode() == Mode::Normal);
            goto_mode(&mut v, 1);
            move_file_start(&mut v, 1);
            assert_eq!(v.state.selection().cursor(), 0);
            assert!(v.state.mode() == Mode::Normal);
        }
    }

    #[test]
    fn short_documents_and_tiny_terminals_never_overflow() {
        for text in ["", "a", "a\nb", "a\nb\nc\n"] {
            for h in [0u16, 1, 2, 3, 24] {
                let mut v = view(text, h);
                for _ in 0..24 {
                    page_down(&mut v, 1);
                    half_page_down(&mut v, 1);
                    page_up(&mut v, 1);
                    half_page_up(&mut v, 1);
                    v.ensure_cursor_in_view();
                    let total = v.state.doc().len_lines();
                    assert!(v.first_line <= total.saturating_sub(v.text_rows()));
                    assert!(v.state.selection().cursor() <= v.state.doc().len_chars());
                }
            }
        }
    }

    #[test]
    fn long_files_scroll_both_directions_and_recover() {
        let text = (0..1000).map(|n| format!("line {}\n", n)).collect::<String>();
        let mut v = view(&text, 18);
        for _ in 0..100 {
            page_down(&mut v, 1);
        }
        let last = v.first_line;
        assert!(last > 100);
        for _ in 0..100 {
            half_page_up(&mut v, 1);
        }
        assert!(v.first_line < last);
        move_file_end(&mut v, 1);
        v.ensure_cursor_in_view();
        assert_eq!(v.state.doc().char_to_line(v.state.selection().cursor()), 999);
        assert!(v.visible_cursor_position(v.state.selection().cursor(), 17).is_some());
        move_file_start(&mut v, 1);
        v.ensure_cursor_in_view();
        assert_eq!(v.first_line, 0);
    }

    #[test]
    fn cursor_can_address_lines_past_u16_max() {
        let text = "a\n".repeat(70_000);
        let mut v = view(&text, 18);
        move_file_end(&mut v, 1);
        v.ensure_cursor_in_view();
        assert!(v.first_line > u16::MAX as usize);
        assert!(v.visible_cursor_position(v.state.selection().cursor(), 17).is_some());
    }
}
