//! Bencode, deterministic v1 creation, and independent verification tests.

use std::{collections::BTreeMap, fs, path::PathBuf};

use torrent_metainfo::{
    DecodeError, PackageFile, TorrentError, TorrentOptions, Value, build_torrent, decode,
    decode_dict_with_spans, encode, file_table, info_hash, piece_length_for, verify_torrent,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("zeron-torrent-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, path: &str, bytes: &[u8]) -> PackageFile {
        let target = self.0.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, bytes).unwrap();
        PackageFile {
            path: path.to_owned(),
            length: bytes.len() as u64,
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn pattern(length: usize, seed: u8) -> Vec<u8> {
    (0..length)
        .map(|index| index.to_le_bytes()[0].wrapping_mul(31).wrapping_add(seed))
        .collect()
}

fn package(scratch: &Scratch) -> Vec<PackageFile> {
    vec![
        scratch.write("resources/b/video.mp4", &pattern(20_000, 1)),
        scratch.write("course.json", &pattern(10_000, 2)),
        scratch.write("resources/a/notes.pdf", &pattern(5_000, 3)),
    ]
}

fn options() -> TorrentOptions {
    TorrentOptions {
        name: "course_rust-1.0.0".to_owned(),
        created_by: Some("Zeron Publisher".to_owned()),
        ..TorrentOptions::default()
    }
}

fn never() -> bool {
    false
}

#[test]
fn bencode_round_trips_canonical_vectors() {
    let bytes = b"d3:cow3:moo4:sizei-42e4:spaml1:a1:bee";
    let value = decode(bytes).unwrap();
    assert_eq!(encode(&value), bytes);
    let Value::Dict(entries) = value else {
        panic!("expected a dictionary")
    };
    assert_eq!(entries[b"size".as_slice()], Value::Integer(-42));

    let mut dict = BTreeMap::new();
    dict.insert(b"zeta".to_vec(), Value::Integer(0));
    dict.insert(b"alpha".to_vec(), Value::Bytes(b"x".to_vec()));
    assert_eq!(encode(&Value::Dict(dict)), b"d5:alpha1:x4:zetai0ee");
}

#[test]
fn bencode_rejects_non_canonical_and_malformed_input() {
    assert!(matches!(
        decode(b"d4:spam1:a3:cow1:be"),
        Err(DecodeError::NonCanonical(_))
    ));
    assert!(matches!(
        decode(b"d3:cow1:a3:cow1:be"),
        Err(DecodeError::NonCanonical(_))
    ));
    assert!(matches!(decode(b"i03e"), Err(DecodeError::NonCanonical(_))));
    assert!(matches!(decode(b"i-0e"), Err(DecodeError::NonCanonical(_))));
    assert!(matches!(
        decode(b"03:abc"),
        Err(DecodeError::NonCanonical(_))
    ));
    assert!(matches!(decode(b"i1ei2e"), Err(DecodeError::TrailingBytes)));
    assert!(matches!(decode(b"5:abc"), Err(DecodeError::Malformed(_))));
    assert!(matches!(
        decode(b"i99999999999999999999e"),
        Err(DecodeError::Malformed(_))
    ));
    assert!(matches!(decode(b"ie"), Err(DecodeError::Malformed(_))));
    assert!(matches!(decode(b"l"), Err(DecodeError::Malformed(_))));
    let deep = [vec![b'l'; 70], vec![b'e'; 70]].concat();
    assert!(matches!(decode(&deep), Err(DecodeError::TooDeep)));
    assert!(matches!(
        decode_dict_with_spans(b"li1ee"),
        Err(DecodeError::NotADictionary)
    ));
}

#[test]
fn bencode_decoder_never_panics_on_arbitrary_bytes() {
    let seeds: [&[u8]; 4] = [
        b"d3:cow3:moo4:sizei-42e4:spaml1:a1:bee",
        b"li1ei2e3:abce",
        b"d1:ad1:bl1:ceee",
        b"i12345e",
    ];
    let mut state = 0x2545_f491_u32;
    for round in 0..5_000 {
        let mut bytes = seeds[round % seeds.len()].to_vec();
        for _ in 0..3 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let index = state as usize % bytes.len();
            bytes[index] = state.to_be_bytes()[1];
        }
        bytes.truncate(bytes.len() - state as usize % 3);
        let _ = decode(&bytes);
        let _ = decode_dict_with_spans(&bytes);
    }
}

#[test]
fn piece_length_policy_is_deterministic_and_bounded() {
    assert_eq!(piece_length_for(0), 16 * 1024);
    assert_eq!(piece_length_for(32 * 1024 * 1024), 16 * 1024);
    assert_eq!(piece_length_for(32 * 1024 * 1024 + 1), 32 * 1024);
    assert_eq!(piece_length_for(4 * 1024 * 1024 * 1024), 2 * 1024 * 1024);
    assert_eq!(piece_length_for(1 << 50), 16 * 1024 * 1024);
}

#[test]
fn identical_packages_build_identical_verifiable_torrents() {
    let scratch = Scratch::new("build");
    let files = package(&scratch);
    let mut reversed = files.clone();
    reversed.reverse();
    let mut events = Vec::new();
    let first = build_torrent(&scratch.0, &files, &options(), &never, &mut |progress| {
        events.push(progress);
    })
    .unwrap();
    let second = build_torrent(&scratch.0, &reversed, &options(), &never, &mut |_| {}).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.piece_length, 16 * 1024);
    assert_eq!(first.total_length, 35_000);
    assert_eq!(first.piece_count, 3);
    assert_eq!(first.info_hash.len(), 40);
    assert_eq!(events.last().unwrap().hashed_bytes, 35_000);
    assert_eq!(
        first.magnet,
        format!(
            "magnet:?xt=urn:btih:{}&dn=course_rust-1.0.0",
            first.info_hash
        )
    );

    let top = decode_dict_with_spans(&first.bytes).unwrap();
    let keys: Vec<_> = top
        .keys()
        .map(|key| String::from_utf8(key.clone()).unwrap())
        .collect();
    assert_eq!(keys, ["created by", "info"]);
    let Value::Dict(info) = &top[b"info".as_slice()].0 else {
        panic!("info must be a dictionary")
    };
    let info_keys: Vec<_> = info
        .keys()
        .map(|key| String::from_utf8(key.clone()).unwrap())
        .collect();
    assert_eq!(info_keys, ["files", "name", "piece length", "pieces"]);
    let Value::List(entries) = &info[b"files".as_slice()] else {
        panic!("files must be a list")
    };
    let first_path = &entries[0];
    assert_eq!(
        encode(first_path),
        b"d6:lengthi10000e4:pathl11:course.jsonee".to_vec()
    );

    let verified = verify_torrent(&first.bytes, &scratch.0, &files, &never, &mut |_| {}).unwrap();
    assert_eq!(verified.info_hash, first.info_hash);
    assert_eq!(info_hash(&first.bytes).unwrap(), first.info_hash);
    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!(file_table(&first.bytes).unwrap(), sorted);
    assert!(file_table(b"d4:infod4:name1:xee").is_err());
    assert!(info_hash(b"d4:name4:teste").is_err());
    assert!(info_hash(b"not bencode").is_err());
    assert_eq!(verified.piece_count, 3);
    assert_eq!(verified.name, "course_rust-1.0.0");
}

