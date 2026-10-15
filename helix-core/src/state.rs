use crate::graphemes::{nth_next_grapheme_boundary, nth_prev_grapheme_boundary, RopeGraphemes};
use crate::syntax::LOADER;
use crate::{Position, Range, Rope, RopeSlice, Selection, Syntax};
use anyhow::Error;

use std::{
    fs::{self, OpenOptions},
    io::{self, BufReader, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// The temporary file lives beside its destination. On failure the temporary
// file is unlinked, while the original file remains intact.
struct PendingSave(PathBuf);

impl Drop for PendingSave {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub enum Mode {
    Normal,
    Insert,
    Goto,
}

/// A state represents the current editor state of a single buffer.
pub struct State {
    // TODO: fields should be private but we need to refactor commands.rs first
    /// Path to file on disk.
    pub path: Option<PathBuf>,
    pub doc: Rope,
    pub selection: Selection,
    pub mode: Mode,

    //
    pub syntax: Option<Syntax>,
    pub(crate) revision: u64,
    saved_revision: u64,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Granularity {
    Character,
    Word,
    Line,
    // LineBoundary
}

impl State {
    #[must_use]
    pub fn new(doc: Rope) -> Self {
        Self {
            path: None,
            doc,
            selection: Selection::single(0, 0),
            mode: Mode::Normal,
            syntax: None,
            revision: 0,
            saved_revision: 0,
        }
    }

    // TODO: passing scopes here is awkward
    pub fn load(path: PathBuf, scopes: &[String]) -> Result<Self, Error> {
        let doc = match fs::File::open(&path) {
            Ok(file) => Rope::from_reader(BufReader::new(file))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Rope::new(),
            Err(error) => return Err(error.into()),
        };

        let mut state = Self::new(doc);

        if let Some(language_config) = LOADER.language_config_for_file_name(path.as_path()) {
            let highlight_config = language_config.highlight_config(scopes).unwrap().unwrap();
            // TODO: config.configure(scopes) is now delayed, is that ok?

            let syntax = Syntax::new(&state.doc, highlight_config.clone());

            state.syntax = Some(syntax);
        };

        state.path = Some(path);

        Ok(state)
    }

    #[inline]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// Persist the rope without materializing the entire file in memory.
    /// A successful same-directory rename commits the replacement atomically
    /// on the Linux target. Failed writes leave the original file unchanged.
    pub fn save(&mut self, new_path: Option<PathBuf>) -> io::Result<()> {
        let path = new_path
            .or_else(|| self.path.clone())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing file name"))?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));

        // Never silently replace a symlink with an unrelated regular file.
        let previous = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "refusing to replace symbolic link",
                    ));
                }
                if !metadata.is_file() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "destination is not a regular file",
                    ));
                }
                Some(metadata)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };

        let name = path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid destination file name")
        })?;
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp_name = format!(
            ".{}.hx-{}-{}.tmp",
            name.to_string_lossy(),
            std::process::id(),
            sequence
        );
        let temp_path = parent.join(temp_name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        // Cleanup ownership begins only after create_new succeeds. Never
        // remove a preexisting file when the exclusive create fails.
        let guard = PendingSave(temp_path.clone());
        if let Some(metadata) = previous {
            file.set_permissions(metadata.permissions())?;
        }
        for chunk in self.doc.chunks() {
            file.write_all(chunk.as_bytes())?;
        }
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, &path)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        drop(guard);
        self.path = Some(path);
        self.saved_revision = self.revision;
        Ok(())
    }

    // TODO: doc/selection accessors

    // TODO: be able to take either Rope or RopeSlice
    #[inline]
    pub fn doc(&self) -> &Rope {
        &self.doc
    }

    #[inline]
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    #[inline]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    #[inline]
    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    // pub fn doc<R>(&self, range: R) -> RopeSlice
    // where
    //     R: std::ops::RangeBounds<usize>,
    // {
    //     self.doc.slice(range)
    // }

    // update/transact:
    // update(desc) => transaction ?  transaction.doc() for applied doc
    // transaction.apply(doc)
    // doc.transact(fn -> ... end)

    // replaceSelection (transaction that replaces selection)
    // changeByRange
    // changes
    // slice
    //
    // getters:
    // tabSize
    // indentUnit
    // languageDataAt()
    //
    // config:
    // indentation
    // tabSize
    // lineUnit
    // syntax
    // foldable
    // changeFilter/transactionFilter

    // TODO: move that accepts a boundary matcher fn/list, we keep incrementing until we hit
    // a boundary

    // TODO: edits, does each keypress trigger a full command? I guess it's adding to the same
    // transaction
    // There should be three pieces of the state: current transaction, the original doc, "preview"
    // of the new state.
    // 1. apply the newly generated keypress as a transaction
    // 2. compose onto a ongoing transaction
    // 3. on insert mode leave, that transaction gets stored into undo history

    pub fn move_pos(
        &self,
        pos: usize,
        dir: Direction,
        granularity: Granularity,
        count: usize,
    ) -> usize {
        let text = &self.doc;
        match (dir, granularity) {
            (Direction::Backward, Granularity::Character) => {
                // Clamp to line
                let line = text.char_to_line(pos);
                let start = text.line_to_char(line);
                std::cmp::max(
                    nth_prev_grapheme_boundary(&text.slice(..), pos, count),
                    start,
                )
            }
            (Direction::Forward, Granularity::Character) => {
                // Clamp to line
                let line = text.char_to_line(pos);
                // Line end is pos at the start of next line - 1
                // subtract another 1 because the line ends with \n
                let end = text.line_to_char(line + 1).saturating_sub(2);
                std::cmp::min(nth_next_grapheme_boundary(&text.slice(..), pos, count), end)
            }
            (Direction::Forward, Granularity::Word) => {
                Self::move_next_word_start(&text.slice(..), pos)
            }
            (Direction::Backward, Granularity::Word) => {
                Self::move_prev_word_start(&text.slice(..), pos)
            }
            (_, Granularity::Line) => move_vertically(&text.slice(..), dir, pos, count),
            _ => pos,
        }
    }

    pub fn move_next_word_start(slice: &RopeSlice, mut pos: usize) -> usize {
        // TODO: confirm it's fine without using graphemes, I think it should be
        let ch = slice.char(pos);
        let next = slice.char(pos.saturating_add(1));
        if categorize(ch) != categorize(next) {
            pos += 1;
        }

        // refetch
        let ch = slice.char(pos);

        if is_word(ch) {
            skip_over_next(slice, &mut pos, is_word);
        } else if ch.is_ascii_punctuation() {
            skip_over_next(slice, &mut pos, |ch| ch.is_ascii_punctuation());
        }

        // TODO: don't include newline?
        skip_over_next(slice, &mut pos, |ch| ch.is_ascii_whitespace());

        pos
    }

    pub fn move_prev_word_start(slice: &RopeSlice, mut pos: usize) -> usize {
        // TODO: confirm it's fine without using graphemes, I think it should be
        let ch = slice.char(pos);
        let prev = slice.char(pos.saturating_sub(1)); // TODO: just return original pos if at start

        if categorize(ch) != categorize(prev) {
            pos -= 1;
        }

        // TODO: skip while eol

        // TODO: don't include newline?
        skip_over_prev(slice, &mut pos, |ch| ch.is_ascii_whitespace());

        // refetch
        let ch = slice.char(pos);

        if is_word(ch) {
            skip_over_prev(slice, &mut pos, is_word);
        } else if ch.is_ascii_punctuation() {
            skip_over_prev(slice, &mut pos, |ch| ch.is_ascii_punctuation());
        }

        pos.saturating_add(1)
    }

    pub fn move_next_word_end(slice: &RopeSlice, mut pos: usize, _count: usize) -> usize {
        // TODO: confirm it's fine without using graphemes, I think it should be
        let ch = slice.char(pos);
        let next = slice.char(pos.saturating_add(1));
        if categorize(ch) != categorize(next) {
            pos += 1;
        }

        // TODO: don't include newline?
        skip_over_next(slice, &mut pos, |ch| ch.is_ascii_whitespace());

        // refetch
        let ch = slice.char(pos);

        if is_word(ch) {
            skip_over_next(slice, &mut pos, is_word);
        } else if ch.is_ascii_punctuation() {
            skip_over_next(slice, &mut pos, |ch| ch.is_ascii_punctuation());
        }

        // TODO: stops on spaces

        pos.saturating_sub(1)
    }

    pub fn move_selection(
        &self,
        dir: Direction,
        granularity: Granularity,
        count: usize,
    ) -> Selection {
        // TODO: move all selections according to normal cursor move semantics by collapsing it
        // into cursors and moving them vertically

        self.selection.transform(|range| {
            // let pos = if !range.is_empty() {
            //     // if selection already exists, bump it to the start or end of current select first
            //     if dir == Direction::Backward {
            //         range.from()
            //     } else {
            //         range.to()
            //     }
            // } else {
            let pos = self.move_pos(range.head, dir, granularity, count);
            // };
            Range::new(pos, pos)
        })
    }

    pub fn extend_selection(
        &self,
        dir: Direction,
        granularity: Granularity,
        count: usize,
    ) -> Selection {
        self.selection.transform(|range| {
            let pos = self.move_pos(range.head, dir, granularity, count);
            Range::new(range.anchor, pos)
        })
    }
}

