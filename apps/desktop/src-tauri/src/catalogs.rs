//! Publisher catalogs in the Library: import from a file or an HTTPS
//! address, refresh, list, and forget. Files are chosen in native dialogs
//! and stay in Rust; addresses must be plain HTTPS without credentials.

use std::{sync::Arc, time::Duration};

use course_signing::MAX_CATALOG_BYTES;
use learning_application::{CatalogView, CourseLibrary, LibraryError, MAX_CATALOG_URL_BYTES};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    AppState,
    commands::{CommandError, command_error, run_blocking},
};

const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogDto {
    publisher_id: String,
    fingerprint: String,
    title: String,
    issued_at_unix_seconds: u64,
    source_url: Option<String>,
    entries: Vec<CatalogEntryDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogEntryDto {
    course_id: String,
    course_version: String,
    title: String,
    description: String,
    magnet: String,
    info_hash: String,
    /// `available`, `in_library`, `update`, `other_version`, or `other_publisher`.
    status: &'static str,
}

impl From<CatalogView> for CatalogDto {
    fn from(view: CatalogView) -> Self {
        Self {
            publisher_id: view.publisher_id,
            fingerprint: view.fingerprint,
            title: view.title,
            issued_at_unix_seconds: view.issued_at,
            source_url: view.source_url,
            entries: view
                .entries
                .into_iter()
                .map(|item| CatalogEntryDto {
                    status: item.status.as_str(),
                    course_id: item.entry.course_id,
                    course_version: item.entry.course_version,
                    title: item.entry.title,
                    description: item.entry.description,
                    magnet: item.entry.magnet,
                    info_hash: item.entry.info_hash,
                })
                .collect(),
        }
    }
}

/// Every kept catalog with each entry related to the Library.
#[tauri::command]
pub(crate) async fn list_catalogs(
    state: State<'_, AppState>,
) -> Result<Vec<CatalogDto>, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        Ok(lock(&library)?
            .catalogs()
            .map_err(catalog_error)?
            .into_iter()
            .map(CatalogDto::from)
            .collect())
    })
    .await
}

/// Import a catalog file chosen in a native dialog. Returns `None` when the
/// dialog is cancelled.
#[tauri::command]
pub(crate) async fn choose_catalog_file(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<CatalogDto>, CommandError> {
    let selected = run_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .add_filter("Zeron catalog", &["json"])
            .blocking_pick_file())
    })
    .await?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let source = selected.into_path().map_err(|_| {
        command_error(
            "unsupported_file_location",
            "the selected file location is not supported",
        )
    })?;
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        let bytes = read_bounded(&source)?;
        import(&library, &bytes, None).map(Some)
    })
    .await
}

/// Download a catalog from an HTTPS address and keep it, to refresh later.
#[tauri::command]
pub(crate) async fn import_catalog_url(
    url: String,
    state: State<'_, AppState>,
) -> Result<CatalogDto, CommandError> {
    let url = checked_url(&url)?;
    let bytes = fetch(&url).await?;
    let library = Arc::clone(&state.library);
    run_blocking(move || import(&library, &bytes, Some(&url))).await
}

/// Download a kept catalog again from its HTTPS address.
#[tauri::command]
pub(crate) async fn refresh_catalog(
    publisher_id: String,
    state: State<'_, AppState>,
) -> Result<CatalogDto, CommandError> {
    let library = Arc::clone(&state.library);
    let lookup = Arc::clone(&library);
    let url = run_blocking(move || {
        lock(&lookup)?
            .catalog_source(&publisher_id)
            .map_err(catalog_error)?
            .ok_or_else(|| {
                command_error(
                    "catalog_not_followed",
                    "this catalog was imported from a file; import the new file instead",
                )
            })
    })
    .await?;
    let bytes = fetch(&url).await?;
    run_blocking(move || import(&library, &bytes, Some(&url))).await
}

/// Forget a catalog. Courses imported from it stay.
#[tauri::command]
pub(crate) async fn remove_catalog(
    publisher_id: String,
    state: State<'_, AppState>,
) -> Result<bool, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        lock(&library)?
            .remove_catalog(&publisher_id)
            .map_err(catalog_error)
    })
    .await
}

