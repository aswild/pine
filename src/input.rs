// Copyright (c) 2021 Allen Wild <allenwild93@gmail.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::{self, File, Metadata};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use libarchive::ArchiveReader;
use lscolors::LsColors;
use termcolor::WriteColor;
use walkdir::WalkDir;

use crate::dir_tree::{DirTree, DirTreeError, DirTreeResult, Entry};

/// Check whether a file's metadata is executable, i.e. whether any of the bits in
/// `S_IXUSR | S_IXGRP | S_IXOTH` are set.
#[inline]
fn is_executable(meta: &Metadata) -> bool {
    (meta.permissions().mode() & 0o111) != 0
}

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

        let (tree, root) = if path == Path::new("-") {
            (read_from_archive(io::stdin(), |_| true)?, None)
        } else {
            let meta = std::fs::metadata(path)?;
            let tree = if meta.is_dir() {
                read_from_filesystem(path)?
            } else {
                read_from_archive_file(path, |_| true)?
            };
            (tree, Some(path.display().to_string()))
        };
        Ok(Self { tree, root })
    }

    /// Create a PineTree from a list of filenames, one per line. All leaf entries are assumed to
    /// be normal files, since there's no way to convey symlink metadata. Any name which appears as
    /// an intermediate path component is assumed to be a directory.
    pub fn from_text_listing(list: &str, check_fs: bool) -> Result<Self, DirTreeError> {
        let mut tree = DirTree::default();
        // strip leading/trailing whitespace from lines and skip blanks
        for line in list.lines().map(str::trim).filter(|s| !s.is_empty()) {
            // when reading filenames from text, we can't know in advanced whether it's supposed
            // to be a file or directory, so assume everything is a file at first, replacing them
            // with directories as needed.
            if line.ends_with('/') {
                // if the path ends with a / then force it to be a directory, even if we'd
                // otherwise be checking the filesystem
                tree.replace(line, Entry::empty_dir())?;
            } else if check_fs {
                // try to stat the path and figure out what sort of file/entry it is
                if let Ok(meta) = fs::symlink_metadata(line) {
                    let ftype = meta.file_type();
                    if ftype.is_file() {
                        let tree_entry =
                            if is_executable(&meta) { Entry::ExecFile } else { Entry::File };
                        tree.replace(line, tree_entry)?;
                    } else if ftype.is_dir() {
                        tree.replace(line, Entry::empty_dir())?;
                    } else if ftype.is_symlink() {
                        let target = fs::read_link(line)
                            .unwrap_or_else(|_| PathBuf::from("[failed to read symlink target]"));
                        tree.replace(line, Entry::Symlink(target))?;
                    } else {
                        unreachable!();
                    }
                } else {
                    // failed to stat the path, just assume it's a file
                    tree.replace(line, Entry::File)?;
                }
            } else {
                tree.replace(line, Entry::File)?;
            }
        }
        Ok(Self { tree, root: None })
    }

    pub fn from_text_listing_path(
        path: impl AsRef<Path>,
        check_fs: bool,
    ) -> Result<Self, DirTreeError> {
        let path = path.as_ref();
        let text = if path == Path::new("-") {
            let mut s = String::new();
            io::stdin().read_to_string(&mut s)?;
            s
        } else {
            fs::read_to_string(path)?
        };
        Self::from_text_listing(&text, check_fs)
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
    let abs_path = path.canonicalize()?;
    let mut dt = DirTree::default();

    for entry in WalkDir::new(&abs_path).min_depth(1) {
        let entry = entry.map_err(|e| DirTreeError::IOError(e.into()))?;

        let filetype = entry.file_type();
        let tree_entry = if filetype.is_file() {
            if let Ok(meta) = entry.metadata() {
                if is_executable(&meta) {
                    Entry::ExecFile
                } else {
                    Entry::File
                }
            } else {
                Entry::File
            }
        } else if filetype.is_symlink() {
            Entry::Symlink(PathBuf::from(entry.file_name()))
        } else if filetype.is_dir() {
            Entry::empty_dir()
        } else {
            unreachable!()
        };

        // since we gave walkdir an absolute path, all the entries will have absolute paths too.
        // Strip off the original path prefix and only include subdirectories in the tree.
        let rela_path = entry.path().strip_prefix(&abs_path).unwrap_or_else(|_| {
            // ugly warning, but I want details if this fails (because it should always work)
            let entry_path = entry.path();
            eprintln!(
                "WARNING: failed to strip abs_path prefix '{}' from entry path '{}'",
                abs_path.display(),
                entry_path.display(),
            );
            entry_path
        });
        dt.insert(rela_path, tree_entry)?;
    }

    Ok(dt)
}

/// Load a DirTree from the libarchive-supported archive stream returned by the reader.
///
/// The `filter` function is called on the full path of every entry in the archive, if it returns
/// false than that entry is skipped. No special handling is done to skip children of directories,
/// the filter function must take care of that if needed.
pub fn read_from_archive<R, F>(reader: R, filter: F) -> DirTreeResult
where
    R: Read,
    F: Fn(&Path) -> bool,
{
    impl_read_from_archive(ArchiveReader::new(reader)?, filter)
}

/// Load a DirTree from the libarchive-supported archive file at path.
///
/// The `filter` works in the same way as [`read_from_archive_with_filter`]
pub fn read_from_archive_file<F>(path: &Path, filter: F) -> DirTreeResult
where
    F: Fn(&Path) -> bool,
{
    let mut file = File::open(path)?;

    // Attempt a no-op seek on the file. If it succeeds, use a seekable archive reader, which is
    // needed for some formats like 7-zip.
    #[allow(clippy::seek_from_current)]
    match file.seek(SeekFrom::Current(0)) {
        Ok(_) => impl_read_from_archive(ArchiveReader::new_seekable(file)?, filter),
        Err(_) => impl_read_from_archive(ArchiveReader::new(file)?, filter),
    }
}

fn impl_read_from_archive<R, F>(mut archive: ArchiveReader<R>, filter: F) -> DirTreeResult
where
    R: Read,
    F: Fn(&Path) -> bool,
{
    let mut dt = DirTree::default();
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

        let tree_entry = if entry.is_exec_file() {
            Entry::ExecFile
        } else if entry.is_file() {
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
                "warning: unknown type/mode {:03o} for entry '{}', assuming File",
                entry.filetype(),
                entry_path.display()
            );
            Entry::File
        };

        dt.insert(entry_path, tree_entry)?;
    }

    Ok(dt)
}