/// Coordinates are a 0-indexed line and column pair.
pub type Coords = (usize, usize); // line, col

/// Convert a character index to (line, column) coordinates.
pub fn coords_at_pos(text: &RopeSlice, pos: usize) -> Position {
    let line = text.char_to_line(pos);
    let line_start = text.line_to_char(line);
    let col = text.slice(line_start..pos).len_chars();
    Position::new(line, col)
}

/// Convert (line, column) coordinates to a character index.
pub fn pos_at_coords(text: &RopeSlice, coords: Position) -> usize {
    let Position { row, col } = coords;
    let line_start = text.line_to_char(row);
    nth_next_grapheme_boundary(text, line_start, col)
}

fn move_vertically(text: &RopeSlice, dir: Direction, pos: usize, count: usize) -> usize {
    let Position { row, col } = coords_at_pos(text, pos);

    let new_line = match dir {
        Direction::Backward => row.saturating_sub(count),
        Direction::Forward => std::cmp::min(row.saturating_add(count), text.len_lines() - 1),
    };

    // convert to 0-indexed, subtract another 1 because len_chars() counts \n
    let new_line_len = text.line(new_line).len_chars().saturating_sub(2);

    let new_col = if new_line_len < col {
        // TODO: preserve horiz here
        new_line_len
    } else {
        col
    };

    pos_at_coords(text, Position::new(new_line, new_col))
}

