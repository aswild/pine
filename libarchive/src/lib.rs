use std::borrow::Borrow;
use std::ffi::{CStr, OsStr};
use std::fmt;
use std::io::Read;
use std::os::raw::{c_char, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::pin::Pin;

#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(clippy::redundant_static_lifetimes)]
// HACK! These constants are #defined like
//    #define AE_IFMT ((__LA_MODE_T)0170000)
// and bindgen can't handle that, it just skips them.  We can do something slightly less janky
// later on so that the values don't have to be hard-coded, but for now just hack them in here.
// The #[path] attributes are so that we can have this inner mod named ffi and also a file called
// ffi.rs, since eventually this will go away (TODO).
// See https://github.com/rust-lang/rust-bindgen/issues/753
#[path = ""]
pub mod ffi {
    #[path = "ffi.rs"]
    mod real_ffi;
    pub use real_ffi::*;

    pub const AE_IFMT: u32 = 0o170000;
    pub const AE_IFREG: u32 = 0o100000;
    pub const AE_IFLNK: u32 = 0o120000;
    pub const AE_IFSOCK: u32 = 0o140000;
    pub const AE_IFCHR: u32 = 0o020000;
    pub const AE_IFBLK: u32 = 0o060000;
    pub const AE_IFDIR: u32 = 0o040000;
    pub const AE_IFIFO: u32 = 0o010000;
}

macro_rules! expect_nonnull {
    ($e:expr) => {
        match $e {
            p if p.is_null() => panic!("{} unexpectedly returned NULL", stringify!($e)),
            p => p,
        }
    };
}

macro_rules! expect_nonnull_unsafe {
    ($e:expr) => {
        unsafe { expect_nonnull!($e) }
    };
}

const DEFAULT_BUF_SIZE: usize = 8192;

/// Convert a borrowed raw C string into an owned PathBuf, or None if the pointer is NULL.

/// SAFETY: `ptr` must point to a null-terminated string, or be a NULL pointer.
unsafe fn raw_cstring_to_pathbuf(ptr: *const c_char) -> Option<PathBuf> {
    if ptr.is_null() {
        None
    } else {
        Some(PathBuf::from(OsStr::from_bytes(CStr::from_ptr(ptr).to_bytes())))
    }
}

#[derive(Debug)]
pub struct ArchiveEntry {
    ptr: *mut ffi::archive_entry,
}

impl Drop for ArchiveEntry {
    fn drop(&mut self) {
        unsafe {
            ffi::archive_entry_free(self.ptr);
        }
    }
}

impl Clone for ArchiveEntry {
    fn clone(&self) -> Self {
        Self { ptr: expect_nonnull_unsafe!(ffi::archive_entry_clone(self.ptr)) }
    }
}

impl Default for ArchiveEntry {
    fn default() -> Self {
        Self::new()
    }
}

impl ArchiveEntry {
    pub fn new() -> Self {
        Self { ptr: expect_nonnull_unsafe!(ffi::archive_entry_new()) }
    }

    pub fn path(&self) -> Option<PathBuf> {
        unsafe { raw_cstring_to_pathbuf(ffi::archive_entry_pathname(self.ptr)) }
    }

    pub fn symlink_path(&self) -> Option<PathBuf> {
        unsafe { raw_cstring_to_pathbuf(ffi::archive_entry_symlink(self.ptr)) }
    }

    pub fn filetype(&self) -> ffi::mode_t {
        unsafe { ffi::archive_entry_filetype(self.ptr) }
    }

    pub fn is_file(&self) -> bool {
        self.filetype() == ffi::AE_IFREG
    }

    pub fn is_dir(&self) -> bool {
        self.filetype() == ffi::AE_IFDIR
    }

    pub fn is_symlink(&self) -> bool {
        self.filetype() == ffi::AE_IFLNK
    }

    fn as_ptr(&mut self) -> *mut ffi::archive_entry {
        self.ptr
    }
}

#[derive(Debug)]
pub struct ArchiveError {
    errno: i32,
    msg: String,
    prefix: Option<String>,
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(ref prefix) = self.prefix {
            write!(f, "{}: {} ({})", prefix, self.msg, self.errno)
        } else {
            write!(f, "{} ({})", self.msg, self.errno)
        }
    }
}

