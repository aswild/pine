#!/bin/bash

set -e

D="$(readlink -f "$(dirname "$0")")"

if pkg-config --exists libarchive; then
    cflags=( $(pkg-config --cflags libarchive) )
else
    cflags=()
fi

header="$(mktemp --suffix=.h)"
trap "rm -f $header" EXIT
echo -e '#include <archive.h>\n#include <archive_entry.h>' >"$header"

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
    # don't leak types for members of struct stat, it's only used as a pointer in the libarchive API
    --whitelist-type 'stat'
    --opaque-type 'stat'
    # disable functions that use FILE (to avoid codegen for the internals of FILE)
    --blacklist-function '.*_FILE$'
)

bindgen "${bindgen_options[@]}" -o "$D/src/ffi.rs" "$header" -- "${cflags[@]}"
