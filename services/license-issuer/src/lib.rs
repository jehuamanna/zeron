//! The reference license issuer for paid Zeron courses (ADR 0017
//! decisions 5, 6, and 8).
//!
//! A small HTTP service a publisher can run. It serves plain HTTP for a
//! reverse proxy that terminates TLS in front of it. It never handles
//! payment details or learner names: entitlements are keyed by learner key.
//!
//! - `GET /v1/issuer` names its signing and encryption keys.
//! - `PUT /v1/delegations` accepts a publisher's signed delegation.
//! - `POST /v1/releases` registers a paid release, its content key sealed to
//!   the issuer, signed by the publisher's current key. The first
//!   registration of a course makes that publisher its owner here.
//! - `POST /v1/licenses` answers a learner's signed request with a license
//!   for every registered version of a course the learner is entitled to,
//!   each content key re-sealed to the learner.

pub mod keys;
pub mod storage;

use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use course_access::{unwrap_content_key, wrap_content_key};
use course_signing::{
    DelegationStatement, DelegationSubmission, LicenseFile, LicenseRequest, LicensedKey, PublicKey,
    RegistrationSubmission, RotationStatement, Signed, license_file, sign_license,
    verify_delegation_submission, verify_license_request, verify_release_registration,
};
use serde::Serialize;
use thiserror::Error;
use tower_http::timeout::TimeoutLayer;

pub use keys::IssuerKeys;
pub use storage::{Registration, Storage};

/// Largest request body read.
pub const MAX_BODY_BYTES: usize = 64 * 1024;
/// How far a license request's time may be from the issuer's clock.
pub const REQUEST_WINDOW_SECONDS: u64 = 300;
/// Longest a request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Why the issuer could not do something. Messages never contain secrets.
#[derive(Debug, Error)]
pub enum IssuerError {
    /// The database failed.
    #[error("the issuer database failed: {0}")]
    Storage(String),
    /// The key file is missing, malformed, or cannot be written.
    #[error("{0}")]
    KeyFile(String),
    /// Another publisher registered this course here first.
    #[error("another publisher owns this course on this issuer")]
    CourseOwned,
    /// This version was registered with another content key.
    #[error("this course version is already registered with another content key")]
    VersionRegistered,
}

/// A refusal sent to the client as `{ "code", "message" }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// HTTP status.
    pub status: StatusCode,
    /// Stable machine-readable code.
    pub code: &'static str,
    /// Plain explanation.
    pub message: String,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct Body<'a> {
            code: &'a str,
            message: &'a str,
        }
        (
            self.status,
            Json(Body {
                code: self.code,
                message: &self.message,
            }),
        )
            .into_response()
    }
}

impl From<IssuerError> for ApiError {
    fn from(error: IssuerError) -> Self {
        match error {
            IssuerError::CourseOwned => {
                Self::new(StatusCode::FORBIDDEN, "course_owned", error.to_string())
            }
            IssuerError::VersionRegistered => Self::new(
                StatusCode::CONFLICT,
                "version_registered",
                error.to_string(),
            ),
            // Database and key-file details stay in the operator's logs.
            IssuerError::Storage(_) | IssuerError::KeyFile(_) => internal(error),
        }
    }
}

/// The issuer's public keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuerInfo {
    /// Publishers delegate to this key; it signs licenses.
    pub issuer_key: String,
    /// Publishers seal release content keys to this key (`x25519-…`).
    pub encryption_key: String,
}

/// A running issuer: its keys, its database, and its clock.
pub struct Issuer {
    keys: IssuerKeys,
    storage: Mutex<Storage>,
    clock: Box<dyn Fn() -> u64 + Send + Sync>,
}

impl Issuer {
    /// An issuer with the system clock.
    #[must_use]
    pub fn new(keys: IssuerKeys, storage: Storage) -> Self {
        Self::with_clock(keys, storage, Box::new(unix_now))
    }

    /// An issuer with a given clock, for tests.
    #[must_use]
    pub fn with_clock(
        keys: IssuerKeys,
        storage: Storage,
        clock: Box<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        Self {
            keys,
            storage: Mutex::new(storage),
            clock,
        }
    }

