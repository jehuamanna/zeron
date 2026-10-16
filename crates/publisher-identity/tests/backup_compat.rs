//! Backups written before the passphrase envelope was shared with learner
//! keys (ADR 0017) still restore unchanged.

use publisher_identity::{MemoryStore, restore_backup, signing_key};

/// A throwaway test identity, exported by the previous code with the
/// passphrase below. It signs nothing real.
const BACKUP: &[u8] = include_bytes!("fixtures/publisher-key-backup-v1.json");

#[test]
fn a_backup_from_the_previous_format_restores() {
    let store = MemoryStore::default();
    let identity = restore_backup(&store, BACKUP, "correct horse battery").unwrap();
    assert_eq!(identity.created_at, 1_700_000_000);
    assert_eq!(
        signing_key(&store, &identity)
            .unwrap()
            .public_key()
            .to_string(),
        identity.current
    );
}
