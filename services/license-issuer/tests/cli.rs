//! The `license-issuer` binary: init, info, grant, and serve.

use std::{
    path::PathBuf,
    process::{Command, Output},
    time::{Duration, Instant},
};

struct DataDirectory(PathBuf);

impl DataDirectory {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("zeron-issuer-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for DataDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn issuer(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_license-issuer"))
        .args(arguments)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn init_info_and_grant_work_on_a_data_directory() {
    let data = DataDirectory::new("commands");
    let path = data.0.to_str().unwrap();
    let created = issuer(&["init", "--data", path]);
    assert!(created.status.success(), "{}", text(&created));
    assert!(text(&created).contains("Issuer key:     ed25519-"));
    assert!(
        !issuer(&["init", "--data", path]).status.success(),
        "never replaces a key"
    );

    let info = issuer(&["info", "--data", path]);
    assert!(info.status.success());
    assert!(text(&created).contains(text(&info).lines().next().unwrap()));

    let refused = issuer(&[
        "grant",
        "--data",
        path,
        "--course",
        "c",
        "--learner",
        "nonsense",
    ]);
    assert!(!refused.status.success());
    assert!(text(&refused).contains("learner ID"));
    let learner = format!(
        "zeron-learner:v1:{}:{}",
        "5748c78a43d41aaa54e830d638aa2700e23c389039105e92f31a8123b22bdf27",
        "b".repeat(64)
    );
    let granted = issuer(&[
        "grant",
        "--data",
        path,
        "--course",
        "c",
        "--learner",
        &learner,
    ]);
    assert!(granted.status.success(), "{}", text(&granted));
    assert!(text(&granted).starts_with("Entitled"));
    let again = issuer(&[
        "grant",
        "--data",
        path,
        "--course",
        "c",
        "--learner",
        &learner,
    ]);
    assert!(text(&again).starts_with("Already entitled"));

    assert!(
        !issuer(&["info", "--data", "/nonexistent/zeron-issuer"])
            .status
            .success()
    );
    assert!(!issuer(&["nonsense"]).status.success());
}

#[test]
fn serve_answers_on_the_chosen_loopback_address() {
    let data = DataDirectory::new("serve");
    let path = data.0.to_str().unwrap();
    assert!(issuer(&["init", "--data", path]).status.success());
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let listen = format!("127.0.0.1:{port}");
    let mut child = Command::new(env!("CARGO_BIN_EXE_license-issuer"))
        .args(["serve", "--data", path, "--listen", &listen])
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let answer = loop {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let response = runtime.block_on(async {
            reqwest::get(format!("http://{listen}/v1/issuer"))
                .await
                .ok()?
                .text()
                .await
                .ok()
        });
        if let Some(body) = response {
            break body;
        }
        assert!(Instant::now() < deadline, "the issuer never answered");
        std::thread::sleep(Duration::from_millis(100));
    };
    let _ = child.kill();
    let _ = child.wait();
    assert!(answer.contains("issuerKey") && answer.contains("x25519-"));
}
