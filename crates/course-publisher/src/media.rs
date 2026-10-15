use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

const MAX_BOX_DEPTH: usize = 8;
const MAX_BOXES_VISITED: usize = 4_096;
const MAX_LEAF_BOX_BYTES: u64 = 64 * 1024;

/// How strongly a media finding should be surfaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingSeverity {
    /// Likely to break or delay playback for learners.
    Warning,
    /// Works in many players but is not the recommended profile.
    Advice,
}

/// One inspection result. Messages never contain paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaFinding {
    /// Stable machine-readable classification.
    pub code: &'static str,
    /// Display severity.
    pub severity: FindingSeverity,
    /// Human-readable remediation.
    pub message: &'static str,
}

/// Facts learned from a bounded, read-only inspection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaReport {
    /// Whole-second duration when the container declares one.
    pub duration_seconds: Option<u64>,
    /// Compatibility findings; empty when nothing needs attention.
    pub findings: Vec<MediaFinding>,
}

/// Inspect a source file according to its declared media type.
///
/// Only container headers are read; sample tables and media data are skipped
/// by seeking. Nothing is ever written or transcoded.
#[must_use]
pub fn inspect_media(path: &Path, media_type: &str) -> MediaReport {
    let mut report = MediaReport::default();
    let Ok(mut file) = File::open(path) else {
        return report;
    };
    match media_type {
        "video/mp4" | "audio/mp4" => inspect_mp4(&mut file, media_type, &mut report),
        "video/webm" | "video/quicktime" | "video/x-matroska" => report.findings.push(finding(
            "container_not_recommended",
            FindingSeverity::Advice,
            "Prefer MP4 with H.264 video and AAC audio for the widest learner playback support.",
        )),
        "application/x-subrip" => report.findings.push(finding(
            "subtitle_format",
            FindingSeverity::Advice,
            "Convert SubRip subtitles to WebVTT so the built-in player can display them.",
        )),
        "application/pdf" if !starts_with(&mut file, b"%PDF-") => report.findings.push(finding(
            "unrecognized_pdf",
            FindingSeverity::Warning,
            "This file does not start like a PDF document; check that the right file was attached.",
        )),
        _ => {}
    }
    report
}

fn inspect_mp4(file: &mut File, media_type: &str, report: &mut MediaReport) {
    let Ok(length) = file.seek(SeekFrom::End(0)) else {
        return;
    };
    let mut scan = Mp4Scan::default();
    if scan.walk(file, 0, length, 0).is_err() || scan.first_box != Some(*b"ftyp") {
        report.findings.push(finding(
            "unrecognized_mp4",
            FindingSeverity::Warning,
            "This file could not be read as an MP4 container; check that the right file was attached.",
        ));
        return;
    }
    report.duration_seconds = scan.duration_seconds;
    match (scan.moov_offset, scan.mdat_offset) {
        (None, _) => report.findings.push(finding(
            "mp4_index_missing",
            FindingSeverity::Warning,
            "The MP4 has no movie index (moov), so it cannot be played; re-export the file.",
        )),
        (Some(moov), Some(mdat)) if mdat < moov => report.findings.push(finding(
            "mp4_not_fast_start",
            FindingSeverity::Warning,
            "The MP4 index is stored after the media data, so playback waits for the whole file; re-export with fast start (moov first).",
        )),
        _ => {}
    }
    if let Some(codec) = scan.video_codec {
        if !matches!(&codec, b"avc1" | b"avc3") {
            report.findings.push(finding(
                "video_codec_not_recommended",
                FindingSeverity::Advice,
                "The video is not H.264; some learner devices may not play it. H.264 is recommended.",
            ));
        }
    } else if media_type == "video/mp4" && scan.moov_offset.is_some() {
        report.findings.push(finding(
            "video_track_missing",
            FindingSeverity::Warning,
            "No video track was found in this video file.",
        ));
    }
    if let Some(codec) = scan.audio_codec
        && &codec != b"mp4a"
    {
        report.findings.push(finding(
            "audio_codec_not_recommended",
            FindingSeverity::Advice,
            "The audio is not AAC; some learner devices may not play it. AAC is recommended.",
        ));
    }
}

#[derive(Default)]
struct Mp4Scan {
    first_box: Option<[u8; 4]>,
    moov_offset: Option<u64>,
    mdat_offset: Option<u64>,
    duration_seconds: Option<u64>,
    video_codec: Option<[u8; 4]>,
    audio_codec: Option<[u8; 4]>,
    current_handler: Option<[u8; 4]>,
    visited: usize,
}

