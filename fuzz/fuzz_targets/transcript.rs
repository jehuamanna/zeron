//! WebVTT and SubRip transcripts: cues are ordered or the file is refused.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(cues) = learning_domain::parse_transcript(data) {
        assert!(cues.iter().all(|cue| cue.start <= cue.end));
    }
});
