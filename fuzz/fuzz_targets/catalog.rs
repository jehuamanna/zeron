//! Catalog documents: bounded parsing and verification never panic.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(catalog) = course_signing::parse_catalog(data) {
        let _ = course_signing::verify_catalog(&catalog);
    }
});
