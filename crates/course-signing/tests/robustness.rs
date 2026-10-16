//! Mutated signatures and catalogs never panic the parsers, and nothing a
//! mutation changes verifies as the publisher's statement.

use course_signing::{
    CatalogEntry, SecretKey, parse_catalog, parse_release_signature, revoke, rotate, sign_catalog,
    sign_release, verify_catalog, verify_release,
};
use mutation_testing::mutations;

#[test]
fn mutated_release_signatures_never_verify_as_something_else() {
    let (root, next) = (
        SecretKey::from_bytes(&[1; 32]),
        SecretKey::from_bytes(&[2; 32]),
    );
    let rotations = vec![rotate(&root, &next.public_key(), 5)];
    let digest = "ab".repeat(32);
    let signed = sign_release(&next, "course_a", "1.0.0", &digest, 10, rotations);
    let original = signed.to_bytes();
    let publisher = root.public_key().to_string();
    for (index, input) in mutations(&[&original], 0x5161_0001, 4_000, 64 * 1024)
        .iter()
        .enumerate()
    {
        let Ok(parsed) = parse_release_signature(input) else {
            continue;
        };
        if verify_release(&parsed, "course_a", "1.0.0", &digest, &publisher, &[]).is_ok() {
            assert_eq!(
                parsed.statement, signed.statement,
                "mutation {index} verified a changed statement"
            );
        }
    }
}

#[test]
fn mutated_catalogs_never_verify_as_something_else() {
    let root = SecretKey::from_bytes(&[3; 32]);
    let info_hash = "c".repeat(40);
    let entry = CatalogEntry {
        course_id: "course_a".to_owned(),
        course_version: "1.0.0".to_owned(),
        title: "Course".to_owned(),
        description: "About".to_owned(),
        magnet: format!("magnet:?xt=urn:btih:{info_hash}"),
        info_hash,
        manifest_sha256: "d".repeat(64),
    };
    let stranger = SecretKey::from_bytes(&[4; 32]).public_key();
    let revocation = revoke(&root, &stranger, 1, "unused");
    let catalog = sign_catalog(
        &root,
        &root.public_key(),
        "Catalog",
        9,
        vec![entry],
        vec![revocation],
        Vec::new(),
    );
    let original = catalog.to_bytes();
    for (index, input) in mutations(&[&original], 0x5161_0002, 4_000, 128 * 1024)
        .iter()
        .enumerate()
    {
        let Ok(parsed) = parse_catalog(input) else {
            continue;
        };
        if verify_catalog(&parsed).is_ok() {
            assert_eq!(
                parsed.statement, catalog.statement,
                "mutation {index} verified a changed catalog"
            );
        }
    }
}
