fn main() {
    pkg_config::Config::new().atleast_version("3.0.0").probe("libarchive").unwrap();
}
