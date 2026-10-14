use crate::Args;
use helix_core::{state::Mode, syntax::HighlightEvent, Range, State};
use helix_view::{commands, keymap, view::display_grapheme_width, View};

use std::{
    borrow::Cow,
    io::{self, stdout, Write},
    path::PathBuf,
    time::Duration,
};

use smol::prelude::*;

use anyhow::Error;

use crossterm::{
    cursor,
    cursor::position,
    event::{self, read, Event, EventStream, KeyCode, KeyEvent},
    execute, queue,
    style::{Color, Print, SetForegroundColor},
    terminal::{self, disable_raw_mode, enable_raw_mode},
};

use tui::{backend::CrosstermBackend, buffer::Buffer as Surface, layout::Rect, style::Style};

type Terminal = tui::Terminal<CrosstermBackend<std::io::Stdout>>;

static EX: smol::Executor = smol::Executor::new();

pub struct Editor {
    terminal: Terminal,
    view: Option<View>,
    size: (u16, u16),
    surface: Surface,
    cache: Surface,
}

impl Editor {
    pub fn new(mut args: Args) -> Result<Self, Error> {
        let backend = CrosstermBackend::new(stdout());

        let mut terminal = Terminal::new(backend)?;
        let size = terminal::size().unwrap();
        let area = Rect::new(0, 0, size.0, size.1);

        let mut editor = Editor {
            terminal,
            view: None,
            size,
            surface: Surface::empty(area),
            cache: Surface::empty(area),
            // TODO; move to state
        };

        if let Some(file) = args.files.pop() {
            editor.open(file)?;
        }

        Ok(editor)
    }

    pub fn open(&mut self, path: PathBuf) -> Result<(), Error> {
        self.view = Some(View::open(path, self.size)?);
        Ok(())
    }