// used for by-word movement

fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

#[derive(Debug, Eq, PartialEq)]
enum Category {
    Whitespace,
    EOL,
    Word,
    Punctuation,
}
fn categorize(ch: char) -> Category {
    if ch == '\n' {
        Category::EOL
    } else if ch.is_ascii_whitespace() {
        Category::Whitespace
    } else if ch.is_ascii_punctuation() {
        Category::Punctuation
    } else if ch.is_ascii_alphanumeric() {
        Category::Word
    } else {
        unreachable!()
    }
}

fn skip_over_next<F>(slice: &RopeSlice, pos: &mut usize, fun: F)
where
    F: Fn(char) -> bool,
{
    let mut chars = slice.chars_at(*pos);

    for ch in chars {
        if !fun(ch) {
            break;
        }
        *pos += 1;
    }
}

fn skip_over_prev<F>(slice: &RopeSlice, pos: &mut usize, fun: F)
where
    F: Fn(char) -> bool,
{
    // need to +1 so that prev() includes current char
    let mut chars = slice.chars_at(*pos + 1);

    while let Some(ch) = chars.prev() {
        if !fun(ch) {
            break;
        }
        *pos -= 1;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_coords_at_pos() {
        let text = Rope::from("ḧëḷḷö\nẅöṛḷḋ");
        assert_eq!(coords_at_pos(&text.slice(..), 0), (0, 0).into());
        assert_eq!(coords_at_pos(&text.slice(..), 5), (0, 5).into()); // position on \n
        assert_eq!(coords_at_pos(&text.slice(..), 6), (1, 0).into()); // position on w
        assert_eq!(coords_at_pos(&text.slice(..), 7), (1, 1).into()); // position on o
        assert_eq!(coords_at_pos(&text.slice(..), 10), (1, 4).into()); // position on d
    }

    #[test]
    fn test_pos_at_coords() {
        let text = Rope::from("ḧëḷḷö\nẅöṛḷḋ");
        assert_eq!(pos_at_coords(&text.slice(..), (0, 0).into()), 0);
        assert_eq!(pos_at_coords(&text.slice(..), (0, 5).into()), 5); // position on \n
        assert_eq!(pos_at_coords(&text.slice(..), (1, 0).into()), 6); // position on w
        assert_eq!(pos_at_coords(&text.slice(..), (1, 1).into()), 7); // position on o
        assert_eq!(pos_at_coords(&text.slice(..), (1, 4).into()), 10); // position on d
    }

    #[test]
    fn test_vertical_move() {
        let text = Rope::from("abcd\nefg\nwrs");
        let pos = pos_at_coords(&text.slice(..), (0, 4).into());
        let slice = text.slice(..);

        assert_eq!(
            coords_at_pos(&slice, move_vertically(&slice, Direction::Forward, pos, 1)),
            (1, 2).into()
        );
    }
}
