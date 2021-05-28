use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use libarchive::{ArchiveError, ArchiveReader};
use lscolors::LsColors;
use termcolor::WriteColor;
use walkdir::WalkDir;

use crate::dir_tree::{DirTree, DirTreeError, Entry};

#[derive(Debug)]
pub enum InputKind {
    Filesystem(PathBuf),
    Archive(PathBuf),
}

#[derive(Debug)]
pub struct PineTree {
    pub kind: InputKind,
    pub tree: DirTree,
}

impl PineTree {
    pub fn new(kind: InputKind) -> Result<Self, DirTreeError> {
        let tree = match &kind {
            InputKind::Filesystem(path) => Self::read_from_filesystem(&path)?,
            InputKind::Archive(path) => Self::read_from_archive(&path)?,
        };
        Ok(Self { kind, tree })
    }

    pub fn print<W>(&self, w: &mut W, color: &LsColors) -> io::Result<()>
    where
        W: Write + WriteColor,
    {
        match &self.kind {
            InputKind::Filesystem(_) => self.tree.print(w, color),
            InputKind::Archive(path) => self.tree.print_with_root(w, path, color),
        }
    }

    fn read_from_filesystem(path: &Path) -> Result<DirTree, DirTreeError> {
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

    fn read_from_archive(path: &Path) -> Result<DirTree, DirTreeError> {
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
                .ok_or_else(|| ArchiveError::new_custom("read an entry that has no path".into()))?;

            let tree_entry = if entry.is_file() {
                Entry::File
            } else if entry.is_symlink() {
                let symlink_path = entry.symlink_path().ok_or_else(|| {
                    ArchiveError::new_custom(format!(
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
}
