//! Authenticated loopback delivery for explicitly registered media resources.

use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::get,
};
use futures_util::StreamExt;
use thiserror::Error;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, SeekFrom},
    net::TcpListener,
    sync::Semaphore,
    task::JoinHandle,
};
use tokio_util::{io::ReaderStream, sync::CancellationToken};
use tower_http::timeout::ResponseBodyTimeoutLayer;

const READ_CHUNK_BYTES: usize = 64 * 1024;

/// Resource metadata accepted by the gateway after application-level lookup.
#[derive(Debug, Clone)]
pub struct FileResource {
    /// Backend-only path to a previously verified file.
    pub path: PathBuf,
    /// Manifest-declared media type.
    pub media_type: String,
    /// Manifest-declared byte length.
    pub byte_length: u64,
}

/// Browser-safe result of registering a resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRegistration {
    /// Authenticated, process-local URL. Treat this value as a bearer secret.
    pub url: String,
    /// Media type that will be returned by the gateway.
    pub media_type: String,
    /// Total resource length.
    pub byte_length: u64,
}

/// Resource registration or server-start failure.
#[derive(Debug, Error)]
pub enum GatewayError {
    /// The loopback listener could not start.
    #[error("cannot start the loopback media gateway: {0}")]
    Listen(#[source] std::io::Error),
    /// Cryptographically secure random bytes were unavailable.
    #[error("cannot generate a media capability: {0}")]
    Random(#[source] getrandom::Error),
    /// The resource is missing, not a regular file, or no longer matches its manifest.
    #[error("the registered media resource is unavailable or has changed")]
    ResourceChanged,
    /// The bounded registry has reached capacity.
    #[error("the media resource registry is full")]
    RegistryFull,
    /// The registry lock is unavailable.
    #[error("the media resource registry is unavailable")]
    RegistryUnavailable,
}

/// Resource and request limits for one gateway process.
#[derive(Debug, Clone, Copy)]
pub struct GatewayLimits {
    /// Maximum resources registered concurrently.
    pub max_registered_resources: usize,
    /// Maximum media response bodies streamed concurrently.
    pub max_concurrent_streams: usize,
    /// Maximum pause between response body frames.
    pub body_frame_timeout: Duration,
}

impl Default for GatewayLimits {
    fn default() -> Self {
        Self {
            max_registered_resources: 256,
            max_concurrent_streams: 8,
            body_frame_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone)]
struct RegisteredResource {
    path: PathBuf,
    media_type: String,
    byte_length: u64,
}

#[derive(Clone)]
struct GatewayState {
    secret: Arc<str>,
    resources: Arc<RwLock<HashMap<String, RegisteredResource>>>,
    streams: Arc<Semaphore>,
}

/// Running loopback gateway.
#[derive(Clone)]
pub struct MediaGateway {
    address: SocketAddr,
    state: GatewayState,
    shutdown: CancellationToken,
    server: Arc<JoinHandle<()>>,
    limits: GatewayLimits,
}

impl MediaGateway {
    /// Bind an ephemeral IPv4 loopback port and start serving.
    ///
    /// # Errors
    ///
    /// Returns an error when secure randomness or loopback binding fails.
    pub async fn start(limits: GatewayLimits) -> Result<Self, GatewayError> {
        let secret = random_hex(32)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(GatewayError::Listen)?;
        let address = listener.local_addr().map_err(GatewayError::Listen)?;
        let state = GatewayState {
            secret: Arc::from(secret),
            resources: Arc::new(RwLock::new(HashMap::new())),
            streams: Arc::new(Semaphore::new(limits.max_concurrent_streams.max(1))),
        };
        let shutdown = CancellationToken::new();
        let router = router(state.clone(), limits.body_frame_timeout);
        let shutdown_signal = shutdown.clone();
        let server = tokio::spawn(async move {
            let result = axum::serve(listener, router)
                .with_graceful_shutdown(shutdown_signal.cancelled_owned())
                .await;
            if result.is_err() {
                // There is no recovery action once the app-owned listener fails.
            }
        });
        Ok(Self {
            address,
            state,
            shutdown,
            server: Arc::new(server),
            limits,
        })
    }

    /// Register an application-resolved file and return an opaque capability URL.
    ///
    /// # Errors
    ///
    /// Rejects non-files, changed lengths, random failures, and a full registry.
    pub async fn register(
        &self,
        resource: FileResource,
    ) -> Result<MediaRegistration, GatewayError> {
        let metadata = tokio::fs::metadata(&resource.path)
            .await
            .map_err(|_| GatewayError::ResourceChanged)?;
        if !metadata.is_file() || metadata.len() != resource.byte_length {
            return Err(GatewayError::ResourceChanged);
        }
        let handle = random_hex(16)?;
        let registered = RegisteredResource {
            path: resource.path,
            media_type: resource.media_type.clone(),
            byte_length: resource.byte_length,
        };
        let mut resources = self
            .state
            .resources
            .write()
            .map_err(|_| GatewayError::RegistryUnavailable)?;
        if resources.len() >= self.limits.max_registered_resources {
            return Err(GatewayError::RegistryFull);
        }
        resources.insert(handle.clone(), registered);
        Ok(MediaRegistration {
            url: format!(
                "http://{}:{}/v1/media/{handle}/{}",
                self.address.ip(),
                self.address.port(),
                self.state.secret
            ),
            media_type: resource.media_type,
            byte_length: resource.byte_length,
        })
    }

    /// Revoke every issued resource handle and request graceful server shutdown.
    pub fn shutdown(&self) {
        if let Ok(mut resources) = self.state.resources.write() {
            resources.clear();
        }
        self.shutdown.cancel();
    }

    /// Wait until the server task has finished after shutdown.
    pub async fn wait_for_shutdown(self) {
        self.shutdown();
        if let Ok(server) = Arc::try_unwrap(self.server) {
            let _ = server.await;
        }
    }
}

fn router(state: GatewayState, timeout: Duration) -> Router {
    Router::new()
        .route(
            "/v1/media/{handle}/{secret}",
            get(get_media).head(head_media),
        )
        .layer(ResponseBodyTimeoutLayer::new(timeout))
        .with_state(state)
}

async fn get_media(
    State(state): State<GatewayState>,
    AxumPath((handle, secret)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Response {
    serve_media(state, handle, secret, headers, false).await
}

async fn head_media(
    State(state): State<GatewayState>,
    AxumPath((handle, secret)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Response {
    serve_media(state, handle, secret, headers, true).await
}

async fn serve_media(
    state: GatewayState,
    handle: String,
    secret: String,
    headers: HeaderMap,
    head_only: bool,
) -> Response {
    if !constant_time_equal(secret.as_bytes(), state.secret.as_bytes()) {
        return empty_response(StatusCode::UNAUTHORIZED);
    }
    let resource = match state.resources.read() {
        Ok(resources) => resources.get(&handle).cloned(),
        Err(_) => return empty_response(StatusCode::SERVICE_UNAVAILABLE),
    };
    let Some(resource) = resource else {
        return empty_response(StatusCode::NOT_FOUND);
    };
    let Ok(range) = parse_range(headers.get(header::RANGE), resource.byte_length) else {
        return range_not_satisfiable(resource.byte_length);
    };
    let (start, end, status) = match range {
        Some((start, end)) => (start, end, StatusCode::PARTIAL_CONTENT),
        None => (0, resource.byte_length.saturating_sub(1), StatusCode::OK),
    };
    let response_length = if resource.byte_length == 0 {
        0
    } else {
        end - start + 1
    };
    let mut builder = Response::builder()
        .status(status)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_TYPE, resource.media_type.as_str())
        .header(header::CONTENT_LENGTH, response_length);
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{}", resource.byte_length),
        );
    }
    if head_only || response_length == 0 {
        return builder
            .body(Body::empty())
            .unwrap_or_else(|_| empty_response(StatusCode::INTERNAL_SERVER_ERROR));
    }
    let Ok(permit) = state.streams.clone().try_acquire_owned() else {
        return empty_response(StatusCode::SERVICE_UNAVAILABLE);
    };
    let Ok(mut file) = File::open(&resource.path).await else {
        return empty_response(StatusCode::NOT_FOUND);
    };
    if file.seek(SeekFrom::Start(start)).await.is_err() {
        return empty_response(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let stream = ReaderStream::with_capacity(file.take(response_length), READ_CHUNK_BYTES).map(
        move |result| {
            let _keep_permit_alive = &permit;
            result
        },
    );
    builder
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| empty_response(StatusCode::INTERNAL_SERVER_ERROR))
}

fn parse_range(value: Option<&HeaderValue>, length: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(value) = value else { return Ok(None) };
    let value = value.to_str().map_err(|_| ())?;
    let spec = value.strip_prefix("bytes=").ok_or(())?;
    if length == 0 || spec.contains(',') {
        return Err(());
    }
    let (start, end) = spec.split_once('-').ok_or(())?;
    if start.is_empty() {
        let suffix = end.parse::<u64>().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        return Ok(Some((length.saturating_sub(suffix), length - 1)));
    }
    let start = start.parse::<u64>().map_err(|_| ())?;
    if start >= length {
        return Err(());
    }
    let end = if end.is_empty() {
        length - 1
    } else {
        end.parse::<u64>().map_err(|_| ())?.min(length - 1)
    };
    if end < start {
        return Err(());
    }
    Ok(Some((start, end)))
}

fn range_not_satisfiable(length: u64) -> Response {
    Response::builder()
        .status(StatusCode::RANGE_NOT_SATISFIABLE)
        .header(header::CONTENT_RANGE, format!("bytes */{length}"))
        .header(header::ACCEPT_RANGES, "bytes")
        .body(Body::empty())
        .unwrap_or_else(|_| empty_response(StatusCode::INTERNAL_SERVER_ERROR))
}

fn empty_response(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .body(Body::empty())
        .unwrap_or_default()
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn random_hex(byte_count: usize) -> Result<String, GatewayError> {
    let mut bytes = vec![0_u8; byte_count];
    getrandom::fill(&mut bytes).map_err(GatewayError::Random)?;
    let mut output = String::with_capacity(byte_count * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        http::{HeaderValue, Request},
    };
    use tower::ServiceExt;

    #[test]
    fn parses_supported_single_ranges() {
        assert_eq!(parse("bytes=2-5", 10), Ok(Some((2, 5))));
        assert_eq!(parse("bytes=8-", 10), Ok(Some((8, 9))));
        assert_eq!(parse("bytes=-3", 10), Ok(Some((7, 9))));
        assert_eq!(parse("bytes=-30", 10), Ok(Some((0, 9))));
        assert_eq!(parse("bytes=7-99", 10), Ok(Some((7, 9))));
    }

    #[test]
    fn rejects_invalid_or_unsatisfiable_ranges() {
        for value in [
            "items=0-1",
            "bytes=",
            "bytes=4-2",
            "bytes=10-",
            "bytes=-0",
            "bytes=0-1,4-5",
        ] {
            assert_eq!(parse(value, 10), Err(()), "{value}");
        }
        assert_eq!(parse("bytes=0-0", 0), Err(()));
    }

    #[test]
    fn accepts_requests_without_a_range() {
        assert_eq!(parse_range(None, 0), Ok(None));
    }

    fn parse(value: &str, length: u64) -> Result<Option<(u64, u64)>, ()> {
        parse_range(
            Some(&HeaderValue::from_str(value).expect("valid header")),
            length,
        )
    }

    #[test]
    fn compares_capabilities_without_early_byte_exit() {
        assert!(constant_time_equal(b"secret", b"secret"));
        assert!(!constant_time_equal(b"secret", b"secreu"));
        assert!(!constant_time_equal(b"short", b"longer"));
    }

    #[test]
    fn binds_only_ipv4_loopback() {
        let address = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 42);
        assert!(address.ip().is_loopback());
    }

    #[tokio::test]
    async fn serves_authenticated_get_head_and_ranges_without_exposing_paths() {
        let test_root =
            std::env::temp_dir().join(format!("zeron-media-test-{}", random_hex(8).unwrap()));
        std::fs::create_dir_all(&test_root).unwrap();
        let media_path = test_root.join("lesson.bin");
        std::fs::write(&media_path, b"0123456789").unwrap();
        let state = GatewayState {
            secret: Arc::from("test-secret"),
            resources: Arc::new(RwLock::new(HashMap::from([(
                "opaque".to_owned(),
                RegisteredResource {
                    path: media_path,
                    media_type: "video/mp4".to_owned(),
                    byte_length: 10,
                },
            )]))),
            streams: Arc::new(Semaphore::new(1)),
        };
        let app = router(state, Duration::from_secs(1));

        let unauthorized = app
            .clone()
            .oneshot(
                Request::get("/v1/media/opaque/wrong")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let ranged = app
            .clone()
            .oneshot(
                Request::get("/v1/media/opaque/test-secret")
                    .header(header::RANGE, "bytes=2-5")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ranged.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(ranged.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
        assert_eq!(to_bytes(ranged.into_body(), 10).await.unwrap(), "2345");

        let head = app
            .clone()
            .oneshot(
                Request::head("/v1/media/opaque/test-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(head.status(), StatusCode::OK);
        assert_eq!(head.headers()[header::CONTENT_LENGTH], "10");
        assert!(to_bytes(head.into_body(), 1).await.unwrap().is_empty());

        let invalid = app
            .oneshot(
                Request::get("/v1/media/opaque/test-secret")
                    .header(header::RANGE, "bytes=40-")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(invalid.headers()[header::CONTENT_RANGE], "bytes */10");
        std::fs::remove_dir_all(test_root).unwrap();
    }
}
