// Copyright (c) 2021 Allen Wild <allenwild93@gmail.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, ErrorKind};
use std::path::{Path, PathBuf};

use crate::dir_tree::DirTreeError;
use crate::input::{self, PineTree};

pub trait PackageManager {
    /// Find the package with the given name and load its contents into a PineTree. Return Ok(None)
    /// for package not found, and Err(...) for a failure to find or read the package database.
    /// The PineTree's root should be set to the actual full name of the package, which may be
    /// different due to aliases/providers.
    fn read_package(&self, name: &str) -> Result<Option<PineTree>, DirTreeError>;
}

/// Load the system's package manager databse and parse its package lists.
pub fn default_package_manager() -> Result<Box<dyn PackageManager>, io::Error> {
    if Path::new(Pacman::DEFAULT_DB_PATH).exists() {
        Ok(Box::new(Pacman::new()?))
    } else if Path::new(Dpkg::DEFAULT_DB_PATH).exists() {
        Ok(Box::new(Dpkg::new()?))
    } else {
        Err(io::Error::new(ErrorKind::NotFound, "no supported package manager found"))
    }
}

/********** PACMAN **********/

#[derive(Debug)]
struct Pacman {
    /// Map of pkgname -> folder on disk
    packages: HashMap<String, PathBuf>,
    /// Map of provider name -> real package name
    provides: HashMap<String, String>,
}

impl Pacman {
    /// Default database path location. Should be a `&'static Path` but I can't figure out how to
    /// make one const.
    const DEFAULT_DB_PATH: &'static str = "/var/lib/pacman/local";

    pub fn new() -> Result<Self, io::Error> {
        Self::with_db_path(Self::DEFAULT_DB_PATH.as_ref())
    }

    pub fn with_db_path(path: &Path) -> Result<Self, io::Error> {
        let mut packages = HashMap::new();
        let mut provides = HashMap::new();

        // loop through directory entries
        let dir_iter = fs::read_dir(path).map_err(|err| {
            io::Error::new(err.kind(), format!("no pacman database at '{}'", path.display()))
        })?;
        for dirent in dir_iter.flatten().filter(|d| matches!(d.file_type(), Ok(t) if t.is_dir())) {
            // look for a desc file in the directory
            let desc_path = dirent.path().join("desc");
            if desc_path.is_file() {
                // parse it, warn & continue on errors
                let desc = match PacmanPackageDesc::load(&desc_path) {
                    Ok(desc) => desc,
                    Err(err) => {
                        eprintln!(
                            "WARN: failed to parse desc file '{}': {}",
                            desc_path.display(),
                            err
                        );
                        continue;
                    }
                };

                // add extra providers (before adding to packages so we can move strings around)
                for name in desc.extra_provides.into_iter() {
                    provides.insert(name, desc.name.clone());
                }
                packages.insert(desc.name, desc.path);
            }
        }

        Ok(Self { packages, provides })
    }
}

impl PackageManager for Pacman {
    fn read_package(&self, name: &str) -> Result<Option<PineTree>, DirTreeError> {
        let (real_name, path) = if let Some(path) = self.packages.get(name) {
            // exact pkgname match
            (name, path)
        } else {
            // no exact match, look for a provider
            if let Some(real_name) = self.provides.get(name) {
                if let Some(path) = self.packages.get(real_name) {
                    (real_name.as_str(), path)
                } else {
                    // we should never have a provider registered for a package that doesn't exist
                    unreachable!();
                }
            } else {
                return Ok(None);
            }
        };

        // Path filter excludes the top-level .BUILDINFO and .PKGINFO metadata files. This could be
        // made fancier to ignore the "./" prefix and look for other top-level dotfiles using
        // a regex, but for now this simple version works.
        let path_filter = |path: &Path| {
            !matches!(
                path.to_str(),
                Some("./.BUILDINFO" | "./.PKGINFO" | "./.SRCINFO" | "./.INSTALL")
            )
        };

        let tree = input::read_from_archive_file(&path.join("mtree"), path_filter)?;
        Ok(Some(PineTree { tree, root: Some(real_name.into()) }))
    }
}

/// The results of parsing relevant info out of a pacman localdb's package `desc` file
#[derive(Debug)]
struct PacmanPackageDesc {
    /// the package name
    name: String,
    /// path to the directory that contains desc and mtree files
    path: PathBuf,
    /// additional names that the package provides (does not include this package's name)
    extra_provides: Vec<String>,
}

impl PacmanPackageDesc {
    /// Parse a `desc` file and return the data we care about. This is a pretty barebones parser of
    /// a simple format, and we only extract the fields we care about.
    fn load(desc_path: &Path) -> Result<Self, io::Error> {
        enum State {
            /// looking for a %SECTION% header line we care about
            FindSection,
            /// reading the value from the %NAME% section
            ReadName,
            /// reading values from the %PROVIDES% section
            ReadProvides,
        }

        let mut name: Option<String> = None;
        let mut extra_provides = Vec::new();
        let mut state = State::FindSection;

        // read the whole desc file in one shot (it's fairly small) rather than unwrapping results
        // for reading one line at a time.
        for line in fs::read_to_string(&desc_path)?.lines() {
            match state {
                State::FindSection => match line {
                    "%NAME%" => state = State::ReadName,
                    "%PROVIDES%" => state = State::ReadProvides,
                    _ => (),
                },
                State::ReadName => {
                    if line.is_empty() {
                        state = State::FindSection;
                    } else {
                        name = Some(line.to_string());
                    }
                }
                State::ReadProvides => {
                    if line.is_empty() {
                        state = State::FindSection;
                    } else {
                        // %PROVIDES% entries can be `name` or `name=version`
                        let provides_name = line.split_once('=').map(|pair| pair.0).unwrap_or(line);
                        extra_provides.push(provides_name.to_string());
                    }
                }
            }
        }

        // make sure we got a name
        let name = name.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "no pkgname found in desc")
        })?;

        // pop off the filename component, leaving just the package's directory
        let path = desc_path.parent().unwrap().to_owned();

        Ok(Self { name, path, extra_provides })
    }
}

