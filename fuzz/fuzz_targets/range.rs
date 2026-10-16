//! Gateway Range header: accepted ranges always lie inside the resource.
#![no_main]

libfuzzer_sys::fuzz_target!(|input: (&str, u64)| {
    let (header, length) = input;
    if let Ok((start, end)) = media_gateway::parse_range_header(header, length) {
        assert!(start <= end && end < length);
    }
});
