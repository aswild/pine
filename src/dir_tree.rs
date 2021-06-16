// Copyright (c) 2021 Allen Wild <allenwild93@gmail.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::btree_map::{BTreeMap, Entry as BTreeEntry};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use libarchive::ArchiveError;
use lscolors::{Indicator, LsColors};
use termcolor::WriteColor;

use crate::util::*;

pub type DirTreeResult = Result<DirTree, DirTreeError>;

#[derive(Debug)]
pub enum Entry {
    File,
    Symlink(PathBuf),
    Directory(DirTree),
}

impl Default for Entry {
    fn default() -> Self {
        Self::empty_dir()
    }
}

impl Entry {
    pub fn empty_dir() -> Self {
        Self::Directory(Default::default())
    }

    /// Write a colored version of `name` to the specified Writer. Files are colored based on file
    /// extensions, directories as such, and symlinks also write the target, formatted as a file
    /// name based on extension.
    fn write_styled_name<W>(&self, w: &mut W, name: &Path, color: &LsColors) -> io::Result<()>
    where
        W: Write + WriteColor,
    {
        let style = if w.supports_color() {
            match self {
                // we can't create a std::fs::Metadata, but passing None makes lscolors assume
                // a regular file to be styled by file extension
                Entry::File => color.style_for_path_with_metadata(name, None),
                // for symlinks and directories, get a style based on that indicator type
                Entry::Symlink(_) => color.style_for_indicator(Indicator::SymbolicLink),
                Entry::Directory(_) => color.style_for_indicator(Indicator::Directory),
            }
        } else {
            // bypass lscolors processing if the output stream has color disabled
            None
        };

        match style.map(ToColorSpec::to_color_spec) {
            Some(cs) => {
                w.set_color(&cs)?;
                write!(w, "{}", name.display())?;
                w.reset()?;
            }
            None => write!(w, "{}", name.display())?,
        }

        // optionally print symlink target
        if let Entry::Symlink(target) = self {
            // cheat slightly by recursively calling this function
            write!(w, " -> ")?;
            Entry::File.write_styled_name(w, &target, color)?;
        }

        Ok(())
    }