#[test]
fn optional_fields_and_trackers_are_explicit() {
    let scratch = Scratch::new("options");
    let files = package(&scratch);
    let options = TorrentOptions {
        tracker_tiers: vec![
            vec!["udp://tracker.example:6969/announce".to_owned()],
            vec!["https://backup.example/announce".to_owned()],
        ],
        web_seeds: vec!["https://cdn.example/courses/".to_owned()],
        comment: Some("Release 1".to_owned()),
        creation_date: Some(1_760_000_000),
        ..options()
    };
    let built = build_torrent(&scratch.0, &files, &options, &never, &mut |_| {}).unwrap();
    let plain = build_torrent(&scratch.0, &files, &self::options(), &never, &mut |_| {}).unwrap();
    assert_eq!(built.info_hash, plain.info_hash);
    let top = decode_dict_with_spans(&built.bytes).unwrap();
    let keys: Vec<_> = top
        .keys()
        .map(|key| String::from_utf8(key.clone()).unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "announce",
            "announce-list",
            "comment",
            "created by",
            "creation date",
            "info",
            "url-list"
        ]
    );
    assert_eq!(
        top[b"announce".as_slice()].0,
        Value::Bytes(b"udp://tracker.example:6969/announce".to_vec())
    );
    assert!(built.magnet.contains("&tr=udp%3A%2F%2Ftracker.example%3A6969%2Fannounce&tr=https%3A%2F%2Fbackup.example%2Fannounce"));
    assert!(
        built
            .magnet
            .ends_with("&ws=https%3A%2F%2Fcdn.example%2Fcourses%2F")
    );
}

#[test]
fn verification_detects_tampering_and_mismatched_tables() {
    let scratch = Scratch::new("tamper");
    let files = package(&scratch);
    let built = build_torrent(&scratch.0, &files, &options(), &never, &mut |_| {}).unwrap();

    let mut tampered = fs::read(scratch.0.join("resources/b/video.mp4")).unwrap();
    tampered[12_345] ^= 1;
    fs::write(scratch.0.join("resources/b/video.mp4"), &tampered).unwrap();
    assert!(matches!(
        verify_torrent(&built.bytes, &scratch.0, &files, &never, &mut |_| {}),
        Err(TorrentError::Verification(message)) if message.contains("piece hash")
    ));
    tampered[12_345] ^= 1;
    fs::write(scratch.0.join("resources/b/video.mp4"), &tampered).unwrap();
    verify_torrent(&built.bytes, &scratch.0, &files, &never, &mut |_| {}).unwrap();

    let mut fewer = files.clone();
    fewer.pop();
    assert!(matches!(
        verify_torrent(&built.bytes, &scratch.0, &fewer, &never, &mut |_| {}),
        Err(TorrentError::Verification(_))
    ));
    let mut corrupted = built.bytes.clone();
    let last = corrupted.len() - 2;
    corrupted[last] ^= 0xff;
    assert!(verify_torrent(&corrupted, &scratch.0, &files, &never, &mut |_| {}).is_err());
}

