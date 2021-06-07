// Copyright (c) 2021 Allen Wild <allenwild93@gmail.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::dir_tree::{DirTree, DirTreeError};
use crate::input::read_from_archive;

// common types and traits for a generic package manager interface

/// The result of loading a package. The String in the Ok variant represents the actual name of the
/// package to be displayed, which may differ from the input in case of packages aliases/providers.
type PkgLoadResult = Result<(String, DirTree), DirTreeError>;

/// Package types have a way to load their name and a DirTree
trait Package {
    fn build_contents(&self) -> PkgLoadResult;
}

/// Package Managers can find packages
trait PackageManager {
    /// The type of package this manager produces
    type Pkg: Package;

    /// Find the package with the given name. Return Ok(None) for package not found, and Err(...)
    /// for a failure to find or read the package database.
    fn find(&self, name: &str) -> Option<Self::Pkg>;
}

/// Initialize the default package manager (e.g. auto-detected on the system) and find a package
pub fn read_from_package(name: &str) -> PkgLoadResult {
    // for now, only pacman is supported
    read_from_package_manager(&Pacman::new()?, name)
}

/// Find a package from a particular package manager
fn read_from_package_manager<M: PackageManager>(manager: &M, name: &str) -> PkgLoadResult {
    let pkg = manager
        .find(name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "package not found"))?;
    pkg.build_contents()
}

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
        for dirent in
            fs::read_dir(path)?.flatten().filter(|d| matches!(d.file_type(), Ok(t) if t.is_dir()))
        {
            // look for a desc file in the directory
            let mut desc_path = dirent.path();
            desc_path.push("desc");
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
    type Pkg = PacmanPackage;

    fn find(&self, name: &str) -> Option<Self::Pkg> {
        // look for an exact match
        if let Some(path) = self.packages.get(name) {
            Some(PacmanPackage { name: name.to_string(), path: path.clone() })
        } else {
            // look for a matching provider
            if let Some(real_name) = self.provides.get(name) {
                if let Some(path) = self.packages.get(real_name) {
                    Some(PacmanPackage { name: real_name.to_string(), path: path.clone() })
                } else {
                    // we should never have a provider registered for a package that doesn't exist
                    unreachable!();
                }
            } else {
                None
            }
        }
    }
}

#[derive(Debug)]
struct PacmanPackage {
    /// the package's name
    name: String,
    /// path to the package's directory (that contains the mtree)
    path: PathBuf,
}

impl Package for PacmanPackage {
    fn build_contents(&self) -> PkgLoadResult {
        let tree = read_from_archive(&self.path.join("mtree"))?;
        Ok((self.name.clone(), tree))
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
