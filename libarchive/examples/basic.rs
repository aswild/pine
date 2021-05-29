use std::env;
use std::fs::File;

use libarchive::ArchiveReader;

fn main() {
    let filename = env::args_os().nth(1).unwrap();
    let file = File::open(&filename).unwrap();

    let mut reader = ArchiveReader::new(file).unwrap();

    loop {
        match reader.read_next_header() {
            Ok(Some(entry)) => println!("{}", entry.path().unwrap().display()),
            Ok(None) => break,
            Err(e) => {
                println!("Error: {}", e);
                break;
            }
        }
    }
}
