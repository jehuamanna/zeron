//! Spike: wrap a release content key to a learner's X25519 key (ADR 0017 decision 4)
//! and check that `hpke` (rozbb) and `hpke-ng` interoperate in both directions.
//!
//! `cargo run --features ng --bin hpke`

use anyhow::ensure;
use hpke::{Deserializable, Kem as _, OpModeR, OpModeS, Serializable};
use hpke_ng::Kem as _;

type RKem = hpke::kem::X25519HkdfSha256;
type RKdf = hpke::kdf::HkdfSha256;
type RAead = hpke::aead::ChaCha20Poly1305;
type NgSuite = hpke_ng::Hpke<hpke_ng::DhKemX25519HkdfSha256, hpke_ng::HkdfSha256, hpke_ng::ChaCha20Poly1305>;
type NgKem = hpke_ng::DhKemX25519HkdfSha256;

const INFO: &[u8] = b"zeron.license.v1";

fn main() -> anyhow::Result<()> {
    let content_key = [42_u8; 32];
    let aad = b"course:rust-basics\nversion:1.2.0";
    let mut rng = rand_core::UnwrapErr(getrandom::SysRng);

    // rozbb seals -> hpke-ng opens.
    let (sk, pk) = RKem::gen_keypair();
    let (enc, ct) = hpke::single_shot_seal::<RAead, RKdf, RKem>(&OpModeS::Base, &pk, INFO, &content_key, aad)?;
    let ng_sk = NgKem::sk_from_bytes(&sk.to_bytes())?;
    let opened = NgSuite::open_base(&NgKem::enc_from_bytes(&enc.to_bytes())?, &ng_sk, INFO, aad, &ct)?;
    ensure!(opened == content_key);
    println!("ok: hpke seals, hpke-ng opens (enc {} B, sealed key {} B)", enc.to_bytes().len(), ct.len());

    // hpke-ng seals -> rozbb opens.
    let (ng_sk, ng_pk) = NgKem::generate(&mut rng)?;
    let (enc, ct) = NgSuite::seal_base(&mut rng, &ng_pk, INFO, aad, &content_key)?;
    let sk = <RKem as hpke::Kem>::PrivateKey::from_bytes(&NgKem::sk_to_bytes(&ng_sk))?;
    let enc = <RKem as hpke::Kem>::EncappedKey::from_bytes(enc.as_ref())?;
    let opened = hpke::single_shot_open::<RAead, RKdf, RKem>(&OpModeR::Base, &sk, &enc, INFO, &ct, aad)?;
    ensure!(opened == content_key);
    println!("ok: hpke-ng seals, hpke opens");

    // Binding: a different course in the AAD, or another learner's key, refuses.
    let (sk2, _) = RKem::gen_keypair();
    ensure!(hpke::single_shot_open::<RAead, RKdf, RKem>(&OpModeR::Base, &sk, &enc, INFO, &ct, b"course:other").is_err());
    ensure!(hpke::single_shot_open::<RAead, RKdf, RKem>(&OpModeR::Base, &sk2, &enc, INFO, &ct, aad).is_err());
    println!("ok: wrong course or wrong learner key is refused");
    Ok(())
}
