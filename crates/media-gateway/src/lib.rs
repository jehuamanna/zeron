//! Authenticated loopback delivery for explicitly registered media resources.

use std::{
    collections::{HashMap, VecDeque},
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicU64, Ordering},
    },
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
use futures_util::{StreamExt, future::BoxFuture};
use thiserror::Error;
use tokio::{
    fs::File,
    io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, SeekFrom},
    net::TcpListener,
    sync::Semaphore,
    task::JoinHandle,
};
use tokio_util::{io::ReaderStream, sync::CancellationToken};
use tower_http::timeout::ResponseBodyTimeoutLayer;

const READ_CHUNK_BYTES: usize = 64 * 1024;
/// Response bodies kept per resource. A newer request (for example after a
/// seek) cancels the oldest, so abandoned waits do not pile up.
const MAX_BODIES_PER_RESOURCE: usize = 2;

/// Seekable reader over a registered resource. Reads may wait, for example
/// until torrent pieces are verified.
pub trait MediaReader: AsyncRead + AsyncSeek + Send + Unpin {}

impl<T: AsyncRead + AsyncSeek + Send + Unpin> MediaReader for T {}

/// Opens a fresh reader for every response. The reader is dropped when the
/// response ends, is cancelled, or the client disconnects.
pub trait MediaSource: Send + Sync + 'static {
    /// Open a reader positioned at the start of the resource.
    fn open(&self) -> BoxFuture<'static, std::io::Result<Box<dyn MediaReader>>>;
}

/// A resource served from a [`MediaSource`] instead of a local file.
#[derive(Clone)]
pub struct StreamResource {
    /// Application-provided reader factory. It must never return unverified bytes.
    pub source: Arc<dyn MediaSource>,
    /// Manifest-declared media type.
    pub media_type: String,
    /// Exact resource length.
    pub byte_length: u64,
}

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

#[derive(Clone)]
enum Origin {
    File(PathBuf),
    Source(Arc<dyn MediaSource>),
}

#[derive(Clone)]
struct RegisteredResource {
    origin: Origin,
    media_type: String,
    byte_length: u64,
    bodies: Arc<Bodies>,
}

/// In-flight response bodies of one resource, oldest first.
#[derive(Default)]
struct Bodies {
    next: AtomicU64,
    active: Mutex<VecDeque<(u64, CancellationToken)>>,
}

impl Bodies {
    fn start(self: &Arc<Self>) -> BodyGuard {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let token = CancellationToken::new();
        if let Ok(mut active) = self.active.lock() {
            active.push_back((id, token.clone()));
            while active.len() > MAX_BODIES_PER_RESOURCE {
                if let Some((_, oldest)) = active.pop_front() {
                    oldest.cancel();
                }
            }
        }
        BodyGuard {
            bodies: Arc::clone(self),
            id,
            token,
        }
    }

    fn cancel_all(&self) {
        if let Ok(mut active) = self.active.lock() {
            for (_, token) in active.drain(..) {
                token.cancel();
            }
        }
    }
}

struct BodyGuard {
    bodies: Arc<Bodies>,
    id: u64,
    token: CancellationToken,
}

