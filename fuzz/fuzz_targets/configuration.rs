//! Publisher configuration files: accepted files round-trip.
#![no_main]

use publisher_application::PublisherConfiguration;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(configuration) = PublisherConfiguration::parse(data) {
        assert_eq!(
            PublisherConfiguration::parse(&configuration.to_bytes()).unwrap(),
            configuration
        );
    }
});
