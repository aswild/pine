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

# regex notes: bindgen is *really* annoying here and the semantics are unintuitive and undocumented.
#   - Each allowlist or blocklist argument is a regex, but it's wrapped as '^({arg})$' so '.*' is
#     frequently required on anything we're tyring to match with prefixes.
#   - bindgen includes the regex crate with 'default-features = false', which means unicode support
#     is totally disabled. Regex has some tricky behavior when unicode is turned off, it doesn't
#     just mean "fall back to ASCII only"
#   - To ignore case, we can use (?i) but that's forbidden when unicode support isn't available,
#     so we also must disable unicode, i.e. (?i-u)
#   - You can't use '.*' in a non-unicode pattern because it "could match invalid utf-8" so the
#     '.*' has to be grouped outside of the context where we do (?i-u:blah)
#   - To make matters worse, bindgen doesn't report anything about invalid regexes, they're
#     just ignored. A warning is issued, but you'll only see it with RUST_LOG=warn (or higher)

bindgen_options=(
    # assume #defines are i32
    --default-macro-constant-type signed
    # use temporary files to parse function-like macros, needed for AE_IFMT and similar
    --clang-macro-fallback
    # don't recursively codegen for libc types
    --no-recursive-allowlist

    # include libarchive stuff
    --allowlist-var '(?i-u:archive)_.*'
    --allowlist-var 'AE_.*'
    --allowlist-function 'archive_.*'
    --allowlist-type '(la|archive)_.*'
    --allowlist-type 'archive'

    # include primitive C types
    --allowlist-type '_*(dev_t|mode_t|time_t|wchar_t)'
    # don't leak types for members of struct stat, it's only used as a pointer in the libarchive API
    --allowlist-type 'stat'
    --opaque-type 'stat'
    # disable functions that use FILE (to avoid codegen for the internals of FILE)
    --blocklist-function '.*_FILE$'
)

bindgen "${bindgen_options[@]}" -o "$D/src/ffi.rs" "$header" -- "${cflags[@]}"

# the AE_IF<type> macros are defaulted to i32 because of --default-macro-constant-type signed,
# but we need them to be u32. Time to patch things manually I guess
sed -Ei '/^pub const AE_IF/s/i32/u32/' "$D/src/ffi.rs"
