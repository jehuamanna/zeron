//! Engine-neutral, deterministic `BitTorrent` v1 metainfo creation and verification.

mod bencode;
mod builder;
mod verify;

pub use bencode::{DecodeError, DictWithSpans, Value, decode, decode_dict_with_spans, encode};
pub use builder::{
    BuiltTorrent, HashProgress, PackageFile, TorrentError, TorrentOptions, build_torrent,
    piece_length_for,
};
pub use verify::{VerifiedTorrent, inspect_torrent, verify_torrent};
