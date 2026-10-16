//! Deterministic input mutation for parser robustness tests on stable Rust.
//!
//! Each run derives every mutation from a fixed seed, so a failure names the
//! seed and iteration that reproduce it. The `fuzz/` targets explore the same
//! parsers far deeper with libFuzzer on nightly; these tests keep a fast,
//! always-on check in the ordinary test suite.

/// A small, fast pseudo-random generator (xorshift64*). Not cryptographic.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator from a non-zero seed.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    /// The next 64 random bits.
    pub const fn next_u64(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;
        self.0 = value;
        value.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number below `bound`, which must be non-zero.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the remainder is below a usize bound"
    )]
    pub const fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

/// Bytes that often change how parsers branch.
const INTERESTING: &[&[u8]] = &[
    b"0",
    b"-1",
    b"18446744073709551615",
    b"99999999999999999999",
    b"\"",
    b"\\",
    b"{",
    b"}",
    b"[",
    b"]",
    b",",
    b":",
    b"null",
    b"e",
    b"i",
    b"l",
    b"d",
    b"..",
    b"/",
    b"\0",
    b"\xff",
    b"\xf0\x9f\x98\x80",
    b"bytes=",
    b"-",
];

/// `count` mutations of the seeds, each derived from `seed`. Mutations flip
/// bits, overwrite, insert, delete, duplicate, truncate, splice two seeds,
/// and insert values that often change parsing. Outputs never exceed
/// `max_len` bytes.
#[must_use]
pub fn mutations(seeds: &[&[u8]], seed: u64, count: usize, max_len: usize) -> Vec<Vec<u8>> {
    let mut rng = Rng::new(seed);
    let mut outputs = Vec::with_capacity(count);
    for _ in 0..count {
        let mut data = seeds[rng.below(seeds.len())].to_vec();
        for _ in 0..=rng.below(4) {
            mutate(&mut data, seeds, &mut rng);
        }
        data.truncate(max_len);
        outputs.push(data);
    }
    outputs
}

fn mutate(data: &mut Vec<u8>, seeds: &[&[u8]], rng: &mut Rng) {
    let position = |rng: &mut Rng, data: &Vec<u8>| rng.below(data.len().max(1));
    match rng.below(8) {
        0 if !data.is_empty() => {
            let at = position(rng, data);
            data[at] ^= 1 << rng.below(8);
        }
        1 if !data.is_empty() => {
            let at = position(rng, data);
            data[at] = rng.next_u64().to_le_bytes()[0];
        }
        2 => {
            let at = rng.below(data.len() + 1);
            data.insert(at, rng.next_u64().to_le_bytes()[0]);
        }
        3 if !data.is_empty() => {
            let at = position(rng, data);
            let end = (at + 1 + rng.below(16)).min(data.len());
            data.drain(at..end);
        }
        4 if !data.is_empty() => {
            let at = position(rng, data);
            let end = (at + 1 + rng.below(32)).min(data.len());
            let copy: Vec<u8> = data[at..end].to_vec();
            let to = rng.below(data.len() + 1);
            data.splice(to..to, copy);
        }
        5 => {
            let keep = rng.below(data.len() + 1);
            data.truncate(keep);
        }
        6 => {
            let other = seeds[rng.below(seeds.len())];
            let cut = rng.below(data.len() + 1);
            let from = rng.below(other.len() + 1);
            data.truncate(cut);
            data.extend_from_slice(&other[from..]);
        }
        _ => {
            let value = INTERESTING[rng.below(INTERESTING.len())];
            let at = rng.below(data.len() + 1);
            data.splice(at..at, value.iter().copied());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutations_are_deterministic_bounded_and_varied() {
        let seeds: &[&[u8]] = &[b"{\"a\":1}", b"d3:keyi42ee"];
        let first = mutations(seeds, 7, 500, 64);
        assert_eq!(first, mutations(seeds, 7, 500, 64));
        assert_ne!(first, mutations(seeds, 8, 500, 64));
        assert!(first.iter().all(|data| data.len() <= 64));
        let distinct: std::collections::BTreeSet<_> = first.iter().collect();
        assert!(distinct.len() > 400);
    }
}
