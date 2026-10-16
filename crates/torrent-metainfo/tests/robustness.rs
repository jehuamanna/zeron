//! Mutated metainfo never panics the decoder or the torrent readers, and
//! anything decoded re-encodes canonically.

use std::fs;

use mutation_testing::mutations;
use torrent_metainfo::{
    PackageFile, TorrentOptions, build_torrent, decode, decode_dict_with_spans, encode, file_table,
    info_hash, inspect_torrent,
};

fn real_torrent() -> (Vec<u8>, Vec<PackageFile>) {
    let root = std::env::temp_dir().join(format!("zeron-metainfo-fuzz-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("resources")).unwrap();
    fs::write(root.join("course.json"), b"{}").unwrap();
    fs::write(root.join("resources/a.bin"), vec![1_u8; 40_000]).unwrap();
    let files = vec![
        PackageFile {
            path: "course.json".to_owned(),
            length: 2,
        },
        PackageFile {
            path: "resources/a.bin".to_owned(),
            length: 40_000,
        },
    ];
    let built = build_torrent(
        &root,
        &files,
        &TorrentOptions {
            name: "course".to_owned(),
            tracker_tiers: vec![vec!["udp://tracker.example:6969/announce".to_owned()]],
            web_seeds: Vec::new(),
            comment: None,
            created_by: None,
            creation_date: None,
        },
        &|| false,
        &mut |_| {},
    )
    .unwrap();
    fs::remove_dir_all(root).unwrap();
    (built.bytes, files)
}

#[test]
fn mutated_metainfo_is_refused_or_decodes_canonically() {
    let (torrent, files) = real_torrent();
    let seeds: &[&[u8]] = &[
        &torrent,
        b"d4:infod6:lengthi1eee",
        b"li1ei-1e3:abce",
        b"i9223372036854775807e",
    ];
    for (index, input) in mutations(seeds, 0x0BAD_5EED, 5_000, 256 * 1024)
        .iter()
        .enumerate()
    {
        if let Ok(value) = decode(input) {
            let encoded = encode(&value);
            assert_eq!(
                decode(&encoded).unwrap(),
                value,
                "mutation {index} did not re-encode"
            );
        }
        let _ = decode_dict_with_spans(input);
        let _ = info_hash(input);
        let _ = file_table(input);
        let _ = inspect_torrent(input, &files);
    }
}
