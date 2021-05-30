// Copyright (c) 2021 Allen Wild <allenwild93@gmail.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{crate_version, App, AppSettings, Arg};
use lscolors::LsColors;
use termcolor::{ColorChoice, StandardStream};

mod dir_tree;
mod input;
mod util;
use input::PineTree;

#[derive(Debug)]
struct Args {
    color_choice: ColorChoice,
    inputs: Vec<PathBuf>,
}

fn parse_args() -> Args {
    let m = App::new("pine")
        .about("Display things as a tree.")
        .version(crate_version!())
        .long_version(
            format!(
                "{}\n\
                Copyright (c) 2021 Allen Wild <allenwild93@gmail.com>\n\
                This is free software; you are free to change and redistribute it.\n\
                There is NO WARRANTY, to the extent permitted by law.",
                crate_version!()
            )
            .as_str(),
        )
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
                .multiple(true)
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

    let inputs = m.values_of_os("input").unwrap().map(PathBuf::from).collect();

    Args { color_choice, inputs }
}

fn run() -> Result<()> {
    let args = parse_args();
    let color = LsColors::from_env().unwrap_or_default();
    let stdout = StandardStream::stdout(args.color_choice);
    let mut stdout_lock = stdout.lock();

    for (i, path) in args.inputs.iter().enumerate() {
        if i != 0 {
            writeln!(&mut stdout_lock)?;
        }
        let tree = PineTree::from_path(path)
            .with_context(|| format!("Failed to load tree for '{}'", path.display()))?;
        tree.print(&mut stdout_lock, &color)?;
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}
