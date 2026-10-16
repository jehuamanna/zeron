//! Mutated publisher configuration files never panic the parser, and any
//! file it accepts round-trips.

use mutation_testing::mutations;
use publisher_application::PublisherConfiguration;

#[test]
fn mutated_configuration_files_are_refused_or_round_trip() {
    let valid = br#"{"format":"zeron.publisher-configuration","version":1,"trackers":["udp://tracker.example:6969/announce"],"trackerLists":["https://lists.example/best.txt"],"network":{"profile":"internet","peerPort":51413,"portForwarding":true,"downloadLimitKib":null,"uploadLimitKib":512}}"#;
    let seeds: &[&[u8]] = &[valid];
    for (index, input) in mutations(seeds, 0xC0F1_0001, 4_000, 32 * 1024)
        .iter()
        .enumerate()
    {
        if let Ok(configuration) = PublisherConfiguration::parse(input) {
            assert_eq!(
                PublisherConfiguration::parse(&configuration.to_bytes()).unwrap(),
                configuration,
                "mutation {index} did not round-trip"
            );
        }
    }
}
