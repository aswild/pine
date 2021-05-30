// Copyright (c) 2021 Allen Wild <allenwild93@gmail.com>
// SPDX-License-Identifier: GPL-3.0-or-later

fn main() {
    pkg_config::Config::new().atleast_version("3.0.0").probe("libarchive").unwrap();
}
