use anyhow::Result;

mod dir_tree;
use dir_tree::{DirTree, DirTreeError, Entry};

// note: used by dir_tree tests, move to test module when no longer used as a placeholder in main
fn make_tree() -> Result<DirTree, DirTreeError> {
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

fn run() -> Result<()> {
    let dt = make_tree()?;
    println!("{:#?}", dt);
    dt.print("root");

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}
