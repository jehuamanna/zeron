//! Bounded media inspection tests with synthetic containers.

use std::{fs, path::PathBuf};

use course_publisher::{FindingSeverity, MediaReport, inspect_media};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("zeron-media-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn mp4_box(kind: &[u8], body: &[u8]) -> Vec<u8> {
    let mut bytes = u32::try_from(body.len() + 8)
        .unwrap()
        .to_be_bytes()
        .to_vec();
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(body);
    bytes
}

fn mvhd(timescale: u32, duration: u32) -> Vec<u8> {
    let mut body = vec![0_u8; 12];
    body.extend_from_slice(&timescale.to_be_bytes());
    body.extend_from_slice(&duration.to_be_bytes());
    body.extend_from_slice(&[0_u8; 80]);
    mp4_box(b"mvhd", &body)
}

fn track(handler: &[u8], codec: &[u8]) -> Vec<u8> {
    let mut hdlr = vec![0_u8; 8];
    hdlr.extend_from_slice(handler);
    hdlr.extend_from_slice(&[0_u8; 13]);
    let mut stsd = vec![0, 0, 0, 0, 0, 0, 0, 1];
    stsd.extend_from_slice(&mp4_box(codec, &[0_u8; 70]));
    let large_table = mp4_box(b"stsz", &vec![0_u8; 200_000]);
    let stbl = mp4_box(b"stbl", &[mp4_box(b"stsd", &stsd), large_table].concat());
    let minf = mp4_box(b"minf", &stbl);
    let mdia = mp4_box(b"mdia", &[mp4_box(b"hdlr", &hdlr), minf].concat());
    mp4_box(b"trak", &mdia)
}

fn movie(video: &[u8], audio: &[u8], fast_start: bool) -> Vec<u8> {
    let ftyp = mp4_box(b"ftyp", b"isom\0\0\x02\0isomavc1");
    let moov = mp4_box(
        b"moov",
        &[
            mvhd(1000, 125_400),
            track(b"vide", video),
            track(b"soun", audio),
        ]
        .concat(),
    );
    let mdat = mp4_box(b"mdat", &[9_u8; 4096]);
    if fast_start {
        [ftyp, moov, mdat].concat()
    } else {
        [ftyp, mdat, moov].concat()
    }
}

fn codes(report: &MediaReport) -> Vec<&'static str> {
    report.findings.iter().map(|finding| finding.code).collect()
}

#[test]
fn recommended_fast_start_mp4_has_no_findings_and_a_duration() {
    let scratch = Scratch::new("ok");
    let report = inspect_media(
        &scratch.file("ok.mp4", &movie(b"avc1", b"mp4a", true)),
        "video/mp4",
    );
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    assert_eq!(report.duration_seconds, Some(125));
}

#[test]
fn index_after_media_data_is_a_warning() {
    let scratch = Scratch::new("slow");
    let report = inspect_media(
        &scratch.file("slow.mp4", &movie(b"avc1", b"mp4a", false)),
        "video/mp4",
    );
    assert_eq!(codes(&report), ["mp4_not_fast_start"]);
    assert_eq!(report.findings[0].severity, FindingSeverity::Warning);
    assert_eq!(report.duration_seconds, Some(125));
}

#[test]
fn non_recommended_codecs_are_advice() {
    let scratch = Scratch::new("codecs");
    let report = inspect_media(
        &scratch.file("hevc.mp4", &movie(b"hvc1", b"Opus", true)),
        "video/mp4",
    );
    assert_eq!(
        codes(&report),
        ["video_codec_not_recommended", "audio_codec_not_recommended"]
    );
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.severity == FindingSeverity::Advice)
    );
}

#[test]
fn missing_index_and_foreign_bytes_are_warnings() {
    let scratch = Scratch::new("broken");
    let no_index = [mp4_box(b"ftyp", b"isom"), mp4_box(b"mdat", &[1_u8; 64])].concat();
    assert_eq!(
        codes(&inspect_media(
            &scratch.file("noindex.mp4", &no_index),
            "video/mp4"
        )),
        ["mp4_index_missing"]
    );
    assert_eq!(
        codes(&inspect_media(
            &scratch.file("text.mp4", b"this is not a movie at all"),
            "video/mp4"
        )),
        ["unrecognized_mp4"]
    );
    let mut oversized = mp4_box(b"ftyp", b"isom");
    oversized.extend_from_slice(&u32::MAX.to_be_bytes());
    oversized.extend_from_slice(b"moov");
    assert_eq!(
        codes(&inspect_media(
            &scratch.file("oversized.mp4", &oversized),
            "video/mp4"
        )),
        ["unrecognized_mp4"]
    );
    assert_eq!(
        codes(&inspect_media(
            &scratch.file("fake.pdf", b"<html>"),
            "application/pdf"
        )),
        ["unrecognized_pdf"]
    );
}

#[test]
fn container_and_subtitle_formats_receive_advice() {
    let scratch = Scratch::new("formats");
    assert_eq!(
        codes(&inspect_media(
            &scratch.file("a.webm", b"\x1aE\xdf\xa3"),
            "video/webm"
        )),
        ["container_not_recommended"]
    );
    assert_eq!(
        codes(&inspect_media(
            &scratch.file("a.srt", b"1\n00:00:01,000 --> 00:00:02,000\nHi\n"),
            "application/x-subrip"
        )),
        ["subtitle_format"]
    );
    assert!(
        inspect_media(&scratch.file("a.pdf", b"%PDF-1.7"), "application/pdf")
            .findings
            .is_empty()
    );
    assert!(
        inspect_media(&scratch.file("a.vtt", b"WEBVTT\n"), "text/vtt")
            .findings
            .is_empty()
    );
    assert_eq!(
        inspect_media(&scratch.0.join("missing.mp4"), "video/mp4"),
        MediaReport::default()
    );
}

#[test]
fn arbitrary_bytes_never_panic() {
    let scratch = Scratch::new("fuzz");
    let base = movie(b"avc1", b"mp4a", true);
    let mut state = 0x9e37_79b9_u32;
    for round in 0..300 {
        let mut bytes = base.clone();
        for _ in 0..8 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let index = state as usize % bytes.len();
            bytes[index] = state.to_be_bytes()[2];
        }
        bytes.truncate(bytes.len() - (state as usize % 64));
        let path = scratch.file(&format!("fuzz{round}.mp4"), &bytes);
        let _ = inspect_media(&path, "video/mp4");
    }
}