    fn storage(&self) -> std::sync::MutexGuard<'_, Storage> {
        self.storage.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The issuer's public keys.
    #[must_use]
    pub fn info(&self) -> IssuerInfo {
        IssuerInfo {
            issuer_key: self.keys.signing_public().to_string(),
            encryption_key: format!("x25519-{}", keys::hex(self.keys.encryption_public())),
        }
    }

    /// Accept a publisher's delegation to this issuer, replacing an earlier
    /// one.
    ///
    /// # Errors
    /// Returns `delegation_invalid` (422).
    pub fn accept_delegation(&self, submission: &DelegationSubmission) -> Result<(), ApiError> {
        let verified = verify_delegation_submission(submission, &self.keys.signing_public(), &[])
            .map_err(|error| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "delegation_invalid",
                error.to_string(),
            )
        })?;
        let delegation = serde_json::to_string(&submission.delegation).map_err(internal)?;
        let rotations = serde_json::to_string(&submission.rotations).map_err(internal)?;
        self.storage().set_publisher(
            &verified.root.to_string(),
            &delegation,
            &rotations,
            (self.clock)(),
        )?;
        Ok(())
    }

    /// Register a paid release.
    ///
    /// # Errors
    /// Returns `registration_invalid` (422), `delegation_missing` (403),
    /// `content_key_invalid` (422), `course_owned` (403), or
    /// `version_registered` (409).
    pub fn register(&self, submission: &RegistrationSubmission) -> Result<Registration, ApiError> {
        let verified = verify_release_registration(submission, &self.keys.signing_public(), &[])
            .map_err(|error| {
                ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "registration_invalid",
                    error.to_string(),
                )
            })?;
        let publisher_id = verified.root.to_string();
        let statement = &submission.registration.statement;
        // The sealed key must open here, or no learner could ever get it.
        unwrap_content_key(
            &statement.sealed_content_key,
            self.keys.encryption_secret(),
            &statement.course_id,
            &statement.course_version,
        )
        .map_err(|_| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "content_key_invalid",
                "the content key is not sealed to this issuer for this course and version",
            )
        })?;
        let mut storage = self.storage();
        let Some((_, stored_rotations)) = storage.publisher(&publisher_id)? else {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "delegation_missing",
                "send your delegation to this issuer before registering releases",
            ));
        };
        let stored: Vec<Signed<RotationStatement>> =
            serde_json::from_str(&stored_rotations).map_err(internal)?;
        if submission.rotations.len() > stored.len() {
            let rotations = serde_json::to_string(&submission.rotations).map_err(internal)?;
            storage.set_rotations(&publisher_id, &rotations)?;
        }
        Ok(storage.register_release(
            &publisher_id,
            &statement.course_id,
            &statement.course_version,
            &statement.sealed_content_key,
            (self.clock)(),
        )?)
    }

    /// Entitle a learner key to a course; `source` says why, such as
    /// `manual` or a payment reference.
    ///
    /// # Errors
    /// Returns [`IssuerError::Storage`].
    pub fn grant(
        &self,
        course_id: &str,
        learner_key: &PublicKey,
        source: &str,
    ) -> Result<bool, IssuerError> {
        self.storage()
            .grant(course_id, &learner_key.to_string(), source, (self.clock)())
    }

    /// Answer a learner's license request.
    ///
    /// # Errors
    /// Returns `request_invalid` (401) for a bad signature or another issuer,
    /// `request_expired` (401) for a time outside the window, `course_unknown`
    /// (404), `not_entitled` (403), or `delegation_not_valid_yet` (503).
    pub fn issue(&self, request: &Signed<LicenseRequest>) -> Result<LicenseFile, ApiError> {
        let verified =
            verify_license_request(request, &self.keys.signing_public()).map_err(|error| {
                ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    "request_invalid",
                    error.to_string(),
                )
            })?;
        let now = (self.clock)();
        if now.abs_diff(verified.requested_at) > REQUEST_WINDOW_SECONDS {
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "request_expired",
                "the request's time is too far from the issuer's clock; check this computer's clock",
            ));
        }
        let mut storage = self.storage();
        let unknown = || {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "course_unknown",
                "this issuer has no such course",
            )
        };
        let publisher_id = storage
            .course_publisher(&verified.course_id)?
            .ok_or_else(unknown)?;
        let learner_key = verified.learner_key.to_string();
        if !storage.entitled(&verified.course_id, &learner_key)? {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "not_entitled",
                "this learner key has no purchase of this course here",
            ));
        }
        let (delegation, rotations) = storage.publisher(&publisher_id)?.ok_or_else(unknown)?;
        let delegation: Signed<DelegationStatement> =
            serde_json::from_str(&delegation).map_err(internal)?;
        let rotations: Vec<Signed<RotationStatement>> =
            serde_json::from_str(&rotations).map_err(internal)?;
        if now < delegation.statement.valid_from {
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "delegation_not_valid_yet",
                "the publisher's delegation to this issuer starts later",
            ));
        }
        let mut keys = Vec::new();
        for release in storage.releases(&verified.course_id)? {
            let content_key = unwrap_content_key(
                &release.sealed_content_key,
                self.keys.encryption_secret(),
                &verified.course_id,
                &release.course_version,
            )
            .map_err(|_| internal("a registered content key no longer opens"))?;
            let wrapped = wrap_content_key(
                &content_key,
                &verified.encryption_key,
                &verified.course_id,
                &release.course_version,
            )
            .map_err(|_| {
                ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    "request_invalid",
                    "the encryption key is not usable",
                )
            })?;
            keys.push(LicensedKey {
                course_version: release.course_version,
                wrapped_content_key: wrapped,
            });
        }
        if keys.is_empty() {
            return Err(unknown());
        }
        storage.record_issued(&verified.course_id, &learner_key, keys.len(), now)?;
        let root: PublicKey = publisher_id.parse().map_err(internal)?;
        Ok(license_file(
            &root,
            rotations,
            Some(delegation),
            sign_license(
                self.keys.signing(),
                &verified.course_id,
                &verified.learner_key,
                keys,
                now,
            ),
        ))
    }
}