impl Mp4Scan {
    fn walk(&mut self, file: &mut File, start: u64, end: u64, depth: usize) -> io::Result<()> {
        if depth > MAX_BOX_DEPTH {
            return Err(invalid());
        }
        let mut offset = start;
        while offset < end {
            self.visited += 1;
            if self.visited > MAX_BOXES_VISITED || end - offset < 8 {
                return Err(invalid());
            }
            file.seek(SeekFrom::Start(offset))?;
            let mut header = [0_u8; 8];
            file.read_exact(&mut header)?;
            let declared = u64::from(u32::from_be_bytes([
                header[0], header[1], header[2], header[3],
            ]));
            let kind = [header[4], header[5], header[6], header[7]];
            let (size, header_len) = match declared {
                0 => (end - offset, 8),
                1 => {
                    let mut large = [0_u8; 8];
                    file.read_exact(&mut large)?;
                    (u64::from_be_bytes(large), 16)
                }
                size => (size, 8),
            };
            if size < header_len || size > end - offset {
                return Err(invalid());
            }
            let body = offset + header_len;
            let box_end = offset + size;
            if depth == 0 && self.first_box.is_none() {
                self.first_box = Some(kind);
            }
            match &kind {
                b"moov" if depth == 0 => {
                    self.moov_offset.get_or_insert(offset);
                    self.walk(file, body, box_end, depth + 1)?;
                }
                b"mdat" if depth == 0 => {
                    self.mdat_offset.get_or_insert(offset);
                }
                b"trak" => {
                    self.current_handler = None;
                    self.walk(file, body, box_end, depth + 1)?;
                }
                b"mdia" | b"minf" | b"stbl" => self.walk(file, body, box_end, depth + 1)?,
                b"mvhd" => self.read_mvhd(file, box_end - body)?,
                b"hdlr" => self.read_hdlr(file, box_end - body)?,
                b"stsd" => self.read_stsd(file, box_end - body)?,
                _ => {}
            }
            offset = box_end;
        }
        Ok(())
    }

    fn read_mvhd(&mut self, file: &mut File, length: u64) -> io::Result<()> {
        let body = read_leaf(file, length)?;
        let (timescale, duration) = match body.first() {
            Some(0) if body.len() >= 20 => {
                (be_u32(&body[12..16]), u64::from(be_u32(&body[16..20])))
            }
            Some(1) if body.len() >= 32 => (be_u32(&body[20..24]), be_u64(&body[24..32])),
            _ => return Err(invalid()),
        };
        if timescale > 0 && duration != u64::MAX && duration != u64::from(u32::MAX) {
            let timescale = u64::from(timescale);
            self.duration_seconds = Some(duration.saturating_add(timescale / 2) / timescale);
        }
        Ok(())
    }

    fn read_hdlr(&mut self, file: &mut File, length: u64) -> io::Result<()> {
        let body = read_leaf(file, length)?;
        if body.len() < 12 {
            return Err(invalid());
        }
        self.current_handler = Some([body[8], body[9], body[10], body[11]]);
        Ok(())
    }

    fn read_stsd(&mut self, file: &mut File, length: u64) -> io::Result<()> {
        let mut prefix = [0_u8; 16];
        if length < 16 {
            return Err(invalid());
        }
        file.read_exact(&mut prefix)?;
        let codec = [prefix[12], prefix[13], prefix[14], prefix[15]];
        match self.current_handler.as_ref() {
            Some(b"vide") => {
                self.video_codec.get_or_insert(codec);
            }
            Some(b"soun") => {
                self.audio_codec.get_or_insert(codec);
            }
            _ => {}
        }
        Ok(())
    }
}

fn read_leaf(file: &mut File, length: u64) -> io::Result<Vec<u8>> {
    if length > MAX_LEAF_BOX_BYTES {
        return Err(invalid());
    }
    let mut body = vec![0_u8; usize::try_from(length).map_err(|_| invalid())?];
    file.read_exact(&mut body)?;
    Ok(body)
}

fn starts_with(file: &mut File, signature: &[u8]) -> bool {
    let mut prefix = vec![0_u8; signature.len()];
    file.read_exact(&mut prefix).is_ok() && prefix == signature
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn be_u64(bytes: &[u8]) -> u64 {
    u64::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "malformed MP4 box structure")
}

fn finding(code: &'static str, severity: FindingSeverity, message: &'static str) -> MediaFinding {
    MediaFinding {
        code,
        severity,
        message,
    }
}
