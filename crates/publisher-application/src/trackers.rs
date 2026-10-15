//! Publisher tracker settings: trackers added by hand, published tracker
//! lists, and the per-list cache of their last successful download.

use crate::{DistributionOptions, DraftServiceError, PublisherDrafts};

const TRACKERS_KEY: &str = "trackers";

/// Most trackers a publisher may list.
pub const MAX_TRACKERS: usize = 100;
/// Longest accepted tracker URL, in bytes.
pub const MAX_TRACKER_URL_BYTES: usize = 512;
/// Budget for all `&tr=` parameters of a magnet link, so a release's magnet
/// stays well inside the Learner's 8 KiB magnet limit.
pub const MAX_MAGNET_TRACKER_BYTES: usize = 6 * 1024;

/// Length of `&tr=<url>` once percent-encoded, as the magnet builder writes it.
fn magnet_parameter_bytes(url: &str) -> usize {
    "&tr=".len()
        + url
            .bytes()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || b"-._*".contains(&byte) {
                    1
                } else {
                    3
                }
            })
            .sum::<usize>()
}

fn acceptable_tracker(url: &str) -> bool {
    url.len() <= MAX_TRACKER_URL_BYTES
        && !url.chars().any(char::is_whitespace)
        && torrent_metainfo::is_tracker_url(url)
}

/// Trim, validate, and de-duplicate tracker URLs, keeping their order.
///
/// # Errors
/// Returns [`DraftServiceError::InvalidInput`] for an overlong URL, anything
/// other than an absolute `http`, `https`, or `udp` URL with a host, more than
/// [`MAX_TRACKERS`] URLs, or more than fits in a magnet link.
pub fn normalize_trackers(urls: &[String]) -> Result<Vec<String>, DraftServiceError> {
    let invalid = |message| DraftServiceError::InvalidInput {
        field: "trackers".to_owned(),
        message,
    };
    let mut trackers: Vec<String> = Vec::new();
    for url in urls
        .iter()
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())
    {
        if url.len() > MAX_TRACKER_URL_BYTES || url.chars().any(char::is_whitespace) {
            return Err(invalid(
                "must be URLs of at most 512 characters without spaces",
            ));
        }
        if !torrent_metainfo::is_tracker_url(url) {
            return Err(invalid("must be http, https, or udp URLs with a host"));
        }
        if !trackers.iter().any(|known| known == url) {
            trackers.push(url.to_owned());
        }
    }
    if trackers.len() > MAX_TRACKERS {
        return Err(invalid("can list at most 100 URLs"));
    }
    if trackers
        .iter()
        .map(|url| magnet_parameter_bytes(url))
        .sum::<usize>()
        > MAX_MAGNET_TRACKER_BYTES
    {
        return Err(invalid("are too long together to fit in a magnet link"));
    }
    Ok(trackers)
}

/// Merge published tracker lists (one URL per line, `#` comments allowed) in
/// order. Unsupported schemes such as `wss` and malformed lines are skipped,
/// duplicates are dropped, and the result always passes [`normalize_trackers`]:
/// merging stops before [`MAX_TRACKERS`] or the magnet budget is exceeded.
#[must_use]
pub fn merge_tracker_lists(lists: &[&str]) -> Vec<String> {
    let mut merged: Vec<String> = Vec::new();
    let mut magnet_bytes = 0;
    for line in lists.iter().flat_map(|list| list.lines()).map(str::trim) {
        if line.is_empty() || line.starts_with('#') || !acceptable_tracker(line) {
            continue;
        }
        if merged.iter().any(|known| known == line) {
            continue;
        }
        let bytes = magnet_parameter_bytes(line);
        if merged.len() == MAX_TRACKERS || magnet_bytes + bytes > MAX_MAGNET_TRACKER_BYTES {
            break;
        }
        magnet_bytes += bytes;
        merged.push(line.to_owned());
    }
    merged
}

const TRACKER_LISTS_KEY: &str = "tracker_lists";
const LIST_CACHE_PREFIX: &str = "tracker_list:";
const LIST_FETCHED_PREFIX: &str = "tracker_list_fetched_at:";

/// Lists used until the publisher edits them: ngosang/trackerslist's "best"
/// list and trackerslist.com's "best" list.
pub const DEFAULT_TRACKER_LISTS: [&str; 2] = [
    "https://cdn.jsdelivr.net/gh/ngosang/trackerslist@master/trackers_best.txt",
    "https://cf.trackerslist.com/best.txt",
];
/// Most tracker lists a publisher may follow.
pub const MAX_TRACKER_LISTS: usize = 10;
/// Largest tracker list body read from the network.
pub const MAX_TRACKER_LIST_BODY_BYTES: usize = 256 * 1024;

/// A followed tracker list and what its last successful download contained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerListStatus {
    /// HTTPS URL of the list.
    pub url: String,
    /// Usable trackers in the last successful download.
    pub tracker_count: usize,
    /// When it was last downloaded successfully; `None` if never.
    pub fetched_at_unix_seconds: Option<u64>,
}

