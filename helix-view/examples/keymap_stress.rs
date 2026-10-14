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
    println!("original Vec lookup: {:.3} ms", old_time.as_secs_f64() * 1_000.0);
    println!("borrowed-slice lookup: {:.3} ms", new_time.as_secs_f64() * 1_000.0);
    println!("speedup (old/new): {:.3}x", old_time.as_secs_f64() / new_time.as_secs_f64());

    let mut accepted = 0usize;
    let controls = [
        event(KeyCode::Enter, KeyModifiers::SHIFT),
        event(KeyCode::Esc, KeyModifiers::CONTROL),
        event(KeyCode::Backspace, KeyModifiers::SHIFT),
        event(KeyCode::Delete, KeyModifiers::CONTROL),
    ];
    let start = Instant::now();
    for i in 0..N {
        accepted += black_box(keymap::lookup(
            black_box(&maps),
            Mode::Insert,
            black_box(controls[i % controls.len()]),
        ).is_some()) as usize;
    }
    assert_eq!(accepted, N);
    println!("Insert mode: {N} modified control keys matched in {:.3} ms", start.elapsed().as_secs_f64() * 1_000.0);
}
