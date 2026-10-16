//! Zeron's headless seed server: a Linux-first daemon that keeps Zeron
//! releases available from a machine that peers can always reach.
//!
//! It either follows a magnet link (joining the swarm, downloading the
//! release from whoever has it, then serving it) or serves a release copied
//! to it. It reuses the desktop app's torrent engine adapter and has no
//! remote management: the seed list is edited on the server itself.

pub mod registry;
pub mod server;