    fn render(&mut self) {
        use tui::backend::Backend;
        use tui::style::Color;
        // TODO: ideally not mut but highlights require it because of cursor cache
        match &mut self.view {
            Some(view) => {
                let area = Rect::new(0, 0, self.size.0, self.size.1);
                let mut stdout = stdout();
                self.surface.reset(); // reset is faster than allocating new empty surface

                //  clear with background color
                self.surface
                    .set_style(area, view.theme.get("ui.background").into());

                let offset = 5 + 1; // 5 linenr + 1 gutter
                let viewport = Rect::new(
                    offset,
                    0,
                    self.size.0.saturating_sub(offset),
                    self.size.1.saturating_sub(1),
                ); // - 1 for statusline
                if viewport.height == 0 || viewport.width == 0 {
                    return;
                }

                // TODO: inefficient, should feed chunks.iter() to tree_sitter.parse_with(|offset, pos|)
                let source_code = view.state.doc().to_string();

                let last_line = std::cmp::min(
                    (view.first_line + viewport.height - 1) as usize,
                    view.state.doc().len_lines() - 1,
                );

                let range = {
                    // calculate viewport byte ranges
                    let start = view.state.doc().line_to_byte(view.first_line.into());
                    let end = view.state.doc().line_to_byte(last_line)
                        + view.state.doc().line(last_line).len_bytes();

                    start..end
                };

                // TODO: range doesn't actually restrict source, just highlight range

                // TODO: cache highlight results
                // TODO: only recalculate when state.doc is actually modified
                let highlights: Vec<_> = match view.state.syntax.as_mut() {
                    Some(syntax) => {
                        syntax
                            .highlight_iter(source_code.as_bytes(), Some(range), None, |_| None)
                            .unwrap()
                            .collect() // TODO: we collect here to avoid double borrow, fix later
                    }
                    None => vec![Ok(HighlightEvent::Source {
                        start: range.start,
                        end: range.end,
                    })],
                };

                let mut spans = Vec::new();

                let mut visual_x = 0;
                let mut line = 0u16;

                let visible_selections: Vec<Range> = view
                    .state
                    .selection()
                    .ranges()
                    .iter()
                    // TODO: limit selection to one in viewport
                    .filter(|range| !range.is_empty()) // && range.overlaps(&Range::new(start, end + 1))
                    .copied()
                    .collect();

                'outer: for event in highlights {
                    match event.unwrap() {
                        HighlightEvent::HighlightStart(span) => {
                            spans.push(span);
                        }
                        HighlightEvent::HighlightEnd => {
                            spans.pop();
                        }
                        HighlightEvent::Source { start, end } => {
                            // TODO: filter out spans out of viewport for now..

                            let start = view.state.doc().byte_to_char(start);
                            let end = view.state.doc().byte_to_char(end);

                            let text = view.state.doc().slice(start..end);

                            use helix_core::graphemes::RopeGraphemes;

                            let style = match spans.first() {
                                Some(span) => {
                                    view.theme.get(view.theme.scopes()[span.0].as_str()).into()
                                }
                                None => Style::default().fg(Color::Rgb(164, 160, 232)), // lavender
                            };

                            // TODO: we could render the text to a surface, then cache that, that
                            // way if only the selection/cursor changes we can copy from cache
                            // and paint the new cursor.

                            let mut char_index = start;

                            // iterate over range char by char
                            for grapheme in RopeGraphemes::new(&text) {
                                // TODO: track current char_index

                                if grapheme == "\n" {
                                    visual_x = 0;
                                    line += 1;

                                    // TODO: with proper iter this shouldn't be necessary
                                    if line >= viewport.height {
                                        break 'outer;
                                    }
                                } else {
                                    // Borrow from rope chunks when possible.
                                    let grapheme: Cow<str> = Cow::from(grapheme);
                                    let width =
                                        display_grapheme_width(&grapheme, visual_x as usize) as u16;

                                    let style = if visible_selections
                                        .iter()
                                        .any(|range| range.contains(char_index))
                                    {
                                        // cedar
                                        style.clone().bg(Color::Rgb(128, 47, 0))
                                    } else {
                                        style
                                    };

                                    // The viewport is a hard screen boundary. Never hand
                                    // offscreen x coordinates to tui::Buffer (it may panic).
                                    // A partially visible tab is blank-filled; a wide
                                    // grapheme is painted only if all its cells fit.
                                    if visual_x < viewport.width {
                                        let remaining = viewport.width - visual_x;
                                        let screen_x = offset + visual_x;
                                        if grapheme == "\t" {
                                            self.surface.set_stringn(
                                                screen_x,
                                                line,
                                                "    ",
                                                remaining.min(width) as usize,
                                                style,
                                            );
                                        } else if width <= remaining {
                                            self.surface
                                                .set_string(screen_x, line, grapheme, style);
                                        }
                                    }
                                    visual_x = visual_x.saturating_add(width);
                                }

                                char_index += 1;
                            }
                        }
                    }
                }

                let style: Style = view.theme.get("ui.linenr").into();
                let visible_lines = view.first_line as usize..=last_line;
                for (screen_row, document_line) in visible_lines.enumerate() {
                    self.surface.set_stringn(
                        0,
                        screen_row as u16,
                        format!("{:>5}", document_line + 1),
                        5,
                        style,
                    );
                }

                // let lines = state
                //     .doc
                //     .lines_at(self.first_line as usize)
                //     .take(self.size.1 as usize)
                //     .map(|x| x.as_str().unwrap());

                // // iterate over selections and render them
                // let select = Style::default().bg(tui::style::Color::LightBlue);
                // let text = state.doc.slice(..);
                // for range in state.selection.ranges() {
                //     // get terminal coords for x,y for each range pos
                //     // TODO: this won't work with multiline
                //     let (y1, x1) = coords_at_pos(&text, range.from());
                //     let (y2, x2) = coords_at_pos(&text, range.to());
                //     let area = Rect::new(
                //         (x1 + 2) as u16,
                //         y1 as u16,
                //         (x2 - x1 + 1) as u16,
                //         (y2 - y1 + 1) as u16,
                //     );
                //     self.surface.set_style(area, select);

                //     // TODO: don't highlight next char in append mode
                // }

                // statusline
                let mode = match view.state.mode() {
                    Mode::Insert => "INS",
                    Mode::Normal => "NOR",
                };
                self.surface.set_style(
                    Rect::new(0, self.size.1 - 1, self.size.0, 1),
                    view.theme.get("ui.statusline").into(),
                );
                // TODO: unfocused one with different color
                let text_color = Style::default().fg(Color::Rgb(219, 191, 239)); // lilac
                self.surface
                    .set_string(1, self.size.1 - 1, mode, text_color);
                if let Some(path) = view.state.path() {
                    self.surface
                        .set_string(6, self.size.1 - 1, path.to_string_lossy(), text_color);
                }

                self.terminal
                    .backend_mut()
                    .draw(self.cache.diff(&self.surface).into_iter());
                // swap the buffer
                std::mem::swap(&mut self.surface, &mut self.cache);

                // set cursor shape
                match view.state.mode() {
                    Mode::Insert => write!(stdout, "\x1B[6 q"),
                    Mode::Normal => write!(stdout, "\x1B[2 q"),
                };

                // The position is already viewport-relative: do not subtract
                // first_line again, and hide the cursor when it is offscreen.
                let pos = view.state.selection().cursor();
                if let Some(coords) = view.visible_cursor_position(pos, viewport.height) {
                    let x = coords.col.saturating_add(viewport.x as usize);
                    let y = coords.row.saturating_add(viewport.y as usize);
                    if x < self.size.0 as usize && y < viewport.height as usize {
                        execute!(stdout, cursor::Show, cursor::MoveTo(x as u16, y as u16));
                    } else {
                        execute!(stdout, cursor::Hide);
                    }
                } else {
                    execute!(stdout, cursor::Hide);
                }
            }
            None => (),
        }
    }

    pub async fn event_loop(&mut self) {
        let mut reader = EventStream::new();
        let keymap = keymap::default();

        self.render();

        loop {
            // Handle key events
            let mut event = reader.next().await;
            match event {
                Some(Ok(Event::Resize(width, height))) => {
                    self.size = (width, height);
                    let area = Rect::new(0, 0, width, height);
                    self.surface = Surface::empty(area);
                    self.cache = Surface::empty(area);

                    // TODO: simplistic ensure cursor in view for now
                    if let Some(view) = &mut self.view {
                        view.size = self.size;
                        view.ensure_cursor_in_view()
                    };

                    self.render();
                }
                Some(Ok(Event::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    ..
                }))) => {
                    break;
                }
                Some(Ok(Event::Key(event))) => {
                    if let Some(view) = &mut self.view {
                        let mode = view.state.mode();
                        if let Some(command) = keymap::lookup(&keymap, mode, event) {
                            // TODO: handle count other than 1
                            command(view, 1);
                            view.ensure_cursor_in_view();
                            self.render();
                        } else if mode == Mode::Insert {
                            // Arbitrary characters are intentionally not entries in the keymap.
                            // Like the original insert mode, every character is inserted
                            // regardless of modifier bits.
                            if let KeyEvent {
                                code: KeyCode::Char(c),
                                ..
                            } = event
                            {
                                commands::insert_char(view, c);
                                view.ensure_cursor_in_view();
                                self.render();
                            }
                        }
                    }
                }
                Some(Ok(_)) => {
                    // unhandled event
                }
                Some(Err(x)) => panic!(x),
                None => break,
            }
        }
    }

    pub async fn run(&mut self) -> Result<(), Error> {
        enable_raw_mode()?;

        let mut stdout = stdout();

        execute!(stdout, terminal::EnterAlternateScreen)?;

        // Exit the alternate screen and disable raw mode before panicking
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            execute!(std::io::stdout(), terminal::LeaveAlternateScreen);
            disable_raw_mode();
            hook(info);
        }));

        self.event_loop().await;

        // reset cursor shape
        write!(stdout, "\x1B[2 q");

        execute!(stdout, terminal::LeaveAlternateScreen)?;

        disable_raw_mode()?;

        Ok(())
    }
}

