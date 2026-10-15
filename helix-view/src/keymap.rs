use crate::commands::{self, Command};
use helix_core::{hashmap, state};
use std::collections::HashMap;

// Kakoune-inspired:
// mode = {
//      normal = {
//          q = record_macro
//          w = (next) word
//          W = next WORD
//          e = end of word
//          E = end of WORD
//          r =
//          t = 'till char
//          y = yank
//          u = undo
//          U = redo
//          i = insert
//          I = INSERT (start of line)
//          o = open below (insert on new line below)
//          O = open above (insert on new line above)
//          p = paste (before cursor)
//          P = PASTE (after cursor)
//          ` =
//          [ = select to text object start (alt = select whole object)
//          ] = select to text object end
//          { = extend to inner object start
//          } = extend to inner object end
//          a = append
//          A = APPEND (end of line)
//          s = split
//          S = select
//          d = delete()
//          f = find_char()
//          g = goto (gg, G, gc, gd, etc)
//
//          h = move_char_left(n)
//          j = move_line_down(n)
//          k = move_line_up(n)
//          l = move_char_right(n)
//          : = command line
//          ; = collapse selection to cursor
//          " = use register
//          ` = convert case? (to lower) (alt = swap case)
//          ~ = convert to upper case
//          . = repeat last command
//          \ = disable hook?
//          / = search
//          > = indent
//          < = deindent
//          % = select whole buffer (in vim = jump to matching bracket)
//          * = search pattern in selection
//          ( = rotate main selection backward
//          ) = rotate main selection forward
//          - = trim selections? (alt = merge contiguous sel together)
//          @ = convert tabs to spaces
//          & = align cursor
//          ? = extend to next given regex match (alt = to prev)
//
//          in kakoune these are alt-h alt-l / gh gl
//                              select from curs to begin end / move curs to begin end
//          0 = start of line
//          ^ = start of line (first non blank char)
//          $ = end of line
//
//          z = save selections
//          Z = restore selections
//          x = select line
//          X = extend line
//          c = change selected text
//          C = copy selection?
//          v = view menu (viewport manipulation)
//          b = select to previous word start
//          B = select to previous WORD start
//
//
//
//
//
//
//          = = align?
//          + =
//      }
// }

#[cfg(feature = "term")]
pub use crossterm::event::{KeyCode, KeyEvent as Key, KeyModifiers as Modifiers};

// TODO: could be trie based
type Keymap = HashMap<Vec<Key>, Command>;
pub type Keymaps = HashMap<state::Mode, Keymap>;

/// A terminal keystroke resolves to exactly one typed effect. Document
/// commands retain the existing zero-allocation borrowed-slice lookup.
#[cfg(feature = "term")]
#[derive(Clone, Copy)]
pub enum TerminalBinding {
    Document(Command),
    OpenPrompt,
    Quit,
}

#[cfg(feature = "term")]
#[inline]
pub fn resolve_terminal(
    keymaps: &Keymaps,
    mode: state::Mode,
    event: Key,
) -> Option<TerminalBinding> {
    if mode == state::Mode::Normal && event.modifiers == Modifiers::NONE {
        match event.code {
            KeyCode::Char(':') => return Some(TerminalBinding::OpenPrompt),
            KeyCode::Char('q') => return Some(TerminalBinding::Quit),
            _ => {}
        }
    }
    lookup(keymaps, mode, event).map(TerminalBinding::Document)
}

// Centralized dispatch, with zero temporary heap allocation on either mode.
// Insert-mode controls deliberately ignore modifier bits, matching the original
// implementation. The existing normal-mode keymap uses borrowed slice lookup.
#[cfg(feature = "term")]
#[inline]
pub fn lookup(keymaps: &Keymaps, mode: state::Mode, event: Key) -> Option<Command> {
    match mode {
        state::Mode::Insert => match event.code {
            KeyCode::Esc => Some(commands::normal_mode as Command),
            KeyCode::Backspace => Some(commands::delete_char_backward as Command),
            KeyCode::Delete => Some(commands::delete_char_forward as Command),
            KeyCode::Enter => Some(commands::insert_newline as Command),
            KeyCode::Tab => Some(commands::insert_tab as Command),
            _ => None,
        },
        state::Mode::Normal => keymaps
            .get(&mode)?
            .get(std::slice::from_ref(&event))
            .copied(),
        // Goto is a one-key continuation, not another heap-allocated keymap.
        state::Mode::Goto => {
            if event.modifiers != Modifiers::NONE {
                return Some(commands::normal_mode as Command);
            }
            Some(match event.code {
                KeyCode::Char('g') => commands::move_file_start as Command,
                KeyCode::Char('e') => commands::move_file_end as Command,
                _ => commands::normal_mode as Command,
            })
        }
    }
}

