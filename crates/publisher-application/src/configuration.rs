//! Publisher configuration files, for moving a publishing setup between
//! computers.

use serde::{Deserialize, Serialize};
use torrent_port::{NetworkProfile, PeerReachability, RateLimits};

use crate::{DraftServiceError, PublisherDrafts, normalize_tracker_lists, normalize_trackers};

/// The `format` value every configuration file carries.
pub const CONFIGURATION_FORMAT: &str = "zeron.publisher-configuration";
/// The only configuration file version understood.
pub const CONFIGURATION_VERSION: u32 = 1;
/// Largest configuration file read.
pub const MAX_CONFIGURATION_BYTES: usize = 256 * 1024;

/// Validated network settings carried by a configuration file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkConfiguration {
    /// Who can reach this computer.
    pub profile: NetworkProfile,
    /// Fixed peer port and router forwarding.
    pub reachability: PeerReachability,
    /// Speed caps.
    pub limits: RateLimits,
}

/// A validated publisher configuration. Every field has been checked with
/// the same rules as the settings screens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublisherConfiguration {
    /// Trackers written into new releases, in order.
    pub trackers: Vec<String>,
    /// Followed tracker lists, in order.
    pub tracker_lists: Vec<String>,
    /// Network settings, when the file carries them.
    pub network: Option<NetworkConfiguration>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigurationFile {
    format: String,
    version: u32,
    trackers: Vec<String>,
    tracker_lists: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    network: Option<NetworkFile>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NetworkFile {
    profile: String,
    peer_port: Option<u16>,
    port_forwarding: bool,
    download_limit_kib: Option<u32>,
    upload_limit_kib: Option<u32>,
}

impl PublisherConfiguration {
    /// Parse and validate an untrusted configuration file.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidInput`] naming the field that is
    /// wrong: the whole file when it is too large, not JSON, of another
    /// format, or of an unknown version.
    pub fn parse(bytes: &[u8]) -> Result<Self, DraftServiceError> {
        let invalid = |field: &str, message| DraftServiceError::InvalidInput {
            field: field.to_owned(),
            message,
        };
        if bytes.len() > MAX_CONFIGURATION_BYTES {
            return Err(invalid("configuration", "is larger than 256 KiB"));
        }
        let file: ConfigurationFile = serde_json::from_slice(bytes)
            .map_err(|_| invalid("configuration", "is not a Zeron publisher configuration"))?;
        if file.format != CONFIGURATION_FORMAT {
            return Err(invalid(
                "configuration",
                "is not a Zeron publisher configuration",
            ));
        }
        if file.version != CONFIGURATION_VERSION {
            return Err(invalid(
                "configuration",
                "was written by a newer version of Zeron",
            ));
        }
        let network = file
            .network
            .map(|network| {
                Ok::<_, DraftServiceError>(NetworkConfiguration {
                    profile: NetworkProfile::from_name(&network.profile)
                        .ok_or_else(|| invalid("network.profile", "is not a known profile"))?,
                    reachability: PeerReachability::new(network.peer_port, network.port_forwarding)
                        .ok_or_else(|| invalid("network.peerPort", "must be 1024 or higher"))?,
                    limits: RateLimits::from_kib_per_second(
                        network.download_limit_kib,
                        network.upload_limit_kib,
                    )
                    .ok_or_else(|| invalid("network", "speed limits must be at least 16 KiB/s"))?,
                })
            })
            .transpose()?;
        Ok(Self {
            trackers: normalize_trackers(&file.trackers)?,
            tracker_lists: normalize_tracker_lists(&file.tracker_lists)?,
            network,
        })
    }

    /// The configuration as a pretty-printed JSON file.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let file = ConfigurationFile {
            format: CONFIGURATION_FORMAT.to_owned(),
            version: CONFIGURATION_VERSION,
            trackers: self.trackers.clone(),
            tracker_lists: self.tracker_lists.clone(),
            network: self.network.map(|network| {
                let (download_limit_kib, upload_limit_kib) = network.limits.as_kib_per_second();
                NetworkFile {
                    profile: network.profile.name().to_owned(),
                    peer_port: network.reachability.port,
                    port_forwarding: network.reachability.port_forwarding,
                    download_limit_kib,
                    upload_limit_kib,
                }
            }),
        };
        let mut bytes = serde_json::to_vec_pretty(&file).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }
}

