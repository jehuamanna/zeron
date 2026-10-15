use std::{collections::BTreeMap, ops::Range};

use thiserror::Error;

const MAX_DEPTH: usize = 64;

/// A bencoded value. Dictionaries are ordered bytewise by key, which is the
/// canonical BEP 3 ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// Signed integer.
    Integer(i64),
    /// Byte string.
    Bytes(Vec<u8>),
    /// Ordered list.
    List(Vec<Value>),
    /// Dictionary keyed by byte strings.
    Dict(BTreeMap<Vec<u8>, Value>),
}

/// Strict decoding failure for untrusted bencode.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum DecodeError {
    /// The input is not valid bencode at the given byte offset.
    #[error("malformed bencode at byte {0}")]
    Malformed(usize),
    /// Valid bencode that is not in canonical form (key order, leading zeros).
    #[error("non-canonical bencode at byte {0}")]
    NonCanonical(usize),
    /// Nesting exceeds the supported depth.
    #[error("bencode nesting is too deep")]
    TooDeep,
    /// Bytes remain after the top-level value.
    #[error("unexpected bytes after the bencoded value")]
    TrailingBytes,
    /// The top-level value is not a dictionary.
    #[error("expected a bencoded dictionary")]
    NotADictionary,
}

/// Encode a value canonically.
#[must_use]
pub fn encode(value: &Value) -> Vec<u8> {
    let mut output = Vec::new();
    encode_into(value, &mut output);
    output
}

fn encode_into(value: &Value, output: &mut Vec<u8>) {
    match value {
        Value::Integer(number) => {
            output.push(b'i');
            output.extend_from_slice(number.to_string().as_bytes());
            output.push(b'e');
        }
        Value::Bytes(bytes) => encode_bytes(bytes, output),
        Value::List(items) => {
            output.push(b'l');
            for item in items {
                encode_into(item, output);
            }
            output.push(b'e');
        }
        Value::Dict(entries) => {
            output.push(b'd');
            for (key, item) in entries {
                encode_bytes(key, output);
                encode_into(item, output);
            }
            output.push(b'e');
        }
    }
}

fn encode_bytes(bytes: &[u8], output: &mut Vec<u8>) {
    output.extend_from_slice(bytes.len().to_string().as_bytes());
    output.push(b':');
    output.extend_from_slice(bytes);
}

/// Decode one complete, canonical bencoded value.
///
/// # Errors
/// Rejects malformed, non-canonical, too deep, or trailing input.
pub fn decode(input: &[u8]) -> Result<Value, DecodeError> {
    let (value, end) = Parser { input }.value(0, 0)?;
    if end == input.len() {
        Ok(value)
    } else {
        Err(DecodeError::TrailingBytes)
    }
}

/// Top-level dictionary entries with the raw byte span of each value.
pub type DictWithSpans = BTreeMap<Vec<u8>, (Value, Range<usize>)>;

/// Decode a canonical top-level dictionary and report the raw byte span of
/// each value, so callers can hash a nested value exactly as it was encoded.
///
/// # Errors
/// Rejects malformed or non-canonical input and non-dictionary roots.
pub fn decode_dict_with_spans(input: &[u8]) -> Result<DictWithSpans, DecodeError> {
    let parser = Parser { input };
    if input.first() != Some(&b'd') {
        return Err(DecodeError::NotADictionary);
    }
    let mut entries = BTreeMap::new();
    let mut position = 1;
    let mut previous: Option<Vec<u8>> = None;
    loop {
        match input.get(position) {
            Some(b'e') => {
                position += 1;
                break;
            }
            Some(_) => {
                let key_start = position;
                let (key, after_key) = parser.bytes(position)?;
                if previous.as_ref().is_some_and(|last| *last >= key) {
                    return Err(DecodeError::NonCanonical(key_start));
                }
                let (value, end) = parser.value(after_key, 1)?;
                previous = Some(key.clone());
                entries.insert(key, (value, after_key..end));
                position = end;
            }
            None => return Err(DecodeError::Malformed(position)),
        }
    }
    if position == input.len() {
        Ok(entries)
    } else {
        Err(DecodeError::TrailingBytes)
    }
}

struct Parser<'a> {
    input: &'a [u8],
}

impl Parser<'_> {
    fn value(&self, position: usize, depth: usize) -> Result<(Value, usize), DecodeError> {
        if depth > MAX_DEPTH {
            return Err(DecodeError::TooDeep);
        }
        match self.input.get(position) {
            Some(b'i') => self.integer(position),
            Some(b'0'..=b'9') => {
                let (bytes, end) = self.bytes(position)?;
                Ok((Value::Bytes(bytes), end))
            }
            Some(b'l') => {
                let mut items = Vec::new();
                let mut cursor = position + 1;
                loop {
                    if self.input.get(cursor) == Some(&b'e') {
                        return Ok((Value::List(items), cursor + 1));
                    }
                    let (item, end) = self.value(cursor, depth + 1)?;
                    items.push(item);
                    cursor = end;
                }
            }
            Some(b'd') => {
                let mut entries = BTreeMap::new();
                let mut cursor = position + 1;
                let mut previous: Option<Vec<u8>> = None;
                loop {
                    if self.input.get(cursor) == Some(&b'e') {
                        return Ok((Value::Dict(entries), cursor + 1));
                    }
                    let key_start = cursor;
                    let (key, after_key) = self.bytes(cursor)?;
                    if previous.as_ref().is_some_and(|last| *last >= key) {
                        return Err(DecodeError::NonCanonical(key_start));
                    }
                    let (item, end) = self.value(after_key, depth + 1)?;
                    previous = Some(key.clone());
                    entries.insert(key, item);
                    cursor = end;
                }
            }
            _ => Err(DecodeError::Malformed(position)),
        }
    }

    fn integer(&self, position: usize) -> Result<(Value, usize), DecodeError> {
        let start = position + 1;
        let end = self.input[start..]
            .iter()
            .position(|byte| *byte == b'e')
            .map(|offset| start + offset)
            .ok_or(DecodeError::Malformed(position))?;
        let digits = &self.input[start..end];
        let (negative, magnitude) = match digits.split_first() {
            Some((b'-', rest)) => (true, rest),
            _ => (false, digits),
        };
        if magnitude.is_empty() || !magnitude.iter().all(u8::is_ascii_digit) {
            return Err(DecodeError::Malformed(position));
        }
        if (magnitude.len() > 1 && magnitude[0] == b'0') || (negative && magnitude == b"0") {
            return Err(DecodeError::NonCanonical(position));
        }
        let text = std::str::from_utf8(digits).map_err(|_| DecodeError::Malformed(position))?;
        let number = text
            .parse::<i64>()
            .map_err(|_| DecodeError::Malformed(position))?;
        Ok((Value::Integer(number), end + 1))
    }

    fn bytes(&self, position: usize) -> Result<(Vec<u8>, usize), DecodeError> {
        let colon = self.input[position..]
            .iter()
            .take(21)
            .position(|byte| *byte == b':')
            .map(|offset| position + offset)
            .ok_or(DecodeError::Malformed(position))?;
        let digits = &self.input[position..colon];
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return Err(DecodeError::Malformed(position));
        }
        if digits.len() > 1 && digits[0] == b'0' {
            return Err(DecodeError::NonCanonical(position));
        }
        let length = std::str::from_utf8(digits)
            .ok()
            .and_then(|text| text.parse::<usize>().ok())
            .ok_or(DecodeError::Malformed(position))?;
        let start = colon + 1;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= self.input.len())
            .ok_or(DecodeError::Malformed(position))?;
        Ok((self.input[start..end].to_vec(), end))
    }
}
