//! Downloads published tracker lists. Only the bodies cross into
//! `publisher-application`, which validates and caches them.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::future::join_all;
use publisher_application::MAX_TRACKER_LIST_BODY_BYTES;

const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
/// Lists change slowly; refresh a cached copy after a day.
pub(crate) const STALE_AFTER_SECONDS: u64 = 24 * 60 * 60;

pub(crate) fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Fetch every list concurrently; each result pairs the URL with its body or
/// a short reason it could not be read.
pub(crate) async fn fetch_all(urls: Vec<String>) -> Vec<(String, Result<String, String>)> {
    let client = match reqwest::Client::builder()
        .user_agent(concat!("Zeron-Publisher/", env!("CARGO_PKG_VERSION")))
        .timeout(FETCH_TIMEOUT)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            let reason = format!("cannot start the HTTPS client: {error}");
            return urls
                .into_iter()
                .map(|url| (url, Err(reason.clone())))
                .collect();
        }
    };
    join_all(urls.into_iter().map(|url| {
        let client = client.clone();
        async move {
            let body = fetch(&client, &url).await;
            (url, body)
        }
    }))
    .await
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let mut response = client.get(url).send().await.map_err(describe)?;
    if !response.status().is_success() {
        return Err(format!("the server answered {}", response.status()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(describe)? {
        if body.len() + chunk.len() > MAX_TRACKER_LIST_BODY_BYTES {
            return Err("the list is larger than 256 KiB".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| "the list is not UTF-8 text".to_owned())
}

fn describe(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "the download timed out".to_owned()
    } else if error.is_connect() {
        "the server could not be reached".to_owned()
    } else {
        error.without_url().to_string()
    }
}

#[cfg(test)]
mod tests {
    use publisher_application::{DEFAULT_TRACKER_LISTS, merge_tracker_lists};

    use super::*;

    /// Downloads the real default lists, so it needs the internet.
    #[tokio::test]
    #[ignore = "downloads the public default tracker lists"]
    async fn the_default_lists_download_and_yield_trackers() {
        let fetched = fetch_all(DEFAULT_TRACKER_LISTS.map(str::to_owned).to_vec()).await;
        for (url, body) in fetched {
            let body = body.unwrap_or_else(|error| panic!("{url}: {error}"));
            let trackers = merge_tracker_lists(&[&body]);
            println!("{url}: {} trackers", trackers.len());
            assert!(trackers.len() >= 10, "{url}");
        }
    }

    #[tokio::test]
    async fn plain_http_lists_are_refused() {
        let fetched = fetch_all(vec!["http://127.0.0.1:9/list.txt".to_owned()]).await;
        assert!(fetched[0].1.is_err());
    }
}