#[test]
fn files_must_exist_with_declared_lengths_and_safe_paths() {
    let scratch = Scratch::new("mismatch");
    let mut files = package(&scratch);
    files[0].length += 1;
    assert!(matches!(
        build_torrent(&scratch.0, &files, &options(), &never, &mut |_| {}),
        Err(TorrentError::FileMismatch { path }) if path == "resources/b/video.mp4"
    ));
    for bad in ["../escape", "a//b", "/abs", "a\\b", "c:/x", "./x"] {
        let files = vec![PackageFile {
            path: bad.to_owned(),
            length: 0,
        }];
        assert!(
            matches!(
                build_torrent(&scratch.0, &files, &options(), &never, &mut |_| {}),
                Err(TorrentError::InvalidPath(_))
            ),
            "{bad}"
        );
    }
    let duplicate = vec![package(&scratch)[1].clone(), package(&scratch)[1].clone()];
    assert!(matches!(
        build_torrent(&scratch.0, &duplicate, &options(), &never, &mut |_| {}),
        Err(TorrentError::InvalidPath(_))
    ));
    assert!(matches!(
        build_torrent(&scratch.0, &[], &options(), &never, &mut |_| {}),
        Err(TorrentError::Empty)
    ));
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_never_followed() {
    let scratch = Scratch::new("symlink");
    let outside = Scratch::new("symlink-outside");
    let secret = outside.write("secret.txt", b"secret");
    std::os::unix::fs::symlink(outside.0.join("secret.txt"), scratch.0.join("course.json"))
        .unwrap();
    let files = vec![PackageFile {
        path: "course.json".to_owned(),
        length: secret.length,
    }];
    assert!(matches!(
        build_torrent(&scratch.0, &files, &options(), &never, &mut |_| {}),
        Err(TorrentError::FileMismatch { .. })
    ));
    fs::create_dir_all(outside.0.join("dir")).unwrap();
    fs::write(outside.0.join("dir/file.bin"), b"data").unwrap();
    std::os::unix::fs::symlink(outside.0.join("dir"), scratch.0.join("resources")).unwrap();
    let files = vec![PackageFile {
        path: "resources/file.bin".to_owned(),
        length: 4,
    }];
    assert!(matches!(
        build_torrent(&scratch.0, &files, &options(), &never, &mut |_| {}),
        Err(TorrentError::FileMismatch { .. })
    ));
}

#[test]
fn invalid_options_are_rejected_before_hashing() {
    let scratch = Scratch::new("invalid-options");
    let files = package(&scratch);
    for options in [
        TorrentOptions {
            name: String::new(),
            ..options()
        },
        TorrentOptions {
            name: "a/b".to_owned(),
            ..options()
        },
        TorrentOptions {
            tracker_tiers: vec![vec![]],
            ..options()
        },
        TorrentOptions {
            tracker_tiers: vec![vec!["ftp://tracker.example/".to_owned()]],
            ..options()
        },
        TorrentOptions {
            web_seeds: vec!["udp://seed.example/".to_owned()],
            ..options()
        },
    ] {
        assert!(matches!(
            build_torrent(&scratch.0, &files, &options, &never, &mut |_| {}),
            Err(TorrentError::InvalidOptions(_))
        ));
    }
}

#[test]
fn hashing_can_be_cancelled() {
    let scratch = Scratch::new("cancel");
    let files = package(&scratch);
    let cancelled = || true;
    assert!(matches!(
        build_torrent(&scratch.0, &files, &options(), &cancelled, &mut |_| {}),
        Err(TorrentError::Cancelled)
    ));
}

#[test]
fn inspection_checks_structure_without_reading_package_bytes() {
    let scratch = Scratch::new("inspect");
    let files = package(&scratch);
    let built = build_torrent(&scratch.0, &files, &options(), &never, &mut |_| {}).unwrap();
    fs::remove_dir_all(scratch.0.join("resources")).unwrap();
    let summary = torrent_metainfo::inspect_torrent(&built.bytes, &files).unwrap();
    assert_eq!(summary.info_hash, built.info_hash);
    assert_eq!(summary.piece_count, built.piece_count);
    let mut other = files.clone();
    other[0].length += 1;
    assert!(matches!(
        torrent_metainfo::inspect_torrent(&built.bytes, &other),
        Err(TorrentError::Verification(_))
    ));
}