impl Drop for BodyGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = self.bodies.active.lock() {
            active.retain(|(id, _)| *id != self.id);
        }
    }
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
        self.insert(
            Origin::File(resource.path),
            resource.media_type,
            resource.byte_length,
        )
    }

    /// Register a reader-backed resource, such as a file streamed from a
    /// torrent, and return an opaque capability URL.
    ///
    /// # Errors
    ///
    /// Rejects random failures and a full registry.
    pub fn register_stream(
        &self,
        resource: StreamResource,
    ) -> Result<MediaRegistration, GatewayError> {
        self.insert(
            Origin::Source(resource.source),
            resource.media_type,
            resource.byte_length,
        )
    }

    /// Revoke one capability URL and cancel its in-flight responses.
    pub fn revoke(&self, url: &str) {
        let Some(handle) = url
            .split("/v1/media/")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
        else {
            return;
        };
        if let Ok(mut resources) = self.state.resources.write()
            && let Some(resource) = resources.remove(handle)
        {
            resource.bodies.cancel_all();
        }
    }

    fn insert(
        &self,
        origin: Origin,
        media_type: String,
        byte_length: u64,
    ) -> Result<MediaRegistration, GatewayError> {
        let handle = random_hex(16)?;
        let registered = RegisteredResource {
            origin,
            media_type: media_type.clone(),
            byte_length,
            bodies: Arc::new(Bodies::default()),
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
            media_type,
            byte_length,
        })
    }

    /// Revoke every issued resource handle and request graceful server shutdown.
    pub fn shutdown(&self) {
        if let Ok(mut resources) = self.state.resources.write() {
            for resource in resources.values() {
                resource.bodies.cancel_all();
            }
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
    let mut reader: Box<dyn MediaReader> = match &resource.origin {
        Origin::File(path) => match File::open(path).await {
            Ok(file) => Box::new(file),
            Err(_) => return empty_response(StatusCode::NOT_FOUND),
        },
        Origin::Source(source) => match source.open().await {
            Ok(reader) => reader,
            Err(_) => return empty_response(StatusCode::SERVICE_UNAVAILABLE),
        },
    };
    if reader.seek(SeekFrom::Start(start)).await.is_err() {
        return empty_response(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let guard = resource.bodies.start();
    let cancelled = guard.token.clone().cancelled_owned();
    let stream = ReaderStream::with_capacity(reader.take(response_length), READ_CHUNK_BYTES)
        .take_until(cancelled)
        .map(move |result| {
            let _keep_alive = (&permit, &guard);
            result
        });
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
                    origin: Origin::File(media_path),
                    media_type: "video/mp4".to_owned(),
                    byte_length: 10,
                    bodies: Arc::new(Bodies::default()),
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

    /// Serves bytes only after its gate opens, like a torrent file whose pieces
    /// are not yet verified.
    struct GatedSource {
        data: Vec<u8>,
        gate: Arc<Semaphore>,
        fail: bool,
    }

    struct GatedReader {
        data: std::io::Cursor<Vec<u8>>,
        gate: Arc<Semaphore>,
        wait: Option<BoxFuture<'static, ()>>,
        open: bool,
    }

    impl MediaSource for GatedSource {
        fn open(&self) -> BoxFuture<'static, std::io::Result<Box<dyn MediaReader>>> {
            let reader = GatedReader {
                data: std::io::Cursor::new(self.data.clone()),
                gate: Arc::clone(&self.gate),
                wait: None,
                open: false,
            };
            let fail = self.fail;
            Box::pin(async move {
                if fail {
                    return Err(std::io::Error::other("not available"));
                }
                Ok(Box::new(reader) as Box<dyn MediaReader>)
            })
        }
    }

    impl AsyncRead for GatedReader {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if !self.open {
                let gate = Arc::clone(&self.gate);
                let wait = self.wait.get_or_insert_with(|| {
                    Box::pin(async move {
                        let _ = gate.acquire().await;
                    })
                });
                std::task::ready!(wait.as_mut().poll(cx));
                self.open = true;
            }
            std::pin::Pin::new(&mut self.data).poll_read(cx, buf)
        }
    }

    impl AsyncSeek for GatedReader {
        fn start_seek(
            mut self: std::pin::Pin<&mut Self>,
            position: SeekFrom,
        ) -> std::io::Result<()> {
            std::pin::Pin::new(&mut self.data).start_seek(position)
        }

        fn poll_complete(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<u64>> {
            std::pin::Pin::new(&mut self.data).poll_complete(cx)
        }
    }

    async fn gated_gateway(fail: bool) -> (MediaGateway, Arc<Semaphore>, String) {
        let gateway = MediaGateway::start(GatewayLimits::default()).await.unwrap();
        let gate = Arc::new(Semaphore::new(0));
        let registration = gateway
            .register_stream(StreamResource {
                source: Arc::new(GatedSource {
                    data: b"0123456789".to_vec(),
                    gate: Arc::clone(&gate),
                    fail,
                }),
                media_type: "video/mp4".to_owned(),
                byte_length: 10,
            })
            .unwrap();
        let path = registration.url.split_once("127.0.0.1").unwrap().1;
        let path = path.split_once('/').unwrap().1;
        (gateway, gate, format!("/{path}"))
    }

    async fn ranged(gateway: &MediaGateway, path: &str, range: &str) -> Response {
        router(gateway.state.clone(), Duration::from_secs(5))
            .oneshot(
                Request::get(path)
                    .header(header::RANGE, range)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn stream_resources_wait_for_bytes_then_serve_exact_ranges() {
        let (gateway, gate, path) = gated_gateway(false).await;
        let response = ranged(&gateway, &path, "bytes=3-6").await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 3-6/10");
        let body = tokio::spawn(to_bytes(response.into_body(), 10));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !body.is_finished(),
            "nothing is served before the bytes are ready"
        );
        gate.add_permits(Semaphore::MAX_PERMITS);
        assert_eq!(body.await.unwrap().unwrap(), "3456");
        gateway.wait_for_shutdown().await;
    }

    #[tokio::test]
    async fn newer_requests_cancel_the_oldest_waiting_body_and_revoke_cancels_all() {
        let (gateway, gate, path) = gated_gateway(false).await;
        let mut bodies = Vec::new();
        for _ in 0..3 {
            let response = ranged(&gateway, &path, "bytes=0-9").await;
            bodies.push(tokio::spawn(to_bytes(response.into_body(), 10)));
        }
        let oldest = bodies.remove(0);
        let cancelled = tokio::time::timeout(Duration::from_secs(2), oldest)
            .await
            .expect("the oldest body is cancelled")
            .unwrap()
            .unwrap();
        assert!(cancelled.is_empty());
        gate.add_permits(Semaphore::MAX_PERMITS);
        for body in bodies {
            assert_eq!(body.await.unwrap().unwrap(), "0123456789");
        }

        let (gateway, _closed, path) = gated_gateway(false).await;
        let response = ranged(&gateway, &path, "bytes=0-9").await;
        let pending = tokio::spawn(to_bytes(response.into_body(), 10));
        gateway.revoke(&format!("http://127.0.0.1{path}"));
        let revoked = tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .expect("revoking cancels in-flight bodies")
            .unwrap()
            .unwrap();
        assert!(revoked.is_empty());
        assert_eq!(
            ranged(&gateway, &path, "bytes=0-9").await.status(),
            StatusCode::NOT_FOUND
        );
        gateway.wait_for_shutdown().await;
    }

    #[tokio::test]
    async fn unavailable_sources_are_reported_without_a_body() {
        let (gateway, _gate, path) = gated_gateway(true).await;
        let response = ranged(&gateway, &path, "bytes=0-1").await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        gateway.wait_for_shutdown().await;
    }
}
