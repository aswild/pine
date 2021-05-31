// Copyright (c) 2021 Allen Wild <allenwild93@gmail.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::io;
use std::path::PathBuf;

use if_chain::if_chain;

use crate::dir_tree::DirTreeResult;

trait Package {
    fn build_contents(&self) -> DirTreeResult;
}

trait PackageManager {
    /// The type of package this manager produces
    type Pkg: Package;

    /// Find the package with the given name. Return Ok(None) for package not found, and Err(...)
    /// for a failure to find or read the package database.
    fn find(&self, name: &str) -> Result<Option<Self::Pkg>, io::Error>;
}

pub fn read_from_package(name: &str) -> DirTreeResult {
    read_from_package_manager(&Pacman::new(), name)
}

fn read_from_package_manager<M: PackageManager>(manager: &M, name: &str) -> DirTreeResult {
    let pkg = manager
        .find(name)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "package not found"))?;
    pkg.build_contents()
}

#[derive(Debug)]
struct PacmanPackage {
    path: PathBuf,
}

impl Package for PacmanPackage {
    fn build_contents(&self) -> DirTreeResult {
        let mut mtree_path = self.path.clone();
        mtree_path.push("mtree");
        crate::input::read_from_archive(&mtree_path)
    }
}

#[derive(Debug)]
struct Pacman {
    db_path: PathBuf,
}

impl Pacman {
    /// Default database path location. Should be a `&'static Path` but I can't figure out how to
    /// make one const.
    const DEFAULT_DB_PATH: &'static str = "/var/lib/pacman/local";

    pub fn new() -> Self {
        Self::with_db_path(Self::DEFAULT_DB_PATH)
    }

    pub fn with_db_path(path: impl Into<PathBuf>) -> Self {
        Self { db_path: path.into() }
    }
}

impl PackageManager for Pacman {
    type Pkg = PacmanPackage;

    fn find(&self, name: &str) -> Result<Option<Self::Pkg>, io::Error> {
        for dirent in fs::read_dir(&self.db_path)?.flatten() {
            if_chain! {
                if dirent.file_type()?.is_dir();
                if let Ok(filename) = dirent.file_name().into_string();
                if let Some(pkgname) = filename.rsplitn(3, '-').nth(2);
                if pkgname == name;
                then {
                    return Ok(Some(PacmanPackage { path: dirent.path() }));
                }
            }
        }
        Ok(None)
    }
}