/********** DPKG **********/

#[derive(Debug, Default)]
struct DpkgStatus {
    name: String,
    arch: String,
    provides: Vec<String>,
    multi_arch_same: bool,
}

impl DpkgStatus {
    fn clear(&mut self) {
        self.name.clear();
        self.arch.clear();
        self.provides.clear();
        self.multi_arch_same = false;
    }

    fn list_path(&self, db_path: &Path) -> Option<PathBuf> {
        if self.name.is_empty() {
            return None;
        }
        let mut path = db_path.join("info");
        if self.multi_arch_same {
            path.push(format!("{}:{}.list", self.name, self.arch));
        } else {
            path.push(format!("{}.list", self.name));
        }
        Some(path)
    }
}

#[derive(Debug)]
struct Dpkg {
    /// Map of pkgname -> .list file on disk
    packages: HashMap<String, PathBuf>,
    /// Map of provider name -> real package name
    provides: HashMap<String, String>,
}

impl Dpkg {
    /// Default database location
    const DEFAULT_DB_PATH: &'static str = "/var/lib/dpkg";

    pub fn new() -> Result<Self, io::Error> {
        Self::with_db_path(Self::DEFAULT_DB_PATH.as_ref())
    }

    /// parse the /var/lib/dpkg/status file and pull out the info we want
    pub fn with_db_path(path: &Path) -> Result<Self, io::Error> {
        let mut packages = HashMap::new();
        let mut provides = HashMap::new();
        let mut current = DpkgStatus::default();

        let info_file = BufReader::new(File::open(&path.join("status"))?);
        for line_ret in info_file.lines() {
            // get one line of text, will not contain the trailing LF
            let line = match line_ret {
                // good line
                Ok(line) => line,
                // invalid UTF-8, just skip it
                Err(e) if e.kind() == ErrorKind::InvalidData => continue,
                // read error, bail
                Err(e) => return Err(e),
            };

            if line.is_empty() {
                // empty lines separate packages, so finish off current and add it
                let list_path = current.list_path(path).ok_or_else(|| {
                    io::Error::new(ErrorKind::InvalidData,
                    "failed parsing dpkg status file: got blank line with no package name defined")
                })?;
                packages.insert(current.name.clone(), list_path);
                for alias in current.provides.iter() {
                    provides.insert(current.name.clone(), alias.to_owned());
                }
                current.clear();
            } else if let Some((key, val)) = line.split_once(": ") {
                // most lines, and all of the ones we care about, look like "Key: some long value"
                match key {
                    "Package" => current.name.push_str(val),
                    "Architecture" => current.arch.push_str(val),
                    "Multi-Arch" => {
                        if val == "same" {
                            current.multi_arch_same = true;
                        }
                    }
                    "Provides" => {
                        for alias in val.split(", ") {
                            // line can look something like
                            // Provides: c++-compiler, g++-x86-64-linux-gnu (= 4:9.3.0-1ubuntu2)
                            // Split on ", " and then stop at the first space to remove the
                            // parenthesized version info
                            let end = alias.find(' ').unwrap_or_else(|| alias.len());
                            current.provides.push(alias[..end].to_owned());
                        }
                    }
                    // skip unused fields
                    _ => (),
                }
            }
            // other non-matching lines (e.g. continuations) are just skipped, we don't care about
            // those fields anyway
        }

        Ok(Self { packages, provides })
    }
}

impl PackageManager for Dpkg {
    fn read_package(&self, name: &str) -> Result<Option<PineTree>, DirTreeError> {
        let (real_name, path) = if let Some(path) = self.packages.get(name) {
            // exact pkgname match
            (name, path)
        } else {
            // no exact match, look for a provider
            if let Some(real_name) = self.provides.get(name) {
                if let Some(path) = self.packages.get(real_name) {
                    (real_name.as_str(), path)
                } else {
                    // we should never have a provider registered for a package that doesn't exist
                    unreachable!();
                }
            } else {
                return Ok(None);
            }
        };

        // read the .list file
        let list_contents = fs::read_to_string(path)?;
        // some package listings start with a line containing "/." which confuses DirTree
        // (Path::new("/.").file_name() == None). Rather than adding extra filtering at the
        // PineTree or DirTree level, just strip it from the beginning since that's the only place
        // it shows up.
        let list_text = list_contents.strip_prefix("/.\n").unwrap_or(&list_contents);

        let mut tree = PineTree::from_text_listing(&list_text, true)?;
        tree.root = Some(real_name.into());
        Ok(Some(tree))
    }
}
