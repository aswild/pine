#!/bin/bash

set -e

D="$(readlink -f "$(dirname "$0")")"

header="$(mktemp --suffix=.h)"
trap "rm -f $header" EXIT
echo -e '#include <archive.h>\n#include <archive_entry.h>' >"$header"

struct_stat_def='#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct stat {
    _unused: [u8; 0],
}'

bindgen_options=(
    # assume size_t is usize and #defines are i32
    --size_t-is-usize --default-macro-constant-type signed
    # don't recursively codegen for libc types
    --no-recursive-whitelist
    # include libarchive stuff
    --whitelist-function 'archive_.*'
    --whitelist-type 'archive|la_.*'
    --whitelist-var '(?i)archive_.*'
    # include primitive C types
    --whitelist-type '_*(dev_t|mode_t|time_t|wchar_t)'
    # disable functions that use FILE (to avoid codegen for the internals of FILE)
    --blacklist-function '.*_FILE$'
    # create a ZST for struct stat, since it's only used as a pointer
    --raw-line "$struct_stat_def"
)

bindgen "${bindgen_options[@]}" -o "$D/src/ffi.rs" "$header"
