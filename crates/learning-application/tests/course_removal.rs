//! Removing a course deletes only Zeron's own torrent downloads, never
//! follows links, and finishes after a crash (ADR 0015).

use std::{
    fs,
    path::{Path, PathBuf},
};

use learning_application::{DELETING_SUFFIX, finish_interrupted_removals, set_aside_course_files};
use learning_storage::TorrentCourseSource;

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("zeron-removal-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const HASH: &str = "abababababababababababababababababababab";
const OTHER_HASH: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

fn source(course_id: &str, info_hash: &str, content_root: PathBuf) -> TorrentCourseSource {
    TorrentCourseSource {
        course_id: course_id.to_owned(),
        info_hash: info_hash.to_owned(),
        metainfo: Vec::new(),
        content_root,
        sharing: false,
        paused: false,
    }
}

fn download(root: &Path, hash: &str) -> PathBuf {
    let directory = root.join(hash);
    fs::create_dir_all(directory.join("lessons")).unwrap();
    fs::write(directory.join("course.json"), b"{}").unwrap();
    fs::write(directory.join("lessons/one.mp4"), b"video").unwrap();
    directory
}

#[test]
fn a_removed_torrent_course_download_is_set_aside_then_deleted_or_put_back() {
    let temp = TempRoot::new("aside");
    let directory = download(&temp.0, HASH);
    let removed = source("course", HASH, directory.clone());

    let aside = set_aside_course_files(&temp.0, &removed, std::slice::from_ref(&removed))
        .unwrap()
        .unwrap();
    assert!(!directory.exists());
    assert!(temp.0.join(format!("{HASH}{DELETING_SUFFIX}")).is_dir());
    aside.restore().unwrap();
    assert!(
        directory.join("lessons/one.mp4").is_file(),
        "a failed removal puts the files back"
    );

    let aside = set_aside_course_files(&temp.0, &removed, std::slice::from_ref(&removed))
        .unwrap()
        .unwrap();
    aside.delete().unwrap();
    assert!(!directory.exists());
    assert!(!temp.0.join(format!("{HASH}{DELETING_SUFFIX}")).exists());
}

#[test]
fn only_zerons_own_unshared_download_directory_is_ever_touched() {
    let temp = TempRoot::new("refuse");
    let directory = download(&temp.0, HASH);
    let elsewhere = TempRoot::new("refuse-elsewhere");

    // A source pointing anywhere but courses_root/<hash> is left alone.
    let foreign = source("course", HASH, elsewhere.0.clone());
    assert!(
        set_aside_course_files(&temp.0, &foreign, std::slice::from_ref(&foreign))
            .unwrap()
            .is_none()
    );
    // A torrent another course still uses is kept.
    let removed = source("course", HASH, directory.clone());
    let sibling = source("course_copy", HASH, directory.clone());
    assert!(
        set_aside_course_files(&temp.0, &removed, &[removed.clone(), sibling])
            .unwrap()
            .is_none()
    );
    // An invalid info hash names nothing Zeron made.
    let bogus = source("course", "../outside", temp.0.join("../outside"));
    assert!(
        set_aside_course_files(&temp.0, &bogus, std::slice::from_ref(&bogus))
            .unwrap()
            .is_none()
    );
    // Nothing downloaded yet.
    let empty = source("course", OTHER_HASH, temp.0.join(OTHER_HASH));
    assert!(
        set_aside_course_files(&temp.0, &empty, std::slice::from_ref(&empty))
            .unwrap()
            .is_none()
    );
    assert!(directory.join("lessons/one.mp4").is_file());
}

#[cfg(unix)]
#[test]
fn links_are_never_followed() {
    let temp = TempRoot::new("links");
    let outside = TempRoot::new("links-outside");
    fs::write(outside.0.join("precious.txt"), b"keep").unwrap();

    // The download directory itself is a link: refused.
    std::os::unix::fs::symlink(&outside.0, temp.0.join(HASH)).unwrap();
    let linked = source("course", HASH, temp.0.join(HASH));
    assert!(
        set_aside_course_files(&temp.0, &linked, std::slice::from_ref(&linked))
            .unwrap()
            .is_none()
    );
    fs::remove_file(temp.0.join(HASH)).unwrap();

    // A link inside the download is removed, not followed.
    let directory = download(&temp.0, HASH);
    std::os::unix::fs::symlink(&outside.0, directory.join("lessons/escape")).unwrap();
    let removed = source("course", HASH, directory);
    set_aside_course_files(&temp.0, &removed, std::slice::from_ref(&removed))
        .unwrap()
        .unwrap()
        .delete()
        .unwrap();
    assert_eq!(fs::read(outside.0.join("precious.txt")).unwrap(), b"keep");

    // A `.deleting` link left in the root is not followed at start-up either.
    std::os::unix::fs::symlink(
        &outside.0,
        temp.0.join(format!("{OTHER_HASH}{DELETING_SUFFIX}")),
    )
    .unwrap();
    assert_eq!(finish_interrupted_removals(&temp.0).unwrap(), 0);
    assert_eq!(fs::read(outside.0.join("precious.txt")).unwrap(), b"keep");
}

#[test]
fn start_up_finishes_removals_a_crash_interrupted_and_nothing_else() {
    let temp = TempRoot::new("startup");
    let interrupted = download(&temp.0, HASH);
    fs::rename(
        &interrupted,
        temp.0.join(format!("{HASH}{DELETING_SUFFIX}")),
    )
    .unwrap();
    let kept = download(&temp.0, OTHER_HASH);
    fs::create_dir_all(temp.0.join(format!("not-a-hash{DELETING_SUFFIX}"))).unwrap();

    assert_eq!(finish_interrupted_removals(&temp.0).unwrap(), 1);
    assert!(!temp.0.join(format!("{HASH}{DELETING_SUFFIX}")).exists());
    assert!(
        kept.join("course.json").is_file(),
        "live downloads are kept"
    );
    assert!(
        temp.0.join(format!("not-a-hash{DELETING_SUFFIX}")).is_dir(),
        "only Zeron's own names are touched"
    );
    assert_eq!(
        finish_interrupted_removals(&temp.0.join("missing")).unwrap(),
        0
    );
}
