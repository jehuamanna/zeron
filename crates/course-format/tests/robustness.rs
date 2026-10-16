//! Mutated manifests never panic the parser, and whatever it accepts is valid
//! and survives a round trip.

use course_format::parse_and_validate;
use mutation_testing::mutations;

const VALID: &[u8] = include_bytes!("fixtures/valid-course.json");

#[test]
fn mutated_manifests_are_refused_or_valid_and_round_trip() {
    let seeds: &[&[u8]] = &[VALID, b"{}", b"{\"schemaVersion\":1}"];
    let mut accepted = 0;
    for (index, input) in mutations(seeds, 0x00C0_FFEE, 4_000, 64 * 1024)
        .iter()
        .enumerate()
    {
        let Ok(manifest) = parse_and_validate(input) else {
            continue;
        };
        accepted += 1;
        manifest
            .validate()
            .unwrap_or_else(|error| panic!("mutation {index} parsed but is invalid: {error}"));
        let bytes = serde_json::to_vec(&manifest).unwrap();
        assert_eq!(
            parse_and_validate(&bytes).unwrap(),
            manifest,
            "mutation {index} did not round-trip"
        );
    }
    assert!(accepted > 0, "no mutation kept the manifest valid");
}
