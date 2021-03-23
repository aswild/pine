use std::collections::btree_map::{BTreeMap, Entry as BTreeEntry};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use lscolors::{Indicator, LsColors};
use termcolor::WriteColor;

use crate::ToColorSpec;

/// Path::new("foo").parent() == Some("") which is weird and not really what I want.
/// This does the same thing but also returns None if the parent is empty
fn dirname(path: &Path) -> Option<&Path> {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => Some(p),
        _ => None,
    }
}

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

    fn write_styled_name<W>(&self, w: &mut W, name: &Path, color: &LsColors) -> io::Result<()>
    where
        W: Write + WriteColor,
    {
        // TODO: fork lscolors and add a way to color a filename based on extension
        let indicator = match self {
            Entry::File => Indicator::RegularFile,
            Entry::Symlink(_) => Indicator::SymbolicLink,
            Entry::Directory(_) => Indicator::Directory,
        };

        match color.style_for_indicator(indicator).map(|s| s.to_color_spec()) {
            Some(color_spec) => {
                w.set_color(&color_spec)?;
                write!(w, "{}", name.display())?;
                w.reset()?;
            }
            None => write!(w, "{}", name.display())?,
        }

        // optionally print symlink target
        if let Entry::Symlink(target) = self {
            // cheat slightly by recursively calling this function
            write!(w, " -> ")?;
            Entry::write_styled_name(&Entry::File, w, &target, color)?;
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
    IOError(#[from] io::Error),
}

#[derive(Debug, Default)]
pub struct DirTree(BTreeMap<PathBuf, Entry>);

impl DirTree {
    #[inline]
    pub fn insert(&mut self, path: impl AsRef<Path>, entry: Entry) -> Result<(), DirTreeError> {
        self._insert(path.as_ref(), entry)
    }

    fn _insert(&mut self, path: &Path, entry: Entry) -> Result<(), DirTreeError> {
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

                let entry = cur.0.entry(PathBuf::from(comp)).or_insert_with(Entry::default);
                if let Entry::Directory(child_dir) = entry {
                    cur = child_dir;
                } else {
                    let mut err_path = PathBuf::new();
                    for p in dir.iter().take(i + 1) {
                        err_path.push(p);
                    }
                    return Err(DirTreeError::NotADirectory(err_path));
                }
            }
        }

        // now cur is the DirTree that we'll add the final path component to it
        let new_name = PathBuf::from(path.file_name().unwrap());
        if let BTreeEntry::Vacant(slot) = cur.0.entry(new_name) {
            slot.insert(entry);
            Ok(())
        } else {
            Err(DirTreeError::FileExists(PathBuf::from(path)))
        }
    }

    fn write_to<W>(&self, w: &mut W, root: Option<&Path>, color: &LsColors) -> io::Result<()>
    where
        W: Write + WriteColor,
    {
        if let Some(ref root) = root {
            writeln!(w, "{}", root.display())?;
        }

        let mut it = self.0.iter().peekable();
        while let Some((name, entry)) = it.next() {
            entry.write_to(w, name, "", root.is_none(), it.peek().is_none(), color)?;
        }
        Ok(())
    }

    pub fn print_with_root<P, W>(&self, w: &mut W, root: P, color: &LsColors) -> io::Result<()>
    where
        P: AsRef<Path>,
        W: Write + WriteColor,
    {
        self.write_to(w, Some(root.as_ref()), color)
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

    use crate::make_tree;

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

        dt.write_to(&mut v, Some(std::path::Path::new("root")), &color).unwrap();
        let s = String::from_utf8(v.into_inner()).unwrap();
        assert_eq!(s, expected);
    }
}
