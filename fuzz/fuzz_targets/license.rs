//! License files: no panic, and verification of whatever parses never
//! panics either.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(license) = course_signing::parse_license(data) {
        if let Ok(learner) = license.license.statement.learner_key.parse() {
            let _ = course_signing::verify_license(&license, &license.publisher_id, &learner, &[]);
        }
    }
});