// TODO: language configs:
// tabSize, fileExtension etc, mapping to tree sitter parser
// themes:
// map tree sitter highlights to color values
//
// TODO: expand highlight thing so we're able to render only viewport range
// TODO: async: maybe pre-cache scopes as empty so we render all graphemes initially as regular
////text until calc finishes
// TODO: scope matching: biggest union match? [string] & [html, string], [string, html] & [ string, html]
// can do this by sorting our theme matches based on array len (longest first) then stopping at the
// first rule that matches (rule.all(|scope| scopes.contains(scope)))
//
// let visual_x = 0;
// let line = ?;
// for span in spans {
// start(scope) => scopes.push(scope)
//  span =>
//      let text = rope.slice(span.start..span.end);
//      let style = calculate_style(scopes);
//      for each grapheme in text.graphemes() {
//          // if newline += lines, continue
//
//          if state.selection.ranges().any(|range| range.contains(char_index)) {
//              if exactly on cursor {
//              }
//              if on primary cursor? {
//              }
//              modify style temporarily
//          }
//
//          // if in bounds
//
//          // if tab, draw tab width
//          // draw(visual_x, line, grapheme, style)
//          // increment visual_x by grapheme_width(grapheme)
//          // increment char_index by grapheme.len_chars()
//      }
//  end => scopes.pop()
// }