/// A failure the client cannot fix. The detail goes to the operator's log,
/// never to the client; it never contains key material.
fn internal(error: impl std::fmt::Display) -> ApiError {
    eprintln!("license-issuer: {error}");
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "issuer_failed",
        "the issuer could not complete the request",
    )
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// The issuer's HTTP routes, with a body limit and a request timeout.
pub fn router(issuer: Arc<Issuer>) -> Router {
    Router::new()
        .route("/v1/issuer", get(info))
        .route("/v1/delegations", put(delegation))
        .route("/v1/releases", post(release))
        .route("/v1/licenses", post(license))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .with_state(issuer)
}

fn parse<T: serde::de::DeserializeOwned>(body: &Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "request_malformed",
            "the request is not valid JSON of the expected shape",
        )
    })
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(work).await.map_err(internal)?
}

async fn info(State(issuer): State<Arc<Issuer>>) -> Json<IssuerInfo> {
    Json(issuer.info())
}

async fn delegation(
    State(issuer): State<Arc<Issuer>>,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let submission: DelegationSubmission = parse(&body)?;
    blocking(move || issuer.accept_delegation(&submission)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn release(State(issuer): State<Arc<Issuer>>, body: Bytes) -> Result<StatusCode, ApiError> {
    let submission: RegistrationSubmission = parse(&body)?;
    Ok(
        match blocking(move || issuer.register(&submission)).await? {
            Registration::Added => StatusCode::CREATED,
            Registration::Unchanged => StatusCode::OK,
        },
    )
}

async fn license(State(issuer): State<Arc<Issuer>>, body: Bytes) -> Result<Response, ApiError> {
    let request: Signed<LicenseRequest> = parse(&body)?;
    let file = blocking(move || issuer.issue(&request)).await?;
    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        file.to_bytes(),
    )
        .into_response())
}
