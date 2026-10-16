//! Fixing the `AppImage's` `GStreamer` plugin scanner path.
//!
//! The `AppImage` start-up hook from `linuxdeploy-plugin-gstreamer` exports
//! `GST_PLUGIN_SCANNER_1_0` for a Debian-style layout
//! (`usr/lib/gstreamer1.0/gstreamer-1.0/`), while bundles built on other
//! distributions put the scanner in `usr/lib/gstreamer-1.0/`. `GStreamer` then
//! warns once per plugin while it scans. Changing the environment in-process
//! is `unsafe`, which this workspace forbids, so Zeron starts itself again
//! once with the right paths instead.

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

/// Set on the restarted process so it never restarts again.
const RESTARTED: &str = "ZERON_GSTREAMER_PATHS_FIXED";

/// The bundled scanner to use, when running from an `AppImage` whose
/// configured scanner does not exist. `None` means nothing needs fixing.
fn bundled_scanner(
    appdir: Option<&Path>,
    configured: Option<&Path>,
    restarted: bool,
) -> Option<PathBuf> {
    let appdir = appdir?;
    if restarted || configured.is_some_and(Path::is_file) {
        return None;
    }
    let scanner = appdir.join("usr/lib/gstreamer-1.0/gst-plugin-scanner");
    scanner.is_file().then_some(scanner)
}

/// Run Zeron again with `GStreamer` pointed at the bundled scanner, when that
/// is needed. Returns the restarted process's exit code, or `None` to carry
/// on in this process.
pub(crate) fn restart_with_bundled_gstreamer() -> Option<i32> {
    let appdir = env::var_os("APPDIR").map(PathBuf::from);
    let configured = env::var_os("GST_PLUGIN_SCANNER_1_0").map(PathBuf::from);
    let scanner = bundled_scanner(
        appdir.as_deref(),
        configured.as_deref(),
        env::var_os(RESTARTED).is_some(),
    )?;
    let executable = env::current_exe().ok()?;
    let mut command = Command::new(executable);
    command
        .args(env::args_os().skip(1))
        .env(RESTARTED, "1")
        .env("GST_PLUGIN_SCANNER_1_0", &scanner)
        .env("GST_PLUGIN_SCANNER", &scanner);
    let helper = scanner.with_file_name("gst-ptp-helper");
    if helper.is_file() {
        command
            .env("GST_PTP_HELPER_1_0", &helper)
            .env("GST_PTP_HELPER", &helper);
    }
    command
        .status()
        .ok()
        .map(|status| status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_only_inside_an_appimage_whose_scanner_path_is_wrong() {
        let root = env::temp_dir().join(format!("zeron-appimage-{}", std::process::id()));
        let lib = root.join("usr/lib/gstreamer-1.0");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("gst-plugin-scanner"), b"").unwrap();
        let wrong = root.join("usr/lib/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner");

        assert_eq!(
            bundled_scanner(Some(&root), Some(&wrong), false),
            Some(lib.join("gst-plugin-scanner"))
        );
        assert_eq!(
            bundled_scanner(Some(&root), None, false),
            Some(lib.join("gst-plugin-scanner"))
        );
        // Not an AppImage, already restarted, or the configured scanner exists.
        assert_eq!(bundled_scanner(None, Some(&wrong), false), None);
        assert_eq!(bundled_scanner(Some(&root), Some(&wrong), true), None);
        assert_eq!(
            bundled_scanner(Some(&root), Some(&lib.join("gst-plugin-scanner")), false),
            None
        );
        // An AppImage without a bundled scanner is left alone.
        assert_eq!(
            bundled_scanner(Some(&root.join("elsewhere")), Some(&wrong), false),
            None
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
