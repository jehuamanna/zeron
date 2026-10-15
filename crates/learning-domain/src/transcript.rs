//! Timed transcripts: `WebVTT` and `SubRip` cues as plain text.

use thiserror::Error;

/// Largest transcript file read.
pub const MAX_TRANSCRIPT_BYTES: usize = 4 * 1024 * 1024;
/// Most cues kept from one transcript.
const MAX_CUES: usize = 50_000;

/// One timed line of a transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    /// Start, in seconds.
    pub start: f64,
    /// End, in seconds.
    pub end: f64,
    /// Plain text: markup removed, entities decoded, lines joined by `\n`.
    pub text: String,
}

/// Why a transcript could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TranscriptError {
    /// Not UTF-8 text within the size limit.
    #[error("the transcript is not readable text")]
    NotText,
    /// No timed cue could be read.
    #[error("the transcript has no timed lines")]
    NoCues,
}

/// Parse a `WebVTT` or `SubRip` transcript. Cues with unreadable timings are
/// skipped; header, `NOTE`, `STYLE`, and `REGION` blocks are ignored. Cues
/// are returned in start order.
///
/// # Errors
/// Returns [`TranscriptError`].
pub fn parse_transcript(bytes: &[u8]) -> Result<Vec<Cue>, TranscriptError> {
    if bytes.len() > MAX_TRANSCRIPT_BYTES {
        return Err(TranscriptError::NotText);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| TranscriptError::NotText)?;
    let text = text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        if cues.len() == MAX_CUES {
            break;
        }
        let mut lines = block.lines().skip_while(|line| line.trim().is_empty());
        let Some(first) = lines.next() else { continue };
        let first = first.trim();
        if first.starts_with("WEBVTT")
            || first.starts_with("NOTE")
            || first == "STYLE"
            || first == "REGION"
        {
            continue;
        }
        // An identifier (or SubRip index) may precede the timing line.
        let timing = if first.contains("-->") {
            first
        } else {
            match lines.next() {
                Some(line) if line.contains("-->") => line.trim(),
                _ => continue,
            }
        };
        let Some((start, end)) = parse_timing(timing) else {
            continue;
        };
        let body: Vec<String> = lines
            .map(clean_line)
            .filter(|line| !line.is_empty())
            .collect();
        if body.is_empty() {
            continue;
        }
        cues.push(Cue {
            start,
            end,
            text: body.join("\n"),
        });
    }
    if cues.is_empty() {
        return Err(TranscriptError::NoCues);
    }
    cues.sort_by(|a, b| a.start.total_cmp(&b.start));
    Ok(cues)
}

fn parse_timing(line: &str) -> Option<(f64, f64)> {
    let (start, rest) = line.split_once("-->")?;
    let end = rest.split_whitespace().next()?;
    let (start, end) = (parse_timestamp(start.trim())?, parse_timestamp(end)?);
    (end >= start).then_some((start, end))
}

/// `hh:mm:ss.mmm`, `mm:ss.mmm`, or `SubRip`'s `hh:mm:ss,mmm`.
fn parse_timestamp(value: &str) -> Option<f64> {
    let value = value.replace(',', ".");
    let (clock, millis) = value.split_once('.')?;
    if millis.len() != 3 || !millis.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parts: Vec<&str> = clock.split(':').collect();
    let number = |part: &str| -> Option<u32> {
        (!part.is_empty() && part.len() <= 4 && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
    };
    let (hours, minutes, seconds) = match parts.as_slice() {
        [hours, minutes, seconds] => (number(hours)?, number(minutes)?, number(seconds)?),
        [minutes, seconds] => (0, number(minutes)?, number(seconds)?),
        _ => return None,
    };
    if minutes > 59 || seconds > 59 {
        return None;
    }
    Some(
        f64::from(hours) * 3600.0
            + f64::from(minutes) * 60.0
            + f64::from(seconds)
            + f64::from(millis.parse::<u32>().ok()?) / 1000.0,
    )
}

/// Remove cue markup (`<b>`, `<v Speaker>`, timestamps, `SubRip` `{\an8}`) and
/// decode the entities `WebVTT` allows.
fn clean_line(line: &str) -> String {
    let mut text = String::with_capacity(line.len());
    let mut in_tag = false;
    let mut in_brace = false;
    for character in line.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            '{' if line.contains("{\\") => in_brace = true,
            '}' if in_brace => in_brace = false,
            _ if in_tag || in_brace => {}
            _ => text.push(character),
        }
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", "\u{a0}")
        .replace("&lrm;", "\u{200e}")
        .replace("&rlm;", "\u{200f}")
        .replace("&amp;", "&")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_webvtt_cues_without_markup() {
        let vtt = "\u{feff}WEBVTT - lesson\r\nKind: captions\r\n\r\nNOTE written by hand\r\nsecond line\r\n\r\nSTYLE\r\n::cue { color: red }\r\n\r\nintro\r\n00:00:01.000 --> 00:00:04.500 align:start\r\n<v Ana>Welcome to <b>caching</b></v>\r\n&lt;tiers&gt; &amp; more\r\n\r\n01:02.250 --> 01:05.000\r\nShort form\r\n";
        let cues = parse_transcript(vtt.as_bytes()).unwrap();
        assert_eq!(
            cues,
            [
                Cue {
                    start: 1.0,
                    end: 4.5,
                    text: "Welcome to caching\n<tiers> & more".to_owned()
                },
                Cue {
                    start: 62.25,
                    end: 65.0,
                    text: "Short form".to_owned()
                },
            ]
        );
    }

    #[test]
    fn reads_subrip_and_sorts_cues() {
        let srt = "2\n01:00:00,500 --> 01:00:02,000\n{\\an8}Later\n\n1\n00:00:00,000 --> 00:00:01,000\n<i>First</i>\n";
        let cues = parse_transcript(srt.as_bytes()).unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| (cue.start, cue.text.as_str()))
                .collect::<Vec<_>>(),
            [(0.0, "First"), (3600.5, "Later")]
        );
    }

    #[test]
    fn skips_broken_cues_and_refuses_non_transcripts() {
        let mixed = "WEBVTT\n\n00:00:05.000 --> 00:00:01.000\nbackwards\n\n00:61.000 --> 01:00.000\nbad seconds\n\n00:01.0 --> 00:02.000\nshort millis\n\n00:00:01.000 --> 00:00:02.000\n\n00:03.000 --> 00:04.000\nkept\n";
        let cues = parse_transcript(mixed.as_bytes()).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "kept");
        assert_eq!(
            parse_transcript(b"WEBVTT\n\n"),
            Err(TranscriptError::NoCues)
        );
        assert_eq!(
            parse_transcript(b"just some text"),
            Err(TranscriptError::NoCues)
        );
        assert_eq!(
            parse_transcript(&[0xff, 0xfe, 0x00]),
            Err(TranscriptError::NotText)
        );
        assert_eq!(
            parse_transcript(&vec![b'a'; MAX_TRANSCRIPT_BYTES + 1]),
            Err(TranscriptError::NotText)
        );
    }
}
