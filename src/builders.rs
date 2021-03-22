use std::path::PathBuf;

use walkdir::WalkDir;

use crate::dir_tree::{DirTree, DirTreeError, Entry};

pub trait DirTreeBuild {
    fn read_dir_tree(&self) -> Result<DirTree, DirTreeError>;
}

#[derive(Debug)]
pub struct Filesystem(PathBuf);

impl Filesystem {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }
}

impl DirTreeBuild for Filesystem {
    fn read_dir_tree(&self) -> Result<DirTree, DirTreeError> {
        let mut dt = DirTree::default();
        for entry in WalkDir::new(&self.0).min_depth(1) {
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
}