fn import(
    library: &Arc<std::sync::Mutex<CourseLibrary>>,
    bytes: &[u8],
    source_url: Option<&str>,
) -> Result<CatalogDto, CommandError> {
    lock(library)?
        .import_catalog(bytes, source_url)
        .map(CatalogDto::from)
        .map_err(catalog_error)
}

/// A plain HTTPS address: no credentials, at most 2 KiB.
fn checked_url(text: &str) -> Result<String, CommandError> {
    let invalid = || {
        command_error(
            "invalid_input",
            "enter an https:// address of a Zeron catalog",
        )
    };
    let text = text.trim();
    if text.len() > MAX_CATALOG_URL_BYTES {
        return Err(invalid());
    }
    let url = url::Url::parse(text).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid());
    }
    Ok(url.to_string())
}

async fn fetch(url: &str) -> Result<Vec<u8>, CommandError> {
    let failed = |message: &str| command_error("catalog_download_failed", message);
    let client = reqwest::Client::builder()
        .user_agent(concat!("Zeron/", env!("CARGO_PKG_VERSION")))
        .timeout(FETCH_TIMEOUT)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|_| failed("the HTTPS client could not start"))?;
    let mut response = client.get(url).send().await.map_err(|error| {
        if error.is_timeout() {
            failed("the download timed out")
        } else {
            failed("the server could not be reached")
        }
    })?;
    if !response.status().is_success() {
        return Err(failed(&format!(
            "the server answered {}",
            response.status()
        )));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failed("the download was interrupted"))?
    {
        if body.len() + chunk.len() > MAX_CATALOG_BYTES {
            return Err(failed("the catalog is larger than 1 MiB"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn read_bounded(source: &std::path::Path) -> Result<Vec<u8>, CommandError> {
    use std::io::Read as _;
    let unreadable = || command_error("catalog_unreadable", "the chosen file could not be read");
    let mut bytes = Vec::new();
    std::fs::File::open(source)
        .map_err(|_| unreadable())?
        .take(MAX_CATALOG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    Ok(bytes)
}

fn lock(
    library: &Arc<std::sync::Mutex<CourseLibrary>>,
) -> Result<std::sync::MutexGuard<'_, CourseLibrary>, CommandError> {
    library.lock().map_err(|_| {
        command_error(
            "library_unavailable",
            "the course library worker is unavailable",
        )
    })
}

fn catalog_error(error: LibraryError) -> CommandError {
    match error {
        LibraryError::Signature(_) => command_error("catalog_invalid", error),
        // Storage errors can name the database location; keep them in Rust.
        _ => command_error(
            "library_unavailable",
            "the catalog could not be saved to the library",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_https_addresses_are_followed() {
        assert_eq!(
            checked_url(" https://courses.example/catalog.json ").unwrap(),
            "https://courses.example/catalog.json"
        );
        for refused in [
            "http://courses.example/catalog.json",
            "file:///etc/passwd",
            "https://user:secret@courses.example/catalog.json",
            "https://",
            "catalog.json",
        ] {
            assert_eq!(
                checked_url(refused).unwrap_err().code,
                "invalid_input",
                "{refused}"
            );
        }
        assert!(checked_url(&format!("https://courses.example/{}", "a".repeat(3000))).is_err());
    }

    #[tokio::test]
    async fn plain_http_catalogs_are_never_fetched() {
        assert_eq!(
            fetch("http://127.0.0.1:9/catalog.json")
                .await
                .unwrap_err()
                .code,
            "catalog_download_failed"
        );
    }

    #[test]
    fn invalid_catalogs_are_refused_without_storage_details() {
        let library = Arc::new(std::sync::Mutex::new(
            CourseLibrary::open_in_memory(learning_application::ImportLimits::default()).unwrap(),
        ));
        let refused = import(&library, b"{}", None).unwrap_err();
        assert_eq!(refused.code, "catalog_invalid");
    }
}
