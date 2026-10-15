//! Turning what a learner types into a safe full-text query, and course text
//! into passages worth showing as results.

use crate::Cue;

/// Most terms used from one query.
const MAX_TERMS: usize = 12;
/// Longest transcript passage, in seconds.
const PASSAGE_SECONDS: f64 = 30.0;
/// Longest passage, in characters, before a new one starts.
const PASSAGE_CHARS: usize = 600;

/// An FTS5 query matching every word typed, the last one as a prefix so
/// results appear while typing. Each word is quoted, so no input can form
/// FTS5 syntax. `None` when nothing searchable was typed.
#[must_use]
pub fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .take(MAX_TERMS)
        .map(|term| format!("\"{term}\""))
        .collect();
    let (last, rest) = terms.split_last()?;
    let mut query = rest.join(" ");
    if !query.is_empty() {
        query.push(' ');
    }
    query.push_str(last);
    query.push('*');
    Some(query)
}

/// Group cues into passages of at most 30 seconds or about 600 characters,
/// each starting where its first cue starts.
#[must_use]
pub fn transcript_passages(cues: &[Cue]) -> Vec<(f64, String)> {
    let mut passages: Vec<(f64, String)> = Vec::new();
    for cue in cues {
        match passages.last_mut() {
            Some((start, text))
                if cue.start - *start < PASSAGE_SECONDS && text.len() < PASSAGE_CHARS =>
            {
                text.push(' ');
                text.push_str(&cue.text.replace('\n', " "));
            }
            _ => passages.push((cue.start, cue.text.replace('\n', " "))),
        }
    }
    passages
}

/// Split text into passages at blank lines, joining short paragraphs up to
/// about 600 characters.
#[must_use]
pub fn text_passages(text: &str) -> Vec<String> {
    let mut passages: Vec<String> = Vec::new();
    for paragraph in text.replace("\r\n", "\n").split("\n\n") {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        match passages.last_mut() {
            Some(passage) if passage.len() + paragraph.len() < PASSAGE_CHARS => {
                passage.push_str("\n\n");
                passage.push_str(paragraph);
            }
            _ => passages.push(paragraph.to_owned()),
        }
    }
    passages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_quote_every_word_and_prefix_the_last() {
        assert_eq!(
            fts_query("consistent hash").as_deref(),
            Some(r#""consistent" "hash"*"#)
        );
        assert_eq!(
            fts_query("  Réplication ").as_deref(),
            Some(r#""Réplication"*"#)
        );
        assert_eq!(
            fts_query(r#"a" OR title:x* NEAR(b) -c ^d"#).as_deref(),
            Some(r#""a" "OR" "title" "x" "NEAR" "b" "c" "d"*"#)
        );
        assert_eq!(fts_query(" *\"()- "), None);
        assert_eq!(
            fts_query(&"word ".repeat(40))
                .unwrap()
                .matches("word")
                .count(),
            MAX_TERMS
        );
    }

    #[test]
    fn transcripts_and_texts_split_into_passages() {
        let cue = |start: f64, text: &str| Cue {
            start,
            end: start + 1.0,
            text: text.to_owned(),
        };
        let passages = transcript_passages(&[
            cue(0.0, "one\ntwo"),
            cue(10.0, "three"),
            cue(31.0, "four"),
            cue(70.0, "five"),
        ]);
        assert_eq!(
            passages,
            [
                (0.0, "one two three".to_owned()),
                (31.0, "four".to_owned()),
                (70.0, "five".to_owned())
            ]
        );
        let long = "x".repeat(700);
        assert_eq!(
            transcript_passages(&[cue(0.0, &long), cue(1.0, "next")]).len(),
            2
        );

        let text = format!("# Title\r\n\r\nFirst paragraph.\n\n\n\nSecond.\n\n{long}\n\nAfter");
        assert_eq!(
            text_passages(&text),
            [
                "# Title\n\nFirst paragraph.\n\nSecond.".to_owned(),
                long.clone(),
                "After".to_owned()
            ]
        );
        assert_eq!(text_passages("  \n\n "), Vec::<String>::new());
    }
}
