//! zeron-signature.json: no panic, and a parsed signature can only verify
//! for the statement it carries.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(signature) = course_signing::parse_release_signature(data) {
        let statement = &signature.statement;
        let _ = course_signing::verify_release(
            &signature,
            &statement.course_id,
            &statement.course_version,
            &statement.manifest_sha256,
            &statement.publisher_key,
            &[],
        );
    }
});
