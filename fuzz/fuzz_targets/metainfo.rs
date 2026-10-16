//! Bencode and .torrent readers: no panic; decoded values re-encode canonically.
#![no_main]

use torrent_metainfo::{
    PackageFile, decode, decode_dict_with_spans, encode, file_table, info_hash, inspect_torrent,
};

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(value) = decode(data) {
        assert_eq!(decode(&encode(&value)).unwrap(), value);
    }
    let _ = decode_dict_with_spans(data);
    let _ = info_hash(data);
    let _ = file_table(data);
    let files = [PackageFile {
        path: "course.json".to_owned(),
        length: 2,
    }];
    let _ = inspect_torrent(data, &files);
});
