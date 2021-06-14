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

/// The parsed directory tree, optionally with a custom root node name (if root is None, then tree
/// usually has only one top-level directory entry)
#[derive(Debug)]
pub struct PineTree {
    pub tree: DirTree,
    pub root: Option<String>,
}

impl PineTree {
    /// Create a PineTree from a filesystem path. If the path is a directory, then walk its
    /// contents. If the path is a file, assume it's an archive and load its contents using
    /// libarchive.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, DirTreeError> {
        let path = path.as_ref();
        let meta = std::fs::metadata(path)?;

        let (tree, root) = if meta.is_dir() {
            (read_from_filesystem(path)?, None)
        } else {
            (read_from_archive(path)?, Some(path.display().to_string()))
        };
        Ok(Self { tree, root })
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

/// Load a DirTree from the libarchive-supported archive file at `path`.
///
/// The `filter` function is called on the full path of every entry in the archive, if it returns
/// false than that entry is skipped. No special handling is done to skip children of directories,
/// the filter function must take care of that if needed.
pub fn read_from_archive_with_filter<F>(path: &Path, filter: F) -> DirTreeResult
where
    F: Fn(&Path) -> bool,
{
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

        if !filter(&entry_path) {
            continue;
        }

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

/// Load a DirTree from the libarchive-supported file at `path`.
pub fn read_from_archive(path: &Path) -> DirTreeResult {
    read_from_archive_with_filter(path, |_| true)
}