impl std::error::Error for ArchiveError {}

impl ArchiveError {
    /// Construct an ArchiveError by calling archive_errno() and archive_error_string() on the
    /// given archive.
    ///
    /// SAFETY: archive must be a valid pointer to a struct archive.
    unsafe fn from_archive(archive: *mut ffi::archive) -> Self {
        let msg = match ffi::archive_error_string(archive) {
            p if p.is_null() => "[unknown error message]".into(),
            p => CStr::from_ptr(p).to_string_lossy().into_owned(),
        };

        Self { errno: ffi::archive_errno(archive), msg, prefix: None }
    }

    unsafe fn with_prefix(archive: *mut ffi::archive, prefix: impl ToString) -> Self {
        let mut err = ArchiveError::from_archive(archive);
        err.prefix = Some(prefix.to_string());
        err
    }

    pub fn new_custom(msg: String) -> Self {
        Self { errno: 0, msg, prefix: None }
    }
}

#[derive(Debug)]
struct ReadInner<R: Read> {
    reader: R,
    buf: Box<[u8]>,
}

impl<R: Read> ReadInner<R> {
    fn new_pinned(reader: R, buf_size: usize) -> Pin<Box<Self>> {
        let buf = vec![0u8; buf_size].into_boxed_slice();
        Box::pin(Self { reader, buf })
    }
}

pub struct ArchiveReader<R: Read> {
    /// Raw FFI object
    ptr: *mut ffi::archive,
    /// Rust reader and buffer, used by the read callback
    // This field is passed as a raw pointer to libarchive, and never used directly from Rust code,
    // thus the compiler thinks it's never used.
    #[allow(dead_code)]
    read_inner: Pin<Box<ReadInner<R>>>,
    /// Cached struct archive_entry for use during reading. A reference to this is returned by
    /// read_next_header
    entry: ArchiveEntry,
}

impl<R: Read> ArchiveReader<R> {
    unsafe extern "C" fn read_callback(
        archive: *mut ffi::archive,
        data: *mut c_void,
        out_buf: *mut *const c_void,
    ) -> ffi::la_ssize_t {
        // SAFETY: This is a C callback called by libarchive, in which things get scary.
        // data is a void* that must point to a ReadInner<R>, it's set up when we call
        // archive_read_open().
        //
        // The ReadInner<R> is pinned and owned by the Pin<Box<ReadInner<R>>> inside the same
        // ArchiveReader<R> that registered this callback.
        //
        // We must not move out of the ReadInner or do anything else that might drop its contents.
        // Lifetime guarantees are also out the window here, but we have to return a data pointer
        // to libarchive via out_buf. This means that the ReadInner's buf must stay alive and
        // pinned for as long as the struct archive* is live.
        //
        // Exclusive access to *mut ReadInner<R> should be guaranteed bceause this callback is only
        // called by libarchive functions that use this ArchiveReader's struct archive*, which will
        // only be callable by &mut self methods. Passing the raw *mut ffi::archive out of this
        // object and using it anywhere else could cause UB and must be avoided.

        // we can't cast *mut T to &mut T, so stick with a raw pointer throughout this function.
        let ri: *mut ReadInner<R> = data as *mut _;

        // dereference the raw pointer to get a ReadInner, then call io::Read::read on its reader,
        // using the ReadInner's boxed slice as the buffer (again, deref the raw pointer, then
        // Box<[u8]> auto-derefs to &mut [u8]).
        // TODO: handle EINTR/EGAIN errors and retry automatically
        match (*ri).reader.read(&mut (*ri).buf) {
            Ok(count) => {
                *out_buf = (*ri).buf.borrow() as *const [u8] as *const c_void;
                count as ffi::la_ssize_t
            }
            Err(err) => {
                // if reading fails, we must call archive_set_error and return -1
                let errno = err.raw_os_error().unwrap_or(libc::EINVAL);
                let msg = CStr::from_bytes_with_nul(b"error reading archive input\0").unwrap();
                // archive_set_error is variadic and takes a printf-like format. for simplicity,
                // just use a constant error message.
                ffi::archive_set_error(archive, errno, msg.as_ptr());
                -1
            }
        }
    }