pub fn default() -> Keymaps {
    hashmap!(
        state::Mode::Normal =>
            hashmap!(
                vec![Key {
                    code: KeyCode::Char('h'),
                    modifiers: Modifiers::NONE
                }] => commands::move_char_left as Command,
                vec![Key {
                    code: KeyCode::Char('j'),
                    modifiers: Modifiers::NONE
                }] => commands::move_line_down as Command,
                vec![Key {
                    code: KeyCode::Char('k'),
                    modifiers: Modifiers::NONE
                }] => commands::move_line_up as Command,
                vec![Key {
                    code: KeyCode::Char('0'),
                    modifiers: Modifiers::NONE
                }] => commands::move_line_start as Command,
                vec![Key {
                    code: KeyCode::Char('$'),
                    modifiers: Modifiers::NONE
                }] => commands::move_line_end as Command,
                vec![Key {
                    code: KeyCode::Char('l'),
                    modifiers: Modifiers::NONE
                }] => commands::move_char_right as Command,
                vec![Key {
                    code: KeyCode::Char('H'),
                    modifiers: Modifiers::SHIFT
                }] => commands::extend_char_left as Command,
                vec![Key {
                    code: KeyCode::Char('J'),
                    modifiers: Modifiers::SHIFT
                }] => commands::extend_line_down as Command,
                vec![Key {
                    code: KeyCode::Char('K'),
                    modifiers: Modifiers::SHIFT
                }] => commands::extend_line_up as Command,
                vec![Key {
                    code: KeyCode::Char('L'),
                    modifiers: Modifiers::SHIFT
                }] => commands::extend_char_right as Command,
                vec![Key {
                    code: KeyCode::Char('w'),
                    modifiers: Modifiers::NONE
                }] => commands::move_next_word_start as Command,
                vec![Key {
                    code: KeyCode::Char('b'),
                    modifiers: Modifiers::NONE
                }] => commands::move_prev_word_start as Command,
                vec![Key {
                    code: KeyCode::Char('e'),
                    modifiers: Modifiers::NONE
                }] => commands::move_next_word_end as Command,
                vec![Key {
                    code: KeyCode::Char('g'),
                    modifiers: Modifiers::NONE
                }] => commands::goto_mode as Command,
                vec![Key {
                    code: KeyCode::PageUp,
                    modifiers: Modifiers::NONE
                }] => commands::page_up as Command,
                vec![Key {
                    code: KeyCode::PageDown,
                    modifiers: Modifiers::NONE
                }] => commands::page_down as Command,
                vec![Key {
                    code: KeyCode::Char('u'),
                    modifiers: Modifiers::CONTROL
                }] => commands::half_page_up as Command,
                vec![Key {
                    code: KeyCode::Char('d'),
                    modifiers: Modifiers::CONTROL
                }] => commands::half_page_down as Command,
                vec![Key {
                    code: KeyCode::Char('i'),
                    modifiers: Modifiers::NONE
                }] => commands::insert_mode as Command,
                vec![Key {
                    code: KeyCode::Char('I'),
                    modifiers: Modifiers::SHIFT,
                }] => commands::prepend_to_line as Command,
                vec![Key {
                    code: KeyCode::Char('a'),
                    modifiers: Modifiers::NONE
                }] => commands::append_mode as Command,
                vec![Key {
                    code: KeyCode::Char('A'),
                    modifiers: Modifiers::SHIFT,
                }] => commands::append_to_line as Command,
                vec![Key {
                    code: KeyCode::Char('o'),
                    modifiers: Modifiers::NONE
                }] => commands::open_below as Command,
                vec![Key {
                    code: KeyCode::Char('d'),
                    modifiers: Modifiers::NONE
                }] => commands::delete_selection as Command,
                vec![Key {
                    code: KeyCode::Char('c'),
                    modifiers: Modifiers::NONE
                }] => commands::change_selection as Command,
                vec![Key {
                    code: KeyCode::Char('s'),
                    modifiers: Modifiers::NONE
                }] => commands::split_selection_on_newline as Command,
                vec![Key {
                    code: KeyCode::Esc,
                    modifiers: Modifiers::NONE
                }] => commands::normal_mode as Command,
            )
    )
}

