//! Spike: issuer-as-relying-party sign-in with a loopback redirect (ADR 0017 decision 7).
//!
//! One process runs three roles on separate loopback ports:
//! - a mock GitHub-style provider (`/authorize` auto-consents, `/token`, `/user`);
//! - a mock issuer (`/auth/start`, `/auth/callback`, `/auth/redeem`);
//! - the desktop app's side: PKCE + state, a one-shot loopback listener, redeem
//!   signed with the learner key.
//!
//! `cargo run --bin oauth -- headless`  a redirect-following HTTP client plays the browser
//! `cargo run --bin oauth -- print`     prints the start URL for a real browser to open
//! `cargo run --bin oauth -- system`    opens the system browser (xdg-open / open / start)
//! `cargo run --bin oauth -- checks`    negative cases against the issuer

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, bail, ensure};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{net::TcpListener, sync::oneshot};

type Params = Query<HashMap<String, String>>;

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).expect("os randomness");
    B64.encode(bytes)
}

fn challenge(verifier: &str) -> String {
    B64.encode(Sha256::digest(verifier.as_bytes()))
}

fn redeem_message(code: &str, learner_key: &str) -> Vec<u8> {
    format!("zeron.redeem.v1\n{code}\n{learner_key}").into_bytes()
}

async fn bind() -> anyhow::Result<(TcpListener, SocketAddr)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    Ok((listener, addr))
}

// ---------------------------------------------------------------- provider

#[derive(Default)]
struct Provider {
    codes: Mutex<HashMap<String, (String, String)>>, // code -> (challenge, user)
    tokens: Mutex<HashMap<String, String>>,          // token -> user
}

async fn provider_authorize(State(p): State<Arc<Provider>>, Query(q): Params) -> Response {
    // A real provider shows a consent page here; the mock consents at once.
    let code = random_token();
    p.codes.lock().unwrap().insert(code.clone(), (q["code_challenge"].clone(), "4242".into()));
    let mut to = Url::parse(&q["redirect_uri"]).unwrap();
    to.query_pairs_mut().append_pair("code", &code).append_pair("state", &q["state"]);
    Redirect::to(to.as_str()).into_response()
}

