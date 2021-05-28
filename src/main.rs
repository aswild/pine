use std::path::PathBuf;

use anyhow::Result;
use clap::{crate_version, App, AppSettings, Arg};
use lscolors::LsColors;
use termcolor::{ColorChoice, ColorSpec, StandardStream};

mod builders;
mod dir_tree;
mod util;
use builders::{DirTreeBuild, Filesystem};
use dir_tree::{DirTree, DirTreeError, Entry};

#[derive(Debug)]
struct Args {
    color_choice: ColorChoice,
    input: PathBuf,
}

fn parse_args() -> Args {
    let m = App::new("pine")
        .about("Display things as a tree.")
        .version(crate_version!())
        .setting(AppSettings::ColoredHelp)
        .setting(AppSettings::DeriveDisplayOrder)
        .setting(AppSettings::UnifiedHelpMessage)
        .arg(
            Arg::with_name("color")
                .long("color")
                .takes_value(true)
                .possible_values(&["auto", "always", "never"])
                .default_value("auto")
                .help("enable terminal colors"),
        )
        .arg(
            Arg::with_name("always_color")
                .short("C")
                .overrides_with("color")
                .help("alias for --color=always"),
        )
        .arg(
            Arg::with_name("input")
                .required(true)
                .help("path to directory, archive file, or package to tree"),
        )
        .get_matches();

    let color_choice = if m.is_present("always_color") {
        ColorChoice::Always
    } else {
        match m.value_of("color") {
            Some("always") => ColorChoice::Always,
            Some("never") => ColorChoice::Never,
            Some("auto") => {
                if atty::is(atty::Stream::Stdout) {
                    ColorChoice::Auto
                } else {
                    ColorChoice::Never
                }
            }
            _ => unreachable!(),
        }
    };

    let input = m.value_of_os("input").unwrap().into();

    Args { color_choice, input }
}

fn run() -> Result<()> {
    let args = parse_args();
    let color = LsColors::from_env().unwrap_or_default();
    let stdout = StandardStream::stdout(args.color_choice);

    let mut stdout_lock = stdout.lock();

    let dt = Filesystem::new(args.input).read_dir_tree()?;
    dt.print(&mut stdout_lock, &color)?;

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
