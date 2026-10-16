//! The issuer over real HTTP on a loopback port: delegation, release
//! registration, entitlements, and license requests, with every refusal.
//! No public network.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use course_access::{ContentKey, unwrap_content_key, wrap_content_key};
use course_signing::{
    DelegationSubmission, PublicKey, SecretKey, parse_license, rotate, sign_delegation,
    sign_license_request, sign_release_registration, verify_license,
};
use license_issuer::{Issuer, IssuerKeys, MAX_BODY_BYTES, Storage, router};

const NOW: u64 = 1_000_000;

struct Server {
    base: String,
    clock: Arc<AtomicU64>,
    issuer_key: PublicKey,
    encryption_key: [u8; 32],
    issuer: Arc<Issuer>,
    http: reqwest::Client,
}

impl Server {
    async fn start() -> Self {
        let keys = IssuerKeys::from_seed(&[11; 32]);
        let issuer_key = keys.signing_public();
        let encryption_key = *keys.encryption_public();
        let clock = Arc::new(AtomicU64::new(NOW));
        let time = Arc::clone(&clock);
        let issuer = Arc::new(Issuer::with_clock(
            keys,
            Storage::open_in_memory().unwrap(),
            Box::new(move || time.load(Ordering::SeqCst)),
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = router(Arc::clone(&issuer));
        tokio::spawn(async move { axum::serve(listener, app).await });
        Self {
            base,
            clock,
            issuer_key,
            encryption_key,
            issuer,
            http: reqwest::Client::new(),
        }
    }

    async fn send(&self, method: reqwest::Method, path: &str, body: Vec<u8>) -> (u16, String) {
        let response = self
            .http
            .request(method, format!("{}{path}", self.base))
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .unwrap();
        (response.status().as_u16(), response.text().await.unwrap())
    }

    async fn put(&self, path: &str, body: &impl serde::Serialize) -> (u16, String) {
        self.send(
            reqwest::Method::PUT,
            path,
            serde_json::to_vec(body).unwrap(),
        )
        .await
    }

    async fn post(&self, path: &str, body: &impl serde::Serialize) -> (u16, String) {
        self.send(
            reqwest::Method::POST,
            path,
            serde_json::to_vec(body).unwrap(),
        )
        .await
    }
}

fn code(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body).unwrap()["code"]
        .as_str()
        .unwrap()
        .to_owned()
}

struct Publisher {
    root: SecretKey,
    current: SecretKey,
    rotations: Vec<course_signing::Signed<course_signing::RotationStatement>>,
}

impl Publisher {
    fn new(seed: u8) -> Self {
        let root = SecretKey::from_bytes(&[seed; 32]);
        let current = SecretKey::from_bytes(&[seed + 1; 32]);
        let rotations = vec![rotate(&root, &current.public_key(), 10)];
        Self {
            root,
            current,
            rotations,
        }
    }

    fn delegation(&self, server: &Server, valid_from: u64) -> DelegationSubmission {
        DelegationSubmission {
            publisher_id: self.root.public_key().to_string(),
            rotations: self.rotations.clone(),
            delegation: sign_delegation(
                &self.current,
                &server.issuer_key,
                "https://licenses.example",
                valid_from,
            ),
        }
    }

    fn registration(
        &self,
        server: &Server,
        course: &str,
        version: &str,
        key: &ContentKey,
    ) -> course_signing::RegistrationSubmission {
        let sealed = wrap_content_key(key, &server.encryption_key, course, version).unwrap();
        sign_release_registration(
            &self.current,
            &self.root.public_key(),
            self.rotations.clone(),
            (course, version, &sealed),
            &server.issuer_key,
            NOW,
        )
    }
}

struct Learner {
    signing: SecretKey,
    secret: [u8; 32],
    public: [u8; 32],
}

impl Learner {
    fn new(seed: u8) -> Self {
        let secret = x25519_dalek::StaticSecret::from([seed; 32]);
        Self {
            signing: SecretKey::from_bytes(&[seed; 32]),
            public: *x25519_dalek::PublicKey::from(&secret).as_bytes(),
            secret: secret.to_bytes(),
        }
    }