impl PublisherDrafts {
    /// This publisher's trackers and followed lists, with the given network
    /// settings, which the app keeps outside publisher storage.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn configuration(
        &self,
        network: Option<NetworkConfiguration>,
    ) -> Result<PublisherConfiguration, DraftServiceError> {
        Ok(PublisherConfiguration {
            trackers: self.trackers()?,
            tracker_lists: self.tracker_lists()?,
            network,
        })
    }

    /// Replace the trackers and followed lists with an imported
    /// configuration's. Its network part is applied by the caller.
    /// Releases already published keep the trackers they carry.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn apply_configuration(
        &mut self,
        configuration: &PublisherConfiguration,
    ) -> Result<(), DraftServiceError> {
        self.set_trackers(&configuration.trackers)?;
        self.set_tracker_lists(&configuration.tracker_lists)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PublisherConfiguration {
        PublisherConfiguration {
            trackers: vec!["udp://tracker.example:6969/announce".to_owned()],
            tracker_lists: vec!["https://lists.example/best.txt".to_owned()],
            network: Some(NetworkConfiguration {
                profile: NetworkProfile::Internet,
                reachability: PeerReachability::new(Some(51_413), true).unwrap(),
                limits: RateLimits::from_kib_per_second(None, Some(512)).unwrap(),
            }),
        }
    }

    #[test]
    fn a_configuration_round_trips_through_its_file() {
        let bytes = sample().to_bytes();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("\"format\": \"zeron.publisher-configuration\""));
        assert!(text.contains("\"peerPort\": 51413"));
        assert_eq!(PublisherConfiguration::parse(&bytes).unwrap(), sample());
        let without_network = PublisherConfiguration {
            network: None,
            ..sample()
        };
        assert_eq!(
            PublisherConfiguration::parse(&without_network.to_bytes()).unwrap(),
            without_network
        );
    }

    fn field(bytes: &[u8]) -> String {
        match PublisherConfiguration::parse(bytes) {
            Err(DraftServiceError::InvalidInput { field, .. }) => field,
            other => panic!("expected invalid input, got {other:?}"),
        }
    }

    #[test]
    fn untrusted_files_are_checked_field_by_field() {
        let valid: serde_json::Value = serde_json::from_slice(&sample().to_bytes()).unwrap();
        let with = |pointer: &str, value: serde_json::Value| {
            let mut changed = valid.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            serde_json::to_vec(&changed).unwrap()
        };
        assert_eq!(field(b"not json"), "configuration");
        assert_eq!(
            field(&with("/format", "something.else".into())),
            "configuration"
        );
        assert_eq!(field(&with("/version", 2.into())), "configuration");
        assert_eq!(
            field(&with("/network/profile", "everywhere".into())),
            "network.profile"
        );
        assert_eq!(
            field(&with("/network/peerPort", 80.into())),
            "network.peerPort"
        );
        assert_eq!(field(&with("/network/uploadLimitKib", 1.into())), "network");
        assert_eq!(
            field(&with(
                "/trackers",
                serde_json::json!(["file:///etc/passwd"])
            )),
            "trackers"
        );
        assert_eq!(
            field(&with(
                "/trackerLists",
                serde_json::json!(["http://plain.example/list.txt"])
            )),
            "trackerLists"
        );
        let mut unknown = valid.clone();
        unknown["extra"] = true.into();
        assert_eq!(
            field(&serde_json::to_vec(&unknown).unwrap()),
            "configuration"
        );
        assert_eq!(
            field(&vec![b' '; MAX_CONFIGURATION_BYTES + 1]),
            "configuration"
        );
    }
}
