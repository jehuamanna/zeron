//! The learner key in this platform's real credential store.
//!
//! Runs only when `ZERON_KEYCHAIN_TESTS=1`, as CI sets on every platform. It
//! uses its own service name, so it never touches a real learner key.

use learner_identity::{
    ACCOUNT, CredentialStore, LearnerKeyError, MemoryStore, OsStore, create, export_backup,
    learner_keys, restore_backup, stored_identity,
};

const TEST_SERVICE: &str = "app.zeron.learner.test";

fn enabled() -> bool {
    std::env::var("ZERON_KEYCHAIN_TESTS").is_ok_and(|value| value == "1")
}

#[test]
fn the_platform_keychain_holds_backs_up_and_restores_the_learner_key() {
    if !enabled() {
        eprintln!("skipped: set ZERON_KEYCHAIN_TESTS=1 to use the real keychain");
        return;
    }
    let store = OsStore::open_service(TEST_SERVICE).expect("this platform's keychain is available");
    store.delete(ACCOUNT).unwrap();
    let identity = create(&store, 1).unwrap();
    assert!(matches!(create(&store, 2), Err(LearnerKeyError::Exists)));
    assert_eq!(stored_identity(&store, 1).unwrap(), Some(identity.clone()));
    assert!(learner_keys(&store, &identity).is_ok());

    let backup = export_backup(&store, &identity, "correct horse battery").unwrap();
    store.delete(ACCOUNT).unwrap();
    assert!(matches!(
        learner_keys(&store, &identity),
        Err(LearnerKeyError::Missing)
    ));
    assert_eq!(
        restore_backup(&store, &backup, "correct horse battery").unwrap(),
        identity
    );
    assert!(learner_keys(&store, &identity).is_ok());
    // A backup restores on a computer without a keychain entry too.
    assert!(restore_backup(&MemoryStore::default(), &backup, "correct horse battery").is_ok());
    store.delete(ACCOUNT).unwrap();
}