    fn request(
        &self,
        server: &Server,
        course: &str,
        at: u64,
    ) -> course_signing::Signed<course_signing::LicenseRequest> {
        sign_license_request(&self.signing, course, &self.public, &server.issuer_key, at)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_learner_gets_a_license_for_every_registered_version_it_is_entitled_to() {
    let server = Server::start().await;
    let (status, info) = server
        .send(reqwest::Method::GET, "/v1/issuer", Vec::new())
        .await;
    assert_eq!(status, 200);
    assert!(info.contains(&server.issuer_key.to_string()));

    let publisher = Publisher::new(1);
    let learner = Learner::new(7);
    let first = ContentKey::from_bytes([31; 32]);
    let second = ContentKey::from_bytes([32; 32]);
    assert_eq!(
        server
            .put("/v1/delegations", &publisher.delegation(&server, NOW - 60))
            .await
            .0,
        204
    );
    let v1 = publisher.registration(&server, "course_a", "1.0.0", &first);
    assert_eq!(server.post("/v1/releases", &v1).await.0, 201);
    assert_eq!(
        server.post("/v1/releases", &v1).await.0,
        200,
        "registering again is harmless"
    );
    server
        .issuer
        .grant("course_a", &learner.signing.public_key(), "manual")
        .unwrap();

    let (status, body) = server
        .post("/v1/licenses", &learner.request(&server, "course_a", NOW))
        .await;
    assert_eq!(status, 200, "{body}");
    // The Library's own check: the license verifies against the pinned
    // publisher, through the delegation, for this learner only.
    let file = parse_license(body.as_bytes()).unwrap();
    let root = publisher.root.public_key().to_string();
    let verified = verify_license(&file, &root, &learner.signing.public_key(), &[]).unwrap();
    assert!(verified.delegated);
    assert_eq!(verified.issuer_key, server.issuer_key);
    assert_eq!(verified.keys.len(), 1);
    let opened = unwrap_content_key(
        &verified.keys[0].wrapped_content_key,
        &learner.secret,
        "course_a",
        "1.0.0",
    )
    .unwrap();
    assert_eq!(opened, first);
    assert!(
        !body.contains(&format!("{:02x}{:02x}", 31, 31)),
        "no content key in the clear"
    );

    // A later version: asking again returns both.
    let v2 = publisher.registration(&server, "course_a", "1.1.0", &second);
    assert_eq!(server.post("/v1/releases", &v2).await.0, 201);
    let (status, body) = server
        .post(
            "/v1/licenses",
            &learner.request(&server, "course_a", NOW + 5),
        )
        .await;
    assert_eq!(status, 200);
    let file = parse_license(body.as_bytes()).unwrap();
    let verified = verify_license(&file, &root, &learner.signing.public_key(), &[]).unwrap();
    let versions: Vec<_> = verified
        .keys
        .iter()
        .map(|key| key.course_version.as_str())
        .collect();
    assert_eq!(versions, ["1.0.0", "1.1.0"]);
    let later = unwrap_content_key(
        &verified.keys[1].wrapped_content_key,
        &learner.secret,
        "course_a",
        "1.1.0",
    )
    .unwrap();
    assert_eq!(later, second);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publishers_are_checked_and_courses_and_versions_stay_theirs() {
    let server = Server::start().await;
    let publisher = Publisher::new(1);
    let rival = Publisher::new(3);
    let key = ContentKey::from_bytes([31; 32]);

    let registration = publisher.registration(&server, "course_a", "1.0.0", &key);
    let (status, body) = server.post("/v1/releases", &registration).await;
    assert_eq!((status, code(&body).as_str()), (403, "delegation_missing"));

    let mut elsewhere = publisher.delegation(&server, 0);
    elsewhere.delegation = sign_delegation(
        &publisher.current,
        &SecretKey::from_bytes(&[99; 32]).public_key(),
        "https://other.example",
        0,
    );
    let (status, body) = server.put("/v1/delegations", &elsewhere).await;
    assert_eq!((status, code(&body).as_str()), (422, "delegation_invalid"));

    assert_eq!(
        server
            .put("/v1/delegations", &publisher.delegation(&server, 0))
            .await
            .0,
        204
    );
    assert_eq!(server.post("/v1/releases", &registration).await.0, 201);

    let changed = publisher.registration(
        &server,
        "course_a",
        "1.0.0",
        &ContentKey::from_bytes([40; 32]),
    );
    let (status, body) = server.post("/v1/releases", &changed).await;
    assert_eq!((status, code(&body).as_str()), (409, "version_registered"));

    assert_eq!(
        server
            .put("/v1/delegations", &rival.delegation(&server, 0))
            .await
            .0,
        204
    );
    let taken = rival.registration(&server, "course_a", "2.0.0", &key);
    let (status, body) = server.post("/v1/releases", &taken).await;
    assert_eq!((status, code(&body).as_str()), (403, "course_owned"));

    // A content key sealed for another course does not open here.
    let sealed = wrap_content_key(&key, &server.encryption_key, "course_c", "1.0.0").unwrap();
    let misdirected = sign_release_registration(
        &publisher.current,
        &publisher.root.public_key(),
        publisher.rotations.clone(),
        ("course_b", "1.0.0", &sealed),
        &server.issuer_key,
        NOW,
    );
    let (status, body) = server.post("/v1/releases", &misdirected).await;
    assert_eq!((status, code(&body).as_str()), (422, "content_key_invalid"));

    let mut forged = publisher.registration(&server, "course_a", "3.0.0", &key);
    forged.registration.statement.course_version = "3.0.1".to_owned();
    let (status, body) = server.post("/v1/releases", &forged).await;
    assert_eq!(
        (status, code(&body).as_str()),
        (422, "registration_invalid")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn license_requests_need_an_entitlement_a_valid_signature_and_a_current_time() {
    let server = Server::start().await;
    let publisher = Publisher::new(1);
    let learner = Learner::new(7);
    let stranger = Learner::new(8);
    let key = ContentKey::from_bytes([31; 32]);

    let (status, body) = server
        .post("/v1/licenses", &learner.request(&server, "course_a", NOW))
        .await;
    assert_eq!((status, code(&body).as_str()), (404, "course_unknown"));

    assert_eq!(
        server
            .put(
                "/v1/delegations",
                &publisher.delegation(&server, NOW + 3_600)
            )
            .await
            .0,
        204
    );
    assert_eq!(
        server
            .post(
                "/v1/releases",
                &publisher.registration(&server, "course_a", "1.0.0", &key)
            )
            .await
            .0,
        201
    );
    server
        .issuer
        .grant("course_a", &learner.signing.public_key(), "manual")
        .unwrap();

    let (status, body) = server
        .post("/v1/licenses", &stranger.request(&server, "course_a", NOW))
        .await;
    assert_eq!((status, code(&body).as_str()), (403, "not_entitled"));

    let (status, body) = server
        .post("/v1/licenses", &learner.request(&server, "course_a", NOW))
        .await;
    assert_eq!(
        (status, code(&body).as_str()),
        (503, "delegation_not_valid_yet")
    );
    server.clock.store(NOW + 3_600, Ordering::SeqCst);
    let stale = learner.request(&server, "course_a", NOW);
    let (status, body) = server.post("/v1/licenses", &stale).await;
    assert_eq!((status, code(&body).as_str()), (401, "request_expired"));

    let mut stolen = learner.request(&server, "course_a", NOW + 3_600);
    stolen.statement.encryption_key = format!("x25519-{}", "11".repeat(32));
    let (status, body) = server.post("/v1/licenses", &stolen).await;
    assert_eq!((status, code(&body).as_str()), (401, "request_invalid"));

    let elsewhere = sign_license_request(
        &learner.signing,
        "course_a",
        &learner.public,
        &SecretKey::from_bytes(&[99; 32]).public_key(),
        NOW + 3_600,
    );
    let (status, body) = server.post("/v1/licenses", &elsewhere).await;
    assert_eq!((status, code(&body).as_str()), (401, "request_invalid"));

    let (status, body) = server
        .post(
            "/v1/licenses",
            &learner.request(&server, "course_a", NOW + 3_600),
        )
        .await;
    assert_eq!(status, 200, "{body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_and_oversized_bodies_are_refused() {
    let server = Server::start().await;
    let (status, body) = server
        .send(reqwest::Method::POST, "/v1/licenses", b"not json".to_vec())
        .await;
    assert_eq!((status, code(&body).as_str()), (400, "request_malformed"));
    let (status, _) = server
        .send(
            reqwest::Method::POST,
            "/v1/releases",
            vec![b' '; MAX_BODY_BYTES + 1],
        )
        .await;
    assert_eq!(status, 413);
    let (status, _) = server
        .send(reqwest::Method::GET, "/v1/licenses", Vec::new())
        .await;
    assert_eq!(status, 405);
}
