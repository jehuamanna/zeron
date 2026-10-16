//! Mutated transcripts, note anchors, note documents, and drawing scenes
//! never panic their parsers.

use learning_domain::{Anchor, NoteBody, Scene, parse_transcript};
use mutation_testing::mutations;

#[test]
fn mutated_transcripts_parse_to_ordered_cues_or_are_refused() {
    let transcripts: &[&[u8]] = &[
        b"WEBVTT\n\n00:01.000 --> 00:02.500\nHello\n\n01:00:00.000 --> 01:00:01.000 align:start\n<v Ana>Hi</v>\n",
        b"1\n00:00:01,000 --> 00:00:02,000\nSubRip line\n\n2\n00:00:03,000 --> 00:00:04,000\nNext\n",
    ];
    for input in mutations(transcripts, 0x7EA5_0001, 4_000, 16 * 1024) {
        if let Ok(cues) = parse_transcript(&input) {
            assert!(cues.iter().all(|cue| cue.start <= cue.end));
        }
    }
}

#[test]
fn mutated_notes_and_drawings_are_refused_without_panicking() {
    let anchors: &[&[u8]] = &[
        br#"{"kind":"videoTime","resourceId":"r1","seconds":12.5}"#,
        br#"{"kind":"pdfPage","resourceId":"r2","page":3,"region":{"x":0.1,"y":0.2,"width":0.3,"height":0.4}}"#,
        br#"{"kind":"lesson"}"#,
    ];
    for input in mutations(anchors, 0x7EA5_0002, 4_000, 4 * 1024) {
        if let Ok(anchor) = Anchor::from_json(&String::from_utf8_lossy(&input)) {
            let _ = anchor.validate();
        }
    }
    let documents: &[&[u8]] = &[
        br#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"note"}]}]}"#,
    ];
    for input in mutations(documents, 0x7EA5_0003, 4_000, 16 * 1024) {
        let _ = NoteBody::parse(&String::from_utf8_lossy(&input));
    }
    let scenes: &[&[u8]] =
        &[br#"{"elements":[{"id":"a","type":"rectangle"}],"appState":{"name":"x"},"files":{}}"#];
    for input in mutations(scenes, 0x7EA5_0004, 4_000, 16 * 1024) {
        let _ = Scene::parse(&String::from_utf8_lossy(&input));
    }
}