    fn write_to<W>(
        &self,
        w: &mut W,
        name: &Path,
        prefix: &str,
        root_entry: bool,
        last_in_dir: bool,
        color: &LsColors,
    ) -> io::Result<()>
    where
        W: Write + WriteColor,
    {
        write!(
            w,
            "{prefix}{leader}",
            prefix = prefix,
            leader = if root_entry {
                ""
            } else if last_in_dir {
                "└── "
            } else {
                "├── "
            },
        )?;
        self.write_styled_name(w, name, color)?;
        writeln!(w)?;

        if let Entry::Directory(dir) = self {
            let new_prefix = format!(
                "{}{}",
                prefix,
                if root_entry {
                    ""
                } else if last_in_dir {
                    "    "
                } else {
                    "│   "
                }
            );
            let mut it = dir.0.iter().peekable();
            while let Some((name, entry)) = it.next() {
                entry.write_to(w, name, &new_prefix, false, it.peek().is_none(), color)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DirTreeError {
    #[error("intermediate path `{0}` is not a directory")]
    NotADirectory(PathBuf),
    #[error("file exists `{0}`")]
    FileExists(PathBuf),
    #[error("invalid path component `{0}`")]
    InvalidPath(PathBuf),
    #[error("{0}")]
    BadEntry(String),
    #[error(transparent)]
    IOError(#[from] io::Error),
    #[error(transparent)]
    ArchiveError(#[from] ArchiveError),
}

#[derive(Debug, Default)]
pub struct DirTree(BTreeMap<PathBuf, Entry>);

impl DirTree {
    /// Insert a new entry into the DirTree. Returns FileExists if the final path component already
    /// exists, or NotADirectory if an intermediate path component exists that's not a directory.
    #[inline]
    pub fn insert(&mut self, path: impl AsRef<Path>, entry: Entry) -> Result<(), DirTreeError> {
        self._insert(path.as_ref(), entry, false)
    }

    /// Insert a new entry or replace an existing entry in the DirTree. If the full path exists,
    /// the existing entry is replaced. If an intermediate path component exists that's not
    /// a directory, it's replaced with a directory. In both cases, the old entry is discarded.
    #[inline]
    pub fn replace(&mut self, path: impl AsRef<Path>, entry: Entry) -> Result<(), DirTreeError> {
        self._insert(path.as_ref(), entry, true)
    }

    fn _insert(
        &mut self,
        path: &Path,
        new_entry: Entry,
        replace: bool,
    ) -> Result<(), DirTreeError> {
        let mut cur = self;
        if let Some(dir) = dirname(path) {
            for (i, comp) in dir.components().enumerate() {
                let comp = match comp {
                    // skip prefix (windows), root (/), and current dir (.) path components
                    Component::Prefix(_) | Component::RootDir | Component::CurDir => continue,
                    // we want normal pathname components
                    Component::Normal(c) => c,
                    // we can't handle '..' in paths, no good way to walk back up the tree
                    Component::ParentDir => {
                        return Err(DirTreeError::InvalidPath(comp.as_os_str().into()))
                    }
                };

                let entry = cur.0.entry(PathBuf::from(comp)).or_insert_with(Entry::empty_dir);
                if let Entry::Directory(child_dir) = entry {
                    // Intermediate path component is a directory as expected
                    cur = child_dir;
                } else if replace {
                    // Intermediate path component isn't a directory, clobber it, discarding
                    // whatever used to be there.
                    let _ = std::mem::replace(entry, Entry::empty_dir());
                    // Slightly ugly match because we need a mut ref to the inside of the entry
                    // after it's created and we can't borrow directly from an enum variant without
                    // matching.
                    cur = match entry {
                        Entry::Directory(child_dir) => child_dir,
                        _ => unreachable!(),
                    };
                } else {
                    // Intermediate path component isn't a directory, return an error.
                    return Err(DirTreeError::NotADirectory(dir.iter().take(i + 1).collect()));
                }
            }
        }

        // now cur is the DirTree that we'll add the final path component to it
        let new_name = PathBuf::from(path.file_name().unwrap());
        if replace {
            cur.0.insert(new_name, new_entry);
        } else if let BTreeEntry::Vacant(slot) = cur.0.entry(new_name) {
            slot.insert(new_entry);
        } else {
            return Err(DirTreeError::FileExists(path.into()));
        }
        Ok(())
    }

    fn write_to<W>(&self, w: &mut W, root: Option<&str>, color: &LsColors) -> io::Result<()>
    where
        W: Write + WriteColor,
    {
        if let Some(ref root) = root {
            writeln!(w, "{}", root)?;
        }

        let mut it = self.0.iter().peekable();
        while let Some((name, entry)) = it.next() {
            entry.write_to(w, name, "", root.is_none(), it.peek().is_none(), color)?;
        }
        Ok(())
    }

    pub fn print_with_root<W>(&self, w: &mut W, root: &str, color: &LsColors) -> io::Result<()>
    where
        W: Write + WriteColor,
    {
        self.write_to(w, Some(root), color)
    }

    pub fn print<W>(&self, w: &mut W, color: &LsColors) -> io::Result<()>
    where
        W: Write + WriteColor,
    {
        self.write_to(w, None, color)
    }
}

#[cfg(test)]
mod tests {
    use lscolors::LsColors;
    use termcolor::NoColor;

    use super::{DirTree, DirTreeResult, Entry};

    fn make_tree() -> DirTreeResult {
        let mut dt = DirTree::default();
        dt.insert("./foo", Entry::empty_dir())?;
        dt.insert("foo/bar", Entry::File)?;
        dt.insert("foo/baz", Entry::Symlink("symlink target".into()))?;
        dt.insert("foo/subdir", Entry::empty_dir())?;
        dt.insert("foo/subdir2/subdir3/subdir_file", Entry::File)?;
        dt.insert("another_dir/some_file", Entry::File)?;
        dt.insert("zed/asdf/ghjk", Entry::File)?;
        dt.insert("zed/b", Entry::File)?;
        Ok(dt)
    }

    #[test]
    fn test_output() {
        let expected = "\
root
├── another_dir
│   └── some_file
├── foo
│   ├── bar
│   ├── baz -> symlink target
│   ├── subdir
│   └── subdir2
│       └── subdir3
│           └── subdir_file
└── zed
    ├── asdf
    │   └── ghjk
    └── b
";
        let dt = make_tree().unwrap();
        let color = LsColors::empty();
        let mut v = NoColor::new(Vec::<u8>::new());

        dt.write_to(&mut v, Some("root"), &color).unwrap();
        let s = String::from_utf8(v.into_inner()).unwrap();
        assert_eq!(s, expected);
    }
}
