//! `license-issuer`: run the reference license issuer (ADR 0017).
//!
//! ```text
//! license-issuer init  --data <dir>
//! license-issuer info  --data <dir>
//! license-issuer serve --data <dir> [--listen 127.0.0.1:8787]
//! license-issuer grant --data <dir> --course <courseId> --learner <learner ID>
//! ```
//!
//! `serve` speaks plain HTTP; put it behind a reverse proxy that terminates
//! TLS, because publishers' delegations name an `https://` address.

use std::{net::SocketAddr, path::PathBuf, process::ExitCode, sync::Arc};

use course_signing::PublicKey;
use license_issuer::{Issuer, IssuerKeys, Storage, router};

const USAGE: &str = "usage:
  license-issuer init  --data <dir>
  license-issuer info  --data <dir>
  license-issuer serve --data <dir> [--listen 127.0.0.1:8787]
  license-issuer grant --data <dir> --course <courseId> --learner <learner ID>";
const DEFAULT_LISTEN: &str = "127.0.0.1:8787";
const LEARNER_ID_PREFIX: &str = "zeron-learner:v1:";

fn main() -> ExitCode {
    match run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn option(arguments: &[String], name: &str) -> Option<String> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn run(arguments: &[String]) -> Result<(), String> {
    let command = arguments.first().map(String::as_str);
    let data = option(arguments, "--data")
        .map(PathBuf::from)
        .ok_or_else(|| USAGE.to_owned())?;
    match command {
        Some("init") => {
            let keys = IssuerKeys::create(&data).map_err(|error| error.to_string())?;
            Storage::open(&data).map_err(|error| error.to_string())?;
            print_info(&Issuer::new(
                keys,
                Storage::open_in_memory().map_err(|error| error.to_string())?,
            ));
            println!(
                "Back up {} and keep it private.",
                license_issuer::keys::KEY_FILE
            );
            Ok(())
        }
        Some("info") => {
            print_info(&open(&data)?);
            Ok(())
        }
        Some("grant") => {
            let course = option(arguments, "--course").ok_or_else(|| USAGE.to_owned())?;
            let learner = option(arguments, "--learner").ok_or_else(|| USAGE.to_owned())?;
            let learner_key = learner_signing_key(&learner)?;
            let new = open(&data)?
                .grant(&course, &learner_key, "manual")
                .map_err(|error| error.to_string())?;
            println!(
                "{} {} for course {course}.",
                if new { "Entitled" } else { "Already entitled:" },
                course_signing::fingerprint(&learner_key)
            );
            Ok(())
        }
        Some("serve") => {
            let listen: SocketAddr = option(arguments, "--listen")
                .unwrap_or_else(|| DEFAULT_LISTEN.to_owned())
                .parse()
                .map_err(|_| "--listen takes an address such as 127.0.0.1:8787".to_owned())?;
            let issuer = Arc::new(open(&data)?);
            let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind(listen)
                    .await
                    .map_err(|error| format!("cannot listen on {listen}: {error}"))?;
                println!("license-issuer listening on http://{listen}");
                axum::serve(listener, router(issuer))
                    .with_graceful_shutdown(async {
                        let _ = tokio::signal::ctrl_c().await;
                    })
                    .await
                    .map_err(|error| error.to_string())
            })
        }
        _ => Err(USAGE.to_owned()),
    }
}

fn open(data: &std::path::Path) -> Result<Issuer, String> {
    let keys = IssuerKeys::load(data).map_err(|error| error.to_string())?;
    let storage = Storage::open(data).map_err(|error| error.to_string())?;
    Ok(Issuer::new(keys, storage))
}

fn print_info(issuer: &Issuer) {
    let info = issuer.info();
    println!("Issuer key:     {}", info.issuer_key);
    println!("Encryption key: {}", info.encryption_key);
}

/// The signing key of a learner ID, `zeron-learner:v1:<signing>:<encryption>`.
fn learner_signing_key(learner_id: &str) -> Result<PublicKey, String> {
    let invalid =
        || "--learner takes a learner ID copied from Zeron's Settings → Learner key".to_owned();
    let rest = learner_id
        .trim()
        .strip_prefix(LEARNER_ID_PREFIX)
        .ok_or_else(invalid)?;
    let (signing, encryption) = rest.split_once(':').ok_or_else(invalid)?;
    if encryption.len() != 64 {
        return Err(invalid());
    }
    format!("ed25519-{signing}").parse().map_err(|_| invalid())
}
