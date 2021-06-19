// Copyright (c) 2021 Allen Wild <allenwild93@gmail.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::ffi::OsString;
use std::io::{self, Write};

use anyhow::{anyhow, Result};
use clap::{crate_version, App, AppSettings, Arg};
use lscolors::LsColors;
use termcolor::{ColorChoice, StandardStream};

mod dir_tree;
mod input;
mod package;
mod util;

use crate::input::PineTree;

#[derive(Debug)]
struct Args {
    color_choice: ColorChoice,
    package: bool,
    inputs: Vec<OsString>,
}

fn parse_args() -> Args {
    let m = App::new("pine")
        .about("Print lists of files as a tree.")
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
        .arg(Arg::with_name("package").short("p").long("package").help(
            "List contents of the named Linux packages rather than archives or directories.\n\
            Currently supported package managers: pacman",
        ))
        .arg(
            Arg::with_name("input")
                .required(true)
                .multiple(true)
                .help("path to directory, archive file, or package name"),
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

    Args {
        color_choice,
        package: m.is_present("package"),
        inputs: m.values_of_os("input").unwrap().map(OsString::from).collect(),
    }
}

fn run() -> Result<i32> {
    let args = parse_args();
    let color = LsColors::from_env().unwrap_or_default();
    let stdout = StandardStream::stdout(args.color_choice);
    let mut stdout_lock = stdout.lock();

    let package_manager = match args.package {
        true => Some(crate::package::default_package_manager()?),
        false => None,
    };

    // un-break libarchive's non-ascii pathname handling
    libarchive::fix_posix_locale_for_libarchive();

    let mut error_count = 0;
    let mut first = true;
    for input in args.inputs.iter() {
        // print blank lines between entries
        if first {
            first = false;
        } else {
            writeln!(&mut stdout_lock)?;
        }

        let tree_ret = if let Some(pm) = &package_manager {
            if let Some(pkgname) = input.to_str() {
                match pm.read_package(pkgname) {
                    Ok(Some(tree)) => Ok(tree),
                    Ok(None) => Err(anyhow!("package not found")),
                    Err(e) => Err(e.into()),
                }
            } else {
                Err(anyhow!("package name is not valid UTF-8"))
            }
        } else {
            // map DirTreeError into Anyhow::Error
            PineTree::from_path(input).map_err(Into::into)
        };

        match tree_ret {
            Ok(tree) => tree.print(&mut stdout_lock, &color)?,
            Err(e) => {
                eprintln!("Error: {}: {:#}", input.to_string_lossy(), e);
                error_count += 1;
            }
        }
    }

    Ok(error_count)
}

fn main() {
    fn is_epipe(err: &anyhow::Error) -> bool {
        if let Some(ioe) = err.downcast_ref::<io::Error>() {
            if ioe.kind() == io::ErrorKind::BrokenPipe {
                return true;
            }
        }
        false
    }

    match run() {
        Ok(0) => (),
        Ok(_) => std::process::exit(1),
        Err(e) if is_epipe(&e) => (),
        Err(e) => {
            eprintln!("Error: {:#}", e);
            std::process::exit(1);
        }
    }
}
