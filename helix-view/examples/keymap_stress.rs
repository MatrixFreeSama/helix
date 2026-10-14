// Reproduce the old normal-mode Vec lookup against the new borrowed lookup.
// No threshold: shared-host throughput varies, so this reports measured data
// and checks semantic parity rather than inventing a fixed performance win.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use helix_core::state::Mode;
use helix_view::keymap;
use std::hint::black_box;
use std::time::Instant;

fn event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent { code, modifiers }
}

fn old_normal(maps: &keymap::Keymaps, event: KeyEvent) -> bool {
    let keys = vec![event];
    maps[&Mode::Normal].get(&keys).is_some()
}

fn new_normal(maps: &keymap::Keymaps, event: KeyEvent) -> bool {
    keymap::lookup(maps, Mode::Normal, event).is_some()
}

fn original_insert(_maps: &keymap::Keymaps, event: KeyEvent) -> bool {
    matches!(
        event.code,
        KeyCode::Esc | KeyCode::Backspace | KeyCode::Delete | KeyCode::Enter
    )
}

fn rebuilt_insert(maps: &keymap::Keymaps, event: KeyEvent) -> bool {
    keymap::lookup(maps, Mode::Insert, event).is_some()
}

fn run(
    iterations: usize,
    keys: &[KeyEvent],
    callback: impl Fn(&keymap::Keymaps, KeyEvent) -> bool,
    maps: &keymap::Keymaps,
) -> (std::time::Duration, usize) {
    let started = Instant::now();
    let mut matched = 0usize;
    for i in 0..iterations {
        let event = black_box(keys[i % keys.len()]);
        matched += black_box(callback(black_box(maps), event)) as usize;
    }
    (started.elapsed(), matched)
}

fn main() {
    const N: usize = 3_000_000;
    let maps = keymap::default();
    let keys = [
        event(KeyCode::Char('h'), KeyModifiers::NONE),
        event(KeyCode::Char('j'), KeyModifiers::NONE),
        event(KeyCode::Char('i'), KeyModifiers::NONE),
        event(KeyCode::Char('w'), KeyModifiers::NONE),
        event(KeyCode::Char('z'), KeyModifiers::NONE),
    ];
    let (old_time, old_matches) = run(N, &keys, old_normal, &maps);
    let (new_time, new_matches) = run(N, &keys, new_normal, &maps);
    assert_eq!(old_matches, new_matches);
    assert_eq!(old_matches, 2_400_000);
    println!("Normal mode: {N} input events, matching checksums = {old_matches}");
    println!(
        "original Vec lookup: {:.3} ms",
        old_time.as_secs_f64() * 1_000.0
    );
    println!(
        "borrowed-slice lookup: {:.3} ms",
        new_time.as_secs_f64() * 1_000.0
    );
    println!(
        "speedup (old/new): {:.3}x",
        old_time.as_secs_f64() / new_time.as_secs_f64()
    );

    let insert_events = [
        event(KeyCode::Enter, KeyModifiers::SHIFT),
        event(KeyCode::Esc, KeyModifiers::CONTROL),
        event(KeyCode::Backspace, KeyModifiers::SHIFT),
        event(KeyCode::Delete, KeyModifiers::CONTROL),
        event(KeyCode::Char('a'), KeyModifiers::NONE),
        event(KeyCode::Char('x'), KeyModifiers::SHIFT),
    ];
    let (old_insert_time, old_insert_matches) = run(N, &insert_events, original_insert, &maps);
    let (new_insert_time, new_insert_matches) = run(N, &insert_events, rebuilt_insert, &maps);
    assert_eq!(old_insert_matches, new_insert_matches);
    assert_eq!(old_insert_matches, 2_000_000);
    println!("Insert mode: {N} input events, matching checksums = {old_insert_matches}");
    println!(
        "original direct match: {:.3} ms",
        old_insert_time.as_secs_f64() * 1_000.0
    );
    println!(
        "rebuilt central match: {:.3} ms",
        new_insert_time.as_secs_f64() * 1_000.0
    );
    println!(
        "insert speedup (old/new): {:.3}x",
        old_insert_time.as_secs_f64() / new_insert_time.as_secs_f64()
    );
}
