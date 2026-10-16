# Phase 9: signed catalogs, revocations, and discovery

Status: complete. Publish → Catalog builds a signed catalog: the releases the publisher explicitly confirmed, plus revocations of retired keys. It exports the catalog as a file to host anywhere. The Library imports catalogs from a file or an HTTPS address, verifies them, refreshes followed ones, and imports their courses through the usual magnet import, which verifies each release's own signature.

## Delivered

- **`course-signing`:**
  - **`RevocationRecord`:** a revocation together with the rotation chain it arrived with. `verify_release` checks revocations against each record's chain as well as the release's own. This closes a bypass found while testing: a thief with a retired key could otherwise sign releases without the rotations that follow it, and the successor key's revocation could not be checked.
  - **Timing:** a key revoked from its retirement moment still hands over validly at that moment. A rotation counts as revoked only if made after the revocation time, while a signature counts as revoked from that time on.
- **Publisher (`publisher-storage` schema v6, `publisher-application::catalog`):**
  - Table `catalog_releases` records per-release confirmations.
  - `set_catalog_release` refuses archived releases and releases whose files were deleted.
  - `catalog_title` and `set_catalog_title` (1–200 characters).
  - `revoke_retired_key` takes only keys this identity retired, signs with the current key, and keeps one revocation per key.
  - `build_catalog` lists confirmed, non-archived releases read back from their verified packages (descriptions come from `course.json`). It carries every issued revocation and the rotation chain, and refuses a key that is not the identity's.
  - **Commands:** `get_catalog`, `set_catalog_title`, `set_catalog_release`, `revoke_retired_key` (revoked from the key's retirement time), and `export_catalog` (native save dialog).
- **Library (`learning-storage` schema v14, `learning-application::catalogs`):**
  - Table `catalogs` holds the verified document, the issue time, and the source address.
  - **`import_catalog`** verifies and refuses a revoked signer. It refuses a catalog older than the one kept, so a stale copy cannot hide a later revocation. It keeps each revocation with the catalog's rotation chain, and keeps the follow address across re-imports from file.
  - **`catalogs`** labels each entry `available`, `in_library`, `update` (a newer semantic version from the pinned publisher), `other_version`, or `other_publisher`.
  - Also `catalog_source` and `remove_catalog` (courses and revocations stay).
  - **Commands:** `list_catalogs`, `choose_catalog_file`, `import_catalog_url` and `refresh_catalog` (plain `https://` only, no credentials, at most 2 KiB address, 15 s timeout, three redirects, 1 MiB body), and `remove_catalog`. Error codes: `catalog_invalid`, `catalog_download_failed`, `catalog_not_followed`, `catalog_unreadable`.
- **UI:**
  - **Publish → Catalog:** the title, release rows with "List in catalog…" behind an explicit confirmation, unlisting, archived releases hidden, export, and retired keys with "Revoke…" and a reason. It asks for an identity first.
  - **Library → Catalogs:**
    - add a catalog file or follow an address;
    - each catalog shows its fingerprint, course and update counts, refresh, and forget;
    - each entry shows its status, with Import or Update through the magnet import.

## Compatibility

- **Storage:** publisher schema v5 → v6, and learner schema v13 → v14. Both add tables only.
- **IPC:** additive.

## Acceptance evidence

- **`course-signing`:** 14 unit tests, including `a_stolen_retired_key_cannot_hide_its_revocation_by_leaving_out_the_chain`. It covers revocation through the catalog's chain, a release from before the revocation, and a tampered chain being ignored.
- **Publisher test** `the_catalog_lists_confirmed_releases_and_carries_revocations`:
  - title rules and refusal of archived releases;
  - revoking only retired keys, with a reason required and one revocation per key;
  - the exported catalog parses and verifies, lists only the confirmed live release with its magnet, and carries the revocation;
  - a stranger's key is refused.
- **Command tests:**
  - **Publisher:** the catalog needs an identity, retired keys and revocations are reported, the export verifies, and no secret appears.
  - **Library:** only plain HTTPS addresses are followed, plain HTTP is never fetched, and invalid catalogs are refused without storage details.
- **Library test** `catalogs_list_releases_carry_revocations_and_never_go_back`, through the real publisher pipeline:
  - three listed releases, one signed with the retired key after its retirement;
  - all are `available`;
  - 1.0.0 imports, and the stolen-key 1.1.0 is refused naming "old laptop";
  - statuses then read `in_library` and `update`, and 1.2.0 imports;
  - an older catalog and a tampered title are refused;
  - the follow address is kept;
  - forgetting the catalog keeps the course.
- **Vitest:**
  - **Library:** catalog fingerprint and statuses, import through the magnet flow and tracking, follow with a refusal message, refresh and forget, and no import without torrents.
  - **Publish:** the identity prompt, listing only after confirmation with archived releases hidden, title and export, and revocation with a required reason.
- All repository gates pass.

## Manual verification

1. Publish → Catalog: list a release, confirm, and export `zeron-catalog.json`. Put it on any HTTPS host.
2. In another Zeron, Library → Catalogs → Follow address… with that URL. The catalog should show the same fingerprint as Publish → Identity. Import the course from it.
3. Publish a newer version, list it, export again, and replace the hosted file. Refresh in the Library; the entry should read "Update available".

## Current limitations

- There is no catalog server or search across publishers; discovery is through the catalogs a learner adds.
- Followed catalogs refresh only on request, not in the background.
- A revocation reaches learners only through a catalog they import or refresh.
