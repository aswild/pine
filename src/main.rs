// Copyright (c) 2021 Allen Wild <allenwild93@gmail.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use anyhow::{anyhow, Context, Result};
use clap::{crate_version, value_parser, AppSettings, Arg};
use libc::c_int;
use lscolors::LsColors;
use termcolor::{ColorChoice, StandardStream};

mod dir_tree;
mod input;
mod package;
mod util;

use crate::input::PineTree;

#[derive(Debug)]
enum InputMode {
    Path,
    Package,
    TextList(bool),
}

#[derive(Debug)]
struct Args {
    color_choice: ColorChoice,
    pager: bool,
    input_mode: InputMode,
    inputs: Vec<OsString>,
}

fn parse_args() -> Args {
    let m = clap::Command::new("pine")
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
        .setting(AppSettings::DeriveDisplayOrder)
        .arg(
            Arg::new("color")
                .long("color")
                .takes_value(true)
                .value_parser(["auto", "always", "never"])
                .default_value("auto")
                .help("enable terminal colors"),
        )
        .arg(
            Arg::new("always_color")
                .short('C')
                .overrides_with("color")
                .help("alias for --color=always"),
        )
        .arg(
            Arg::new("pager")
                .short('P')
                .long("pager")
                .takes_value(false)
                .help("Send output to a pager, either $PINE_PAGER, $PAGER, or `less`"),
        )
        .arg(Arg::new("package").short('p').long("package").help(
            "List contents of the named Linux packages rather than archives or directories.\n\
            Currently supported package managers: pacman, dpkg",
        ))
        .arg(
            Arg::new("text_listing")
                .short('t')
                .long("text-listing")
                .conflicts_with("package")
                .help("Read a newline-separated list of file and directory names"),
        )
        .arg(Arg::new("check_filesystem").short('F').long("check-filesystem").help(
            "When combined with --text-listing, look for file types and symlink targets by \
             checking the files on disk. Note this will call lstat() on each line of input. \
             Non-absolute paths will be resolved relative to the current working directory.",
        ))
        .arg(
            Arg::new("input")
                .required(true)
                .multiple_values(true)
                .value_parser(value_parser!(OsString))
                .help("path to directory, archive file, or package name. Use '-' to read stdin."),
        )
        .get_matches();

    let color_choice = if m.contains_id("always_color") {
        ColorChoice::Always
    } else {
        match m.get_one("color").map(String::as_str) {
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

    let input_mode = if m.contains_id("package") {
        InputMode::Package
    } else if m.contains_id("text_listing") {
        InputMode::TextList(m.contains_id("check_filesystem"))
    } else {
        InputMode::Path
    };

    Args {
        color_choice,
        pager: m.contains_id("pager"),
        input_mode,
        inputs: m.get_many("input").unwrap().cloned().collect(),
    }
}

fn run() -> Result<i32> {
    // un-break libarchive's non-ascii pathname handling
    libarchive::fix_posix_locale_for_libarchive();

    let args = parse_args();
    let color = LsColors::from_env().unwrap_or_default();

    // evil stdout redirection into a pager process
    let pager_redirect = if args.pager { Some(PagerOutputRedirect::spawn()?) } else { None };

    let stdout = StandardStream::stdout(args.color_choice);
    let mut stdout_lock = stdout.lock();

    let package_manager = match args.input_mode {
        InputMode::Package => Some(crate::package::default_package_manager()?),
        _ => None,
    };

    let mut error_count = 0;
    let mut first = true;
    for input in args.inputs.iter() {
        // print blank lines between entries
        if first {
            first = false;
        } else {
            writeln!(&mut stdout_lock)?;
        }

        let tree_ret = match args.input_mode {
            InputMode::Package => {
                if let Some(pkgname) = input.to_str() {
                    match package_manager.as_ref().unwrap().read_package(pkgname) {
                        Ok(Some(tree)) => Ok(tree),
                        Ok(None) => Err(anyhow!("package not found")),
                        Err(e) => Err(e.into()),
                    }
                } else {
                    Err(anyhow!("package name is not valid UTF-8"))
                }
            }
            InputMode::Path => PineTree::from_path(input).map_err(Into::into),
            InputMode::TextList(check_fs) => {
                PineTree::from_text_listing_path(input, check_fs).map_err(Into::into)
            }
        };

        match tree_ret {
            Ok(tree) => tree.print(&mut stdout_lock, &color)?,
            Err(e) => {
                let input_name = if input == "-" {
                    std::borrow::Cow::Borrowed("[stdin]")
                } else {
                    input.to_string_lossy()
                };
                eprintln!("Error: {}: {:#}", input_name, e);
                error_count += 1;
            }
        }
    }

    if let Some(mut p) = pager_redirect {
        // flush the output buffer before pager_redirect is dropped and waits for the pager process
        let _ = stdout_lock.flush();
        p.wait()?;
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

/// Evil (lazy) stdout redirect hackery. Termcolor doesn't have public APIs like StandardStream
/// that accept ColorChoice and do that logic, only Ansi (writer that always colors) or NoColor
/// (writer that never colors). So either we add two layers of abstraction and reimplement
/// termcolor's ColorChoice logic, or we do some hacks.
///
/// Here, we have the hacks. Creating a PagerOutputRedirect spawns a child process, and then
/// redirects stdout (file descriptor 1) to the write side of that child's pipe, all writes to
/// stdout (TODO stderr too) will go to the pager process.
///
/// When the PagerOutputRedirect is dropped (or the close() method is called), we restore the
/// original stdout and wait for the pager process to exit.
///
/// This is evil because it globally affects the entire process, but it works. There's unsafe code
/// for the libc calls, but we're only messing with file descriptors so there's no meaningful
/// memory safety risks.
struct PagerOutputRedirect {
    child: Child,
    saved_stdout_fd: Option<c_int>,
}

impl Drop for PagerOutputRedirect {
    fn drop(&mut self) {
        if let Err(e) = self.wait() {
            eprintln!("Error: failed to wait for pager during drop: {:#}", e);
        }
    }
}

impl PagerOutputRedirect {
    fn spawn() -> Result<Self> {
        // what pager should we use?
        let pager = env::var_os("PINE_PAGER")
            .unwrap_or_else(|| env::var_os("PAGER").unwrap_or_else(|| "less".into()));

        // Spawn the pager as a child process. Do this before fiddling with our own file
        // descriptors below so that the pager process doesn't inherit any extras.
        let mut cmd = Command::new(&pager);
        cmd.stdin(Stdio::piped());
        if Path::new(&pager).file_stem().map(|s| s.to_str()) == Some(Some("less")) {
            // for less, enable the option to quit on one screen of text (buggy before less version
            // 530, but ignore that and assume a reasonably recent less version)
            cmd.arg("--quit-if-one-screen");
        }
        let child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn pager '{}'", pager.to_string_lossy()))?;

        // and now for the evil part: rather than reimplementing a bunch of internal termcolor code
        // from ColorChoice and StandardStream to handle whether or not to use or ignore output, we
        // just use dup2 to redirect all of our stdout to this new child process!
        // This is unsafe because of the libc FFI calls, but we're just throwing around file
        // descriptors so this can't really cause memory safety issues.
        //
        // First, dup the current stdout to a new file descriptor so we can restore it later
        let saved_stdout_fd = match unsafe { libc::dup(libc::STDOUT_FILENO) } {
            -1 => {
                eprintln!("Error: failed to dup() stdout");
                None
            }
            fd => Some(fd),
        };

        // now, get the child's stdin file descriptor (the write end of our pipe) and dup it to
        // stdout, so that all further stdout writes from Rust code are sent to the pager
        let ret =
            unsafe { libc::dup2(child.stdin.as_ref().unwrap().as_raw_fd(), libc::STDOUT_FILENO) };
        if ret == -1 {
            eprintln!("Error: failed to dup2() the pager's stdin to our stdout");
        }

        Ok(Self { child, saved_stdout_fd })
    }

    fn wait(&mut self) -> Result<()> {
        // At this point we have two file descriptors that both point to out write end of the
        // child's stdin pipe, whatever fd is inside the Child struct (from when process::Command
        // ran pipe() initially), and our current stdout fd (1) that we dup'd to a copy of it.
        // We must close both of those before waiting for the pager process to exit (so that the
        // pager knows it's done reading).
        if let Some(fd) = self.saved_stdout_fd {
            // restore our backup, which will close the current stdout
            let ret = unsafe { libc::dup2(fd, libc::STDOUT_FILENO) };
            if ret == -1 {
                eprintln!("Error: failed to dup2() to restore the original stdout {}", fd);
            }
            // close the backup fd and clear it out
            if unsafe { libc::close(fd) } == -1 {
                eprintln!("Error: failed to close stdout backup fd {}", fd);
            }
            self.saved_stdout_fd = None;
        }

        // now wait for the child process, this will automatically close child.stdin before waiting
        let status = self.child.wait().context("failed to wait for pager process")?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow!("pager process returned {}", status))
        }
    }
}