#[cfg(all(test, feature = "term"))]
mod test {
    use super::*;

    fn key(code: KeyCode, modifiers: Modifiers) -> Key {
        Key { code, modifiers }
    }

    #[test]
    fn insert_commands_keep_original_modifier_independence() {
        let maps = default();
        let controls = [
            (KeyCode::Esc, commands::normal_mode as Command),
            (
                KeyCode::Backspace,
                commands::delete_char_backward as Command,
            ),
            (KeyCode::Delete, commands::delete_char_forward as Command),
            (KeyCode::Enter, commands::insert_newline as Command),
            (KeyCode::Tab, commands::insert_tab as Command),
        ];
        for (code, expected) in controls.iter().copied() {
            for modifiers in [Modifiers::NONE, Modifiers::SHIFT, Modifiers::CONTROL]
                .iter()
                .copied()
            {
                let resolved = lookup(&maps, state::Mode::Insert, key(code, modifiers)).unwrap();
                assert_eq!(resolved as usize, expected as usize);
            }
        }
    }

    #[test]
    fn normal_commands_still_require_exact_modifiers() {
        let maps = default();
        assert!(lookup(
            &maps,
            state::Mode::Normal,
            key(KeyCode::Char('h'), Modifiers::NONE)
        )
        .is_some());
        assert!(lookup(
            &maps,
            state::Mode::Normal,
            key(KeyCode::Char('h'), Modifiers::SHIFT)
        )
        .is_none());
        assert!(lookup(
            &maps,
            state::Mode::Insert,
            key(KeyCode::Char('x'), Modifiers::NONE)
        )
        .is_none());
    }

    #[test]
    fn goto_is_a_single_continuation_and_falls_back_to_normal() {
        let maps = default();
        for (code, expected) in [
            (KeyCode::Char('g'), commands::move_file_start as Command),
            (KeyCode::Char('e'), commands::move_file_end as Command),
            (KeyCode::Esc, commands::normal_mode as Command),
            (KeyCode::Char('x'), commands::normal_mode as Command),
        ]
        .iter()
        .copied()
        {
            let cmd = lookup(&maps, state::Mode::Goto, key(code, Modifiers::NONE)).unwrap();
            assert_eq!(cmd as usize, expected as usize);
        }
        let cmd = lookup(
            &maps,
            state::Mode::Goto,
            key(KeyCode::Char('g'), Modifiers::CONTROL),
        )
        .unwrap();
        assert_eq!(cmd as usize, commands::normal_mode as Command as usize);
        for code in [KeyCode::PageUp, KeyCode::PageDown] {
            assert!(lookup(&maps, state::Mode::Normal, key(code, Modifiers::NONE)).is_some());
        }
    }

    #[test]
    fn terminal_effects_have_one_mode_aware_dispatch_entry() {
        let maps = default();
        assert!(matches!(
            resolve_terminal(
                &maps,
                state::Mode::Normal,
                key(KeyCode::Char('q'), Modifiers::NONE)
            ),
            Some(TerminalBinding::Quit)
        ));
        assert!(matches!(
            resolve_terminal(
                &maps,
                state::Mode::Normal,
                key(KeyCode::Char(':'), Modifiers::NONE)
            ),
            Some(TerminalBinding::OpenPrompt)
        ));
        assert!(resolve_terminal(
            &maps,
            state::Mode::Insert,
            key(KeyCode::Char('q'), Modifiers::NONE)
        )
        .is_none());
        assert!(resolve_terminal(
            &maps,
            state::Mode::Goto,
            key(KeyCode::Char('q'), Modifiers::NONE)
        )
        .is_some());
    }

    #[test]
    fn million_events_match_expected_dispatch() {
        let maps = default();
        let mut commands = 0usize;
        for i in 0..1_000_000 {
            let (mode, event) = match i % 4 {
                0 => (
                    state::Mode::Normal,
                    key(KeyCode::Char('h'), Modifiers::NONE),
                ),
                1 => (state::Mode::Insert, key(KeyCode::Enter, Modifiers::SHIFT)),
                2 => (
                    state::Mode::Insert,
                    key(KeyCode::Backspace, Modifiers::CONTROL),
                ),
                _ => (
                    state::Mode::Insert,
                    key(KeyCode::Char('a'), Modifiers::NONE),
                ),
            };
            commands += lookup(&maps, mode, event).is_some() as usize;
        }
        assert_eq!(commands, 750_000);
    }
}
