//! course.json: accepted manifests are valid and round-trip.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(manifest) = course_format::parse_and_validate(data) {
        assert!(manifest.validate().is_ok());
        let bytes = serde_json::to_vec(&manifest).unwrap();
        assert_eq!(course_format::parse_and_validate(&bytes).unwrap(), manifest);
    }
});
