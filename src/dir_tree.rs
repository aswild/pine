use std::collections::btree_map::{BTreeMap, Entry as BTreeEntry};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

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

    fn write_to<W: Write>(
        &self,
        w: &mut W,
        name: &Path,
        prefix: &str,
        last: bool,
    ) -> io::Result<()> {
        write!(
            w,
            "{prefix}{leader}{name}",
            prefix = prefix,
            leader = if last { "└── " } else { "├── " },
            name = name.display(),
        )?;

        // optionally print symlink target, then complete the line
        match self {
            Entry::Symlink(target) => writeln!(w, " -> {}", target.display())?,
            _ => writeln!(w)?,
        }

        if let Entry::Directory(dir) = self {
            let new_prefix = format!("{}{}", prefix, if last { "    " } else { "│   " });
            let mut it = dir.0.iter().peekable();
            while let Some((name, entry)) = it.next() {
                entry.write_to(w, name, &new_prefix, it.peek().is_none())?;
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

    pub fn write_to<W: Write>(&self, w: &mut W, name: &str) -> io::Result<()> {
        writeln!(w, "{}", name)?;
        let mut it = self.0.iter().peekable();
        while let Some((name, entry)) = it.next() {
            entry.write_to(w, name, "", it.peek().is_none())?;
        }
        Ok(())
    }

    pub fn print(&self, name: &str) {
        let _ = self.write_to(&mut io::stdout().lock(), name);
    }
}

#[cfg(test)]
mod tests {
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
        let dt = crate::make_tree().unwrap();
        let mut v = Vec::<u8>::new();
        dt.write_to(&mut v, "root").unwrap();
        let s = String::from_utf8(v).unwrap();
        assert_eq!(s, expected);
    }
}
