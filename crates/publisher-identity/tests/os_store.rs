//! The real credential store of this platform: the macOS Keychain, the
//! Windows Credential Manager, or the Secret Service on Linux.
//!
//! These tests write to the user's real keychain, so they run only when
//! `ZERON_KEYCHAIN_TESTS=1`. CI sets it on every platform.

use publisher_identity::{
    CredentialStore, IdentityError, OsStore, create, export_backup, forget_key, restore_backup,
    rotate_key, signing_key,
};

fn enabled() -> bool {
    std::env::var("ZERON_KEYCHAIN_TESTS").is_ok_and(|value| value == "1")
}

#[test]
fn the_platform_keychain_holds_rotates_and_forgets_signing_keys() {
    if !enabled() {
        eprintln!("skipped: set ZERON_KEYCHAIN_TESTS=1 to use the real keychain");
        return;
    }
    let store = OsStore::open().expect("this platform's keychain is available");
    let identity = create(&store, 1).unwrap();
    let first = signing_key(&store, &identity).unwrap();
    assert_eq!(first.public_key().to_string(), identity.current);

    let rotated = rotate_key(&store, &identity, 2).unwrap();
    assert_eq!(
        signing_key(&store, &rotated)
            .unwrap()
            .public_key()
            .to_string(),
        rotated.current
    );
    forget_key(&store, &identity.current_key().unwrap()).unwrap();
    assert!(matches!(
        signing_key(&store, &identity),
        Err(IdentityError::MissingKey)
    ));

    let backup = export_backup(&store, &rotated, "a long test passphrase").unwrap();
    forget_key(&store, &rotated.current_key().unwrap()).unwrap();
    assert!(matches!(
        signing_key(&store, &rotated),
        Err(IdentityError::MissingKey)
    ));
    let restored = restore_backup(&store, &backup, "a long test passphrase").unwrap();
    assert_eq!(restored, rotated);
    assert!(signing_key(&store, &restored).is_ok());
    forget_key(&store, &rotated.current_key().unwrap()).unwrap();
    assert!(store.load("signing-key-absent").unwrap().is_none());
}