async fn provider_token(State(p): State<Arc<Provider>>, Json(q): Json<HashMap<String, String>>) -> Response {
    let Some((chal, user)) = p.codes.lock().unwrap().remove(&q["code"]) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if challenge(&q["code_verifier"]) != chal {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let token = random_token();
    p.tokens.lock().unwrap().insert(token.clone(), user);
    Json(serde_json::json!({ "access_token": token })).into_response()
}

async fn provider_user(State(p): State<Arc<Provider>>, headers: axum::http::HeaderMap) -> Response {
    let token = headers.get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    match token.and_then(|t| p.tokens.lock().unwrap().get(t).cloned()) {
        Some(id) => Json(serde_json::json!({ "id": id.parse::<u64>().unwrap(), "login": "alice" })).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

// ------------------------------------------------------------------ issuer

struct Flow {
    app_redirect: Url,
    app_state: String,
    app_challenge: String,
    provider_verifier: String,
    started: Instant,
}

struct Grant {
    subject: String,
    app_challenge: String,
    issued: Instant,
}

struct Issuer {
    me: SocketAddr,
    provider: SocketAddr,
    http: reqwest::Client,
    flows: Mutex<HashMap<String, Flow>>,
    grants: Mutex<HashMap<String, Grant>>,
    devices: Mutex<HashMap<String, Vec<String>>>, // subject -> learner keys
    purchases: HashMap<String, Vec<String>>,      // subject -> course IDs
}

const FLOW_TTL: Duration = Duration::from_secs(600);
const GRANT_TTL: Duration = Duration::from_secs(60);
const DEVICE_LIMIT: usize = 3;

/// Only a loopback `http://127.0.0.1:<port>/callback` may receive grants (RFC 8252 §7.3).
fn loopback_redirect(raw: &str) -> Option<Url> {
    let url = Url::parse(raw).ok()?;
    let ok = url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && url.path() == "/callback"
        && url.query().is_none()
        && url.username().is_empty()
        && url.password().is_none();
    ok.then_some(url)
}

async fn issuer_start(State(i): State<Arc<Issuer>>, Query(q): Params) -> Response {
    let (Some(redirect), Some(state), Some(chal)) =
        (q.get("redirect_uri").and_then(|r| loopback_redirect(r)), q.get("state"), q.get("code_challenge"))
    else {
        return (StatusCode::BAD_REQUEST, "invalid sign-in request").into_response();
    };
    if q.get("code_challenge_method").map(String::as_str) != Some("S256") || q.get("provider").map(String::as_str) != Some("github") {
        return (StatusCode::BAD_REQUEST, "invalid sign-in request").into_response();
    }
    let flow = random_token();
    let provider_verifier = random_token();
    let mut to = Url::parse(&format!("http://{}/authorize", i.provider)).unwrap();
    to.query_pairs_mut()
        .append_pair("client_id", "zeron-issuer")
        .append_pair("redirect_uri", &format!("http://{}/auth/callback", i.me))
        .append_pair("state", &flow)
        .append_pair("code_challenge", &challenge(&provider_verifier))
        .append_pair("code_challenge_method", "S256");
    i.flows.lock().unwrap().insert(
        flow,
        Flow { app_redirect: redirect, app_state: state.clone(), app_challenge: chal.clone(), provider_verifier, started: Instant::now() },
    );
    Redirect::to(to.as_str()).into_response()
}

async fn issuer_callback(State(i): State<Arc<Issuer>>, Query(q): Params) -> Response {
    let flow = q.get("state").and_then(|s| i.flows.lock().unwrap().remove(s));
    let Some(flow) = flow.filter(|f| f.started.elapsed() < FLOW_TTL) else {
        return (StatusCode::BAD_REQUEST, "sign-in expired; start again from Zeron").into_response();
    };
    let subject = async {
        let token: HashMap<String, String> = i
            .http
            .post(format!("http://{}/token", i.provider))
            .json(&serde_json::json!({ "code": q.get("code")?, "code_verifier": flow.provider_verifier }))
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()?;
        let user: serde_json::Value = i
            .http
            .get(format!("http://{}/user", i.provider))
            .bearer_auth(token.get("access_token")?)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()?;
        Some(format!("github:{}", user["id"].as_u64()?))
    }
    .await;
    let Some(subject) = subject else {
        return (StatusCode::BAD_GATEWAY, "GitHub sign-in failed").into_response();
    };
    let code = random_token();
    i.grants.lock().unwrap().insert(code.clone(), Grant { subject, app_challenge: flow.app_challenge, issued: Instant::now() });
    let mut to = flow.app_redirect;
    to.query_pairs_mut().append_pair("code", &code).append_pair("state", &flow.app_state);
    Redirect::to(to.as_str()).into_response()
}

#[derive(Serialize, Deserialize)]
struct Redeem {
    code: String,
    code_verifier: String,
    learner_key: String,
    signature: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Redeemed {
    subject: String,
    devices: usize,
    courses: Vec<String>,
}

async fn issuer_redeem(State(i): State<Arc<Issuer>>, Json(r): Json<Redeem>) -> Response {
    let refuse = |why: &'static str| (StatusCode::BAD_REQUEST, why).into_response();
    // One-time: the grant is removed before any check, so a failed attempt burns it.
    let Some(grant) = i.grants.lock().unwrap().remove(&r.code) else { return refuse("unknown code") };
    if grant.issued.elapsed() > GRANT_TTL {
        return refuse("expired code");
    }
    if challenge(&r.code_verifier) != grant.app_challenge {
        return refuse("wrong verifier");
    }
    let key = B64.decode(&r.learner_key).ok().and_then(|k| VerifyingKey::try_from(k.as_slice()).ok());
    let sig = B64.decode(&r.signature).ok().and_then(|s| Signature::from_slice(&s).ok());
    let (Some(key), Some(sig)) = (key, sig) else { return refuse("malformed key") };
    if key.verify(&redeem_message(&r.code, &r.learner_key), &sig).is_err() {
        return refuse("bad signature");
    }
    let mut devices = i.devices.lock().unwrap();
    let list = devices.entry(grant.subject.clone()).or_default();
    if !list.contains(&r.learner_key) {
        if list.len() >= DEVICE_LIMIT {
            return (StatusCode::CONFLICT, "device limit reached").into_response();
        }
        list.push(r.learner_key.clone());
    }
    Json(Redeemed {
        devices: list.len(),
        courses: i.purchases.get(&grant.subject).cloned().unwrap_or_default(),
        subject: grant.subject,
    })
    .into_response()
}

// ----------------------------------------------------------------- the app

struct SignIn {
    start_url: Url,
    verifier: String,
    callback: oneshot::Receiver<Result<String, String>>,
}

/// The desktop side: a one-shot loopback listener that accepts exactly one
/// callback whose `state` matches, then stops.
async fn begin_sign_in(issuer: SocketAddr) -> anyhow::Result<SignIn> {
    let (listener, addr) = bind().await?;
    let state = random_token();
    let verifier = random_token();
    let (tx, rx) = oneshot::channel();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let expected = state.clone();
    let app = Router::new().route(
        "/callback",
        get(move |Query(q): Params| {
            let tx = tx.clone();
            let expected = expected.clone();
            async move {
                let result = match (q.get("state"), q.get("code")) {
                    (Some(s), Some(c)) if *s == expected => Ok(c.clone()),
                    _ => Err("state mismatch".to_owned()),
                };
                let ok = result.is_ok();
                if let Some(tx) = tx.lock().unwrap().take() {
                    let _ = tx.send(result);
                }
                Html(if ok {
                    "<h1>Signed in</h1><p>You can close this tab and return to Zeron.</p>"
                } else {
                    "<h1>Sign-in failed</h1><p>Return to Zeron and try again.</p>"
                })
            }
        }),
    );
    tokio::spawn(async move { axum::serve(listener, app).await });
    let mut start_url = Url::parse(&format!("http://{issuer}/auth/start"))?;
    start_url
        .query_pairs_mut()
        .append_pair("provider", "github")
        .append_pair("redirect_uri", &format!("http://{addr}/callback"))
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge(&verifier))
        .append_pair("code_challenge_method", "S256");
    Ok(SignIn { start_url, verifier, callback: rx })
}

async fn redeem(http: &reqwest::Client, issuer: SocketAddr, code: &str, verifier: &str, key: &SigningKey) -> anyhow::Result<reqwest::Response> {
    let learner_key = B64.encode(key.verifying_key().as_bytes());
    let signature = B64.encode(key.sign(&redeem_message(code, &learner_key)).to_bytes());
    Ok(http
        .post(format!("http://{issuer}/auth/redeem"))
        .json(&Redeem { code: code.into(), code_verifier: verifier.into(), learner_key, signature })
        .send()
        .await?)
}

fn learner_key() -> SigningKey {
    let mut seed = [0_u8; 32];
    getrandom::fill(&mut seed).expect("os randomness");
    SigningKey::from_bytes(&seed)
}

async fn open_browser(url: &Url, mode: &str, http: &reqwest::Client) -> anyhow::Result<()> {
    match mode {
        "headless" => {
            let page = http.get(url.as_str()).send().await?.text().await?;
            ensure!(page.contains("Signed in"), "unexpected final page: {page}");
        }
        "print" => println!("open in a browser: {url}"),
        "system" => {
            let opener = if cfg!(target_os = "macos") { "open" } else if cfg!(windows) { "explorer" } else { "xdg-open" };
            std::process::Command::new(opener).arg(url.as_str()).spawn().context("open system browser")?;
        }
        other => bail!("unknown mode {other}"),
    }
    Ok(())
}

async fn start_servers() -> anyhow::Result<SocketAddr> {
    let (pl, paddr) = bind().await?;
    let provider = Arc::new(Provider::default());
    let papp = Router::new()
        .route("/authorize", get(provider_authorize))
        .route("/token", post(provider_token))
        .route("/user", get(provider_user))
        .with_state(provider);
    tokio::spawn(async move { axum::serve(pl, papp).await });

    let (il, iaddr) = bind().await?;
    let issuer = Arc::new(Issuer {
        me: iaddr,
        provider: paddr,
        http: reqwest::Client::new(),
        flows: Mutex::default(),
        grants: Mutex::default(),
        devices: Mutex::default(),
        purchases: HashMap::from([("github:4242".into(), vec!["rust-basics".into()])]),
    });
    let iapp = Router::new()
        .route("/auth/start", get(issuer_start))
        .route("/auth/callback", get(issuer_callback))
        .route("/auth/redeem", post(issuer_redeem))
        .with_state(issuer);
    tokio::spawn(async move { axum::serve(il, iapp).await });
    Ok(iaddr)
}

async fn sign_in(issuer: SocketAddr, mode: &str, http: &reqwest::Client, key: &SigningKey) -> anyhow::Result<Redeemed> {
    let started = Instant::now();
    let flow = begin_sign_in(issuer).await?;
    open_browser(&flow.start_url, mode, http).await?;
    let code = tokio::time::timeout(Duration::from_secs(300), flow.callback)
        .await
        .context("sign-in timed out")??
        .map_err(anyhow::Error::msg)?;
    let response = redeem(http, issuer, &code, &flow.verifier, key).await?;
    ensure!(response.status().is_success(), "redeem: {} {}", response.status(), response.text().await?);
    let redeemed: Redeemed = response.json().await?;
    println!("signed in as {} in {:?}: {:?}", redeemed.subject, started.elapsed(), redeemed);
    Ok(redeemed)
}

/// Drive the browser leg with a plain client but stop at the app callback, so
/// the test holds the issuer's one-time code itself.
async fn grant_code(issuer: SocketAddr, verifier: &str, redirect: &str) -> anyhow::Result<String> {
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build()?;
    let mut url = Url::parse(&format!("http://{issuer}/auth/start"))?;
    url.query_pairs_mut()
        .append_pair("provider", "github")
        .append_pair("redirect_uri", redirect)
        .append_pair("state", "s")
        .append_pair("code_challenge", &challenge(verifier))
        .append_pair("code_challenge_method", "S256");
    loop {
        let response = http.get(url.as_str()).send().await?;
        ensure!(response.status().is_redirection(), "stopped at {}", response.status());
        url = Url::parse(response.headers()["location"].to_str()?)?;
        if url.path() == "/callback" {
            return Ok(url.query_pairs().find(|(k, _)| k == "code").unwrap().1.into_owned());
        }
    }
}

async fn checks(issuer: SocketAddr) -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let key = learner_key();
    let cb = "http://127.0.0.1:9/callback";
    let status = |r: reqwest::Response| r.status().as_u16();

    let code = grant_code(issuer, "v1", cb).await?;
    ensure!(status(redeem(&http, issuer, &code, "v1", &key).await?) == 200);
    ensure!(status(redeem(&http, issuer, &code, "v1", &key).await?) == 400, "code replay accepted");
    println!("ok: a code redeems once");

    let code = grant_code(issuer, "v1", cb).await?;
    ensure!(status(redeem(&http, issuer, &code, "other", &key).await?) == 400);
    ensure!(status(redeem(&http, issuer, &code, "v1", &key).await?) == 400, "burned code accepted");
    println!("ok: a wrong PKCE verifier is refused and burns the code");

    let code = grant_code(issuer, "v1", cb).await?;
    let learner = B64.encode(key.verifying_key().as_bytes());
    let forged = B64.encode(learner_key().sign(&redeem_message(&code, &learner)).to_bytes());
    let response = http
        .post(format!("http://{issuer}/auth/redeem"))
        .json(&Redeem { code, code_verifier: "v1".into(), learner_key: learner, signature: forged })
        .send()
        .await?;
    ensure!(status(response) == 400);
    println!("ok: a redeem not signed by the presented learner key is refused");

    for bad in ["https://evil.example/callback", "http://localhost:9/callback", "http://127.0.0.1/callback", "http://127.0.0.1:9/other", "http://127.0.0.1:9/callback?x=1"] {
        ensure!(grant_code(issuer, "v1", bad).await.is_err(), "accepted redirect {bad}");
    }
    println!("ok: only http://127.0.0.1:<port>/callback receives codes");

    for _ in 0..2 {
        let code = grant_code(issuer, "v1", cb).await?;
        ensure!(status(redeem(&http, issuer, &code, "v1", &learner_key()).await?) == 200);
    }
    let code = grant_code(issuer, "v1", cb).await?;
    ensure!(status(redeem(&http, issuer, &code, "v1", &learner_key()).await?) == 409);
    let code = grant_code(issuer, "v1", cb).await?;
    ensure!(status(redeem(&http, issuer, &code, "v1", &key).await?) == 200);
    println!("ok: a 4th machine hits the device limit; a known machine still signs in");

    let flow = begin_sign_in(issuer).await?;
    let wrong = format!("{}?code=x&state=forged", flow.start_url.query_pairs().find(|(k, _)| k == "redirect_uri").unwrap().1);
    reqwest::get(&wrong).await?;
    ensure!(flow.callback.await? == Err("state mismatch".into()));
    println!("ok: the app refuses a callback with a forged state");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "headless".into());
    let issuer = start_servers().await?;
    let http = reqwest::Client::new();
    if mode == "checks" {
        return checks(issuer).await;
    }
    let key = learner_key();
    let first = sign_in(issuer, &mode, &http, &key).await?;
    // A second machine: a new learner key, same subject, same purchases.
    let second = sign_in(issuer, &mode, &http, &learner_key()).await?;
    ensure!(first.subject == second.subject && second.devices == 2 && second.courses == first.courses);
    println!("ok: a second machine got the same purchases");
    Ok(())
}
