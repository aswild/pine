#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
pub mod ffi;

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
