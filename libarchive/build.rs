// Copyright (c) 2021 Allen Wild <allenwild93@gmail.com>
// SPDX-License-Identifier: GPL-3.0-or-later

fn main() {
    eprintln!("Searching for libarchive>=3.0.0 with pkg-config.");
    if pkg_config::Config::new().atleast_version("3.0.0").probe("libarchive").is_ok() {
        // probe() printed all the relevant cargo metadata output so we don't have to do anything
        eprintln!("libarchive found using pkg-config");
        return;
    }

    // This hack looks for libarchive.so (probably a symlink) either in the libarchive
    // directory here, or in the parent pine directory. If found, we use our local directory as
    // a native library search path and tell rustc to link with it. This is a hack, but it
    // works here because all we need to do is link against libarchive.so, we don't really need
    // anything else pkg-config would tell us, and lets pine work on systems that have
    // libarchive installed but not libarchive-dev. The linker is smart enough to follow the
    // libarchive.so symlink path to the real library, and no special rpath handling is needed.
    //
    // For custom libarchive installations, set PKG_CONFIG_PATH and use pkg-config as usual, not
    // this method.
    eprintln!("Failed to find libarchive using pkg-config, looking for local libarchive instead.");

    // look for libarchive.so in the pine/libarchive directory
    let mydir = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    if mydir.join("libarchive.so").exists() {
        eprintln!("Found libarchive.so in {}", mydir.display());
        println!("cargo:rustc-link-search=native={}", mydir.display());
        println!("cargo:rustc-link-lib=archive");
        return;
    }

    // look for libarchive.so in the top-level pine subdirectory
    let pinedir = mydir.parent().unwrap();
    if pinedir.join("libarchive.so").exists() {
        eprintln!("Found libarchive.so in {}", pinedir.display());
        println!("cargo:rustc-link-search=native={}", pinedir.display());
        println!("cargo:rustc-link-lib=archive");
        return;
    }

    // none of the above methods worked, print a useful message and bail.
    eprintln!("No libarchive.so found in the local `pine` or `pine/libarchive` directories.\n\
               Either install the libarchive-dev and pkg-config packages, or create a `libarchive.so` \
               symlink in the current directory pointing to a suitable `libarchive.so.13` file.");
    std::process::exit(1);
}