/// Trim, validate, and de-duplicate tracker list URLs, keeping their order.
///
/// # Errors
/// Returns [`DraftServiceError::InvalidInput`] for anything other than an
/// absolute `https` URL with a host of at most 512 bytes, or more than
/// [`MAX_TRACKER_LISTS`] URLs.
pub fn normalize_tracker_lists(urls: &[String]) -> Result<Vec<String>, DraftServiceError> {
    let invalid = |message| DraftServiceError::InvalidInput {
        field: "trackerLists".to_owned(),
        message,
    };
    let mut lists: Vec<String> = Vec::new();
    for url in urls
        .iter()
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())
    {
        let https = url::Url::parse(url)
            .is_ok_and(|parsed| parsed.scheme() == "https" && parsed.host_str().is_some());
        if url.len() > MAX_TRACKER_URL_BYTES || !https {
            return Err(invalid("must be https URLs of at most 512 characters"));
        }
        if !lists.iter().any(|known| known == url) {
            lists.push(url.to_owned());
        }
    }
    if lists.len() > MAX_TRACKER_LISTS {
        return Err(invalid("can list at most 10 URLs"));
    }
    Ok(lists)
}

fn lines(stored: Option<String>) -> Vec<String> {
    stored
        .map(|stored| stored.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

impl PublisherDrafts {
    /// Trackers written into new releases, in order; empty unless the
    /// publisher chose some.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn trackers(&self) -> Result<Vec<String>, DraftServiceError> {
        Ok(self
            .storage
            .setting(TRACKERS_KEY)?
            .map(|stored| stored.lines().map(str::to_owned).collect())
            .unwrap_or_default())
    }

    /// Validate and store the trackers written into new releases. Existing
    /// releases are immutable and keep the trackers they were published with.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidInput`] (see [`normalize_trackers`])
    /// or a storage failure.
    pub fn set_trackers(&mut self, urls: &[String]) -> Result<Vec<String>, DraftServiceError> {
        let trackers = normalize_trackers(urls)?;
        self.storage
            .set_setting(TRACKERS_KEY, &trackers.join("\n"))?;
        Ok(trackers)
    }

    /// Tracker lists followed by this publisher, in order;
    /// [`DEFAULT_TRACKER_LISTS`] until the publisher edits them.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn tracker_lists(&self) -> Result<Vec<String>, DraftServiceError> {
        Ok(match self.storage.setting(TRACKER_LISTS_KEY)? {
            Some(stored) => lines(Some(stored)),
            None => DEFAULT_TRACKER_LISTS.map(str::to_owned).to_vec(),
        })
    }

    /// Validate and store the tracker lists to follow. An empty list stops
    /// following any list.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidInput`] (see
    /// [`normalize_tracker_lists`]) or a storage failure.
    pub fn set_tracker_lists(&mut self, urls: &[String]) -> Result<Vec<String>, DraftServiceError> {
        let lists = normalize_tracker_lists(urls)?;
        self.storage.set_setting(
            TRACKER_LISTS_KEY,
            &lists.join(
                "
",
            ),
        )?;
        Ok(lists)
    }

    /// Each followed list with its cached download.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn tracker_list_statuses(&self) -> Result<Vec<TrackerListStatus>, DraftServiceError> {
        self.tracker_lists()?
            .into_iter()
            .map(|url| {
                let tracker_count = self.cached_list(&url)?.len();
                let fetched_at_unix_seconds = self
                    .storage
                    .setting(&format!("{LIST_FETCHED_PREFIX}{url}"))?
                    .and_then(|value| value.parse().ok());
                Ok(TrackerListStatus {
                    url,
                    tracker_count,
                    fetched_at_unix_seconds,
                })
            })
            .collect()
    }

    /// Cache a successful download of `url`. Lines that are not usable
    /// trackers are dropped. Returns the number of usable trackers.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn store_tracker_list(
        &mut self,
        url: &str,
        body: &str,
        fetched_at_unix_seconds: u64,
    ) -> Result<usize, DraftServiceError> {
        let trackers = merge_tracker_lists(&[body]);
        self.storage.set_setting(
            &format!("{LIST_CACHE_PREFIX}{url}"),
            &trackers.join(
                "
",
            ),
        )?;
        self.storage.set_setting(
            &format!("{LIST_FETCHED_PREFIX}{url}"),
            &fetched_at_unix_seconds.to_string(),
        )?;
        Ok(trackers.len())
    }

    fn cached_list(&self, url: &str) -> Result<Vec<String>, DraftServiceError> {
        Ok(lines(
            self.storage.setting(&format!("{LIST_CACHE_PREFIX}{url}"))?,
        ))
    }

    /// Trackers a new release carries: those added by hand first, then each
    /// followed list's cached download in order, de-duplicated and cut to
    /// what fits in a magnet link. Publishing never waits on the network.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn effective_trackers(&self) -> Result<Vec<String>, DraftServiceError> {
        let mut sources = vec![self.trackers()?.join(
            "
",
        )];
        for url in self.tracker_lists()? {
            sources.push(self.cached_list(&url)?.join(
                "
",
            ));
        }
        let sources: Vec<&str> = sources.iter().map(String::as_str).collect();
        Ok(merge_tracker_lists(&sources))
    }

    /// Distribution options for a new release: one tier per tracker, so
    /// clients announce to every tracker rather than stopping at the first.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn distribution_options(&self) -> Result<DistributionOptions, DraftServiceError> {
        Ok(DistributionOptions {
            tracker_tiers: self
                .effective_trackers()?
                .into_iter()
                .map(|url| vec![url])
                .collect(),
            web_seeds: Vec::new(),
        })
    }
}
