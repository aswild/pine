// Copyright (c) 2021 Allen Wild <allenwild93@gmail.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use libarchive::ArchiveReader;
use lscolors::LsColors;
use termcolor::WriteColor;
use walkdir::WalkDir;

use crate::dir_tree::{DirTree, DirTreeError, DirTreeResult, Entry};
use crate::package::read_from_package;

/// The flavors of input that pine can load and generate a tree from
#[derive(Debug)]
pub enum InputKind {
    /// Recursively walk a filesystem directory
    Filesystem(PathBuf),
    /// Load a single archive file in a format supported by libarchive
    Archive(PathBuf),
    /// Load files owned by a Linux distro packge
    Package(String),
}

/// The parsed directory tree, with a link back to the type of input
#[derive(Debug)]
pub struct PineTree {
    tree: DirTree,
    root: Option<String>,
}

impl PineTree {
    pub fn new(kind: InputKind) -> Result<Self, DirTreeError> {
        let mut root = None;
        let tree = match &kind {
            InputKind::Filesystem(path) => read_from_filesystem(&path)?,
            InputKind::Archive(path) => {
                let tree = read_from_archive(&path)?;
                root = Some(path.display().to_string());
                tree
            }
            InputKind::Package(name) => {
                let (pkgname, tree) = read_from_package(&name)?;
                root = Some(pkgname);
                tree
            }
        };
        Ok(Self { tree, root })
    }

    /// Look at a path and determine which sort of input it should be. Assumes that all archives
    /// are files.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, DirTreeError> {
        let path = path.as_ref();
        let meta = std::fs::metadata(path)?;
        let kind = if meta.is_dir() {
            InputKind::Filesystem(path.into())
        } else {
            InputKind::Archive(path.into())
        };
        Self::new(kind)
    }

    /// Build a tree from a distro package
    pub fn from_package(pkg: impl Into<String>) -> Result<Self, DirTreeError> {
        Self::new(InputKind::Package(pkg.into()))
    }

    /// Print our DirTree to a stream. For archives, we have to specify the name of the root node.
    pub fn print<W>(&self, w: &mut W, color: &LsColors) -> io::Result<()>
    where
        W: Write + WriteColor,
    {
        match &self.root {
            Some(root) => self.tree.print_with_root(w, root, color),
            None => self.tree.print(w, color),
        }
    }
}

fn read_from_filesystem(path: &Path) -> DirTreeResult {
    let mut dt = DirTree::default();
    for entry in WalkDir::new(path).min_depth(1) {
        let entry = entry.map_err(|e| DirTreeError::IOError(e.into()))?;

        let filetype = entry.file_type();
        let tree_entry = if filetype.is_file() {
            Entry::File
        } else if filetype.is_symlink() {
            Entry::Symlink(PathBuf::from(entry.file_name()))
        } else if filetype.is_dir() {
            Entry::empty_dir()
        } else {
            unreachable!()
        };

        dt.insert(entry.path(), tree_entry)?;
    }

    Ok(dt)
}

pub fn read_from_archive(path: &Path) -> DirTreeResult {
    let mut dt = DirTree::default();
    let mut archive = ArchiveReader::new(File::open(path)?)?;
    loop {
        let entry = match archive.read_next_header() {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(e) => return Err(e.into()),
        };

        let entry_path = entry
            .path()
            .ok_or_else(|| DirTreeError::BadEntry("libarchive entry has no path".into()))?;

        let tree_entry = if entry.is_file() {
            Entry::File
        } else if entry.is_symlink() {
            let symlink_path = entry.symlink_path().ok_or_else(|| {
                DirTreeError::BadEntry(format!(
                    "Entry '{}' is a symlink but has no symlink path",
                    entry_path.display()
                ))
            })?;
            Entry::Symlink(symlink_path)
        } else if entry.is_dir() {
            Entry::empty_dir()
        } else {
            eprintln!(
                "Oh no: unknown type {:o} for entry '{}'",
                entry.filetype(),
                entry_path.display()
            );
            unreachable!()
        };

        dt.insert(entry_path, tree_entry)?;
    }

    Ok(dt)
}
