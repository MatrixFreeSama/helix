#![allow(unused)]

mod editor;
mod prompt;

use editor::Editor;

use argh::FromArgs;
use std::path::PathBuf;

use anyhow::Error;

#[derive(FromArgs)]
/// A post-modern text editor.
pub struct Args {
    #[argh(positional)]
    files: Vec<PathBuf>,
}

static EX: smol::Executor = smol::Executor::new();

fn main() -> Result<(), Error> {
    let args: Args = argh::from_env();

    // The interactive editor owns one event loop. Do not start one idle
    // executor per CPU; the OS can migrate this necessary worker dynamically.
    smol::block_on(EX.run(async move {
        let mut editor = editor::Editor::new(args)?;
        editor.run().await
    }))
}
