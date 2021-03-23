use std::path::PathBuf;

use anyhow::Result;
use lscolors::LsColors;
use structopt::StructOpt;
use termcolor::{ColorChoice, ColorSpec, StandardStream};

mod builders;
mod dir_tree;
use builders::{DirTreeBuild, Filesystem};
use dir_tree::{DirTree, DirTreeError, Entry};

// Printing things with ansi_term involves some weird Cow trait bounds that break things, so I want
// to use termcolor instead. lscolors::Style has a method to convert to an ansi_term::Style, but
// not to a termcolor::ColorSpec, so roll my own conversion with some extension traits.

trait ToColor {
    fn to_color(&self) -> termcolor::Color;
}

impl ToColor for lscolors::style::Color {
    fn to_color(&self) -> termcolor::Color {
        use lscolors::style::Color::*;
        use termcolor::Color;
        match *self {
            Black => Color::Black,
            Red => Color::Red,
            Green => Color::Green,
            Yellow => Color::Yellow,
            Blue => Color::Blue,
            Magenta => Color::Magenta,
            Cyan => Color::Cyan,
            White => Color::White,
            Fixed(x) => Color::Ansi256(x),
            RGB(r, g, b) => Color::Rgb(r, g, b),
        }
    }
}

trait ToColorSpec {
    fn to_color_spec(&self) -> ColorSpec;
}

impl ToColorSpec for lscolors::Style {
    fn to_color_spec(&self) -> ColorSpec {
        let mut cs = ColorSpec::new();
        cs.set_fg(self.foreground.as_ref().map(|c| c.to_color()))
            .set_bg(self.background.as_ref().map(|c| c.to_color()))
            .set_bold(self.font_style.bold)
            .set_dimmed(self.font_style.dimmed)
            .set_italic(self.font_style.italic)
            .set_underline(self.font_style.underline);
        // note: no termcolor properties for blink, reverse, hidden, or strikethrough
        cs
    }
}

// note: used by dir_tree tests, move to test module when no longer used as a placeholder in main
fn make_tree() -> Result<DirTree, DirTreeError> {
    let mut dt = DirTree::default();
    dt.insert("./foo", Entry::empty_dir())?;
    dt.insert("foo/bar", Entry::File)?;
    dt.insert("foo/baz", Entry::Symlink("symlink target".into()))?;
    dt.insert("foo/subdir", Entry::empty_dir())?;
    dt.insert("foo/subdir2/subdir3/subdir_file", Entry::File)?;
    dt.insert("another_dir/some_file", Entry::File)?;
    dt.insert("zed/asdf/ghjk", Entry::File)?;
    dt.insert("zed/b", Entry::File)?;
    Ok(dt)
}

#[derive(Debug, StructOpt)]
struct Args {
    /// Directory to scan
    dir: Option<PathBuf>,
}

fn run() -> Result<()> {
    let args = Args::from_args();
    // TODO: command line args and tty check
    let color = LsColors::from_env().unwrap_or_default();
    let stdout = StandardStream::stdout(ColorChoice::Always);
    let mut stdout_lock = stdout.lock();

    if let Some(ref dir) = args.dir {
        let dt = Filesystem::new(dir).read_dir_tree()?;
        dt.print(&mut stdout_lock, &color)?;
    } else {
        let dt = make_tree()?;
        dt.print_with_root(&mut stdout_lock, "root", &color)?;
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