    /// Create a new ArchiveReader wrapping the given reader.
    ///
    /// May panic if archive_read_new() fails.
    pub fn new(reader: R) -> Result<Self, ArchiveError> {
        let mut read_inner = ReadInner::new_pinned(reader, DEFAULT_BUF_SIZE);

        let ptr = unsafe {
            let archive = expect_nonnull!(ffi::archive_read_new());
            if ffi::archive_read_support_format_all(archive) != ffi::ARCHIVE_OK {
                return Err(ArchiveError::with_prefix(archive, "failed to enable archive formats"));
            }
            if ffi::archive_read_support_filter_all(archive) != ffi::ARCHIVE_OK {
                return Err(ArchiveError::with_prefix(archive, "failed to enable archive filters"));
            }

            // as_mut converts Pin<Box<ReadInner<R>>> to Pin<&mut ReadInner<R>>,
            // get_unchecked_mut converts Pin<&mut ReadInner<R>> to &mut ReadInner<R>,
            // then cast mut reference into a raw pointer, then cast to void*.
            //
            // SAFETY: we must never use this pointer to move out of or drop the read_inner. This
            // pointer is passed to read_callback() where we have to use it carefully.
            let data_ptr =
                read_inner.as_mut().get_unchecked_mut() as *mut ReadInner<R> as *mut c_void;

            // args are struct archive*. void* user_data, open callback, read callback, close
            // callback. We don't give libarchive any open/close callbacks because all of that is
            // handled in Rust.
            assert_eq!(
                ffi::archive_read_open(archive, data_ptr, None, Some(Self::read_callback), None),
                ffi::ARCHIVE_OK,
                "archive_read_open failed: {}",
                ArchiveError::from_archive(archive),
            );

            archive
        };
        Ok(Self { ptr, read_inner, entry: ArchiveEntry::new() })
    }

    /// Read the next entry in the archive, consuming input from the inner reader. Returns a shared
    /// reference to an ArchiveEntry owned by this ArchiveReader, or `Ok(None)` on EOF.
    pub fn read_next_header(&mut self) -> Result<Option<&ArchiveEntry>, ArchiveError> {
        let ret = unsafe { ffi::archive_read_next_header2(self.ptr, self.entry.as_ptr()) };
        match ret {
            ffi::ARCHIVE_OK => Ok(Some(&self.entry)),
            ffi::ARCHIVE_EOF => Ok(None),
            ffi::ARCHIVE_RETRY => todo!("handling ARCHIVE_RETRY is not yet implemented"),
            ffi::ARCHIVE_WARN | ffi::ARCHIVE_FATAL => Err(self.last_error()),
            _ => unreachable!(),
        }
    }

    pub fn last_error(&mut self) -> ArchiveError {
        // SAFETY: self.ptr is always valid
        unsafe { ArchiveError::from_archive(self.ptr) }
    }
}

impl<R: Read> Drop for ArchiveReader<R> {
    fn drop(&mut self) {
        let ret = unsafe { ffi::archive_read_free(self.ptr) };
        debug_assert_eq!(ret, ffi::ARCHIVE_OK, "archive_read_free failed!");
        // drop for the ReadInner will run next, closing the inner reader and dropping the buffer
        // now that we're sure that libarchive is done with it.
    }
}

#[cfg(test)]
mod tests {
    use crate::ffi;

    #[test]
    fn new_and_free() {
        unsafe {
            let ar = ffi::archive_read_new();
            assert!(!ar.is_null());
            ffi::archive_read_free(ar);
        }
    }
}
