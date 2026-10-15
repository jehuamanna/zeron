# Course and release versioning

## Identities

| Concept | Identity | Mutability |
|---|---|---|
| Logical course | `courseId` | stable forever |
| Draft | `draftId` + monotonically increasing revision | mutable |
| Module / lesson / resource | opaque stable ID scoped to course | metadata/order may change |
| Course version | publisher-selected SemVer string | immutable after release |
| Release | `releaseId` | immutable record |
| Torrent content | v1 SHA-1 info hash initially | derived and immutable |
| Annotation | learner-owned `annotationId` | mutable independently |

IDs are generated, not derived from titles or paths. A resource retains its ID only when it is logically the same learning object; changed bytes receive a new content digest while relocation alone may preserve identity.

## Release rules

A release is a tuple of course identity, version, canonical manifest digest, package inventory, torrent identity, and publication metadata. The pair `(courseId, version)` is unique. Republishing different bytes under the same version is rejected. Correcting a release creates a new version and release; prior releases remain available until explicitly archived.

`SUPERSEDED` means a newer release is preferred, not that old bytes can be deleted. Learners opt into migration and can keep multiple versions when notes or offline commitments require them.

## Annotation migration

Annotations reference course ID, source course version, lesson ID, resource ID, typed anchor payload, and optional source content digest. Migration is a recorded operation, never an overwrite:

1. Stable lesson/resource ID and unchanged content digest: migrate automatically.
2. Video with unchanged media digest: preserve timestamp/range.
3. Text/PDF with robust quote/context match and high confidence: create a migrated anchor while retaining provenance.
4. Missing ID, changed media, or ambiguous match: retain the original and mark `review_required` or `orphaned`.

The learner can view old-context data and accept/reject suggested anchors. Torrent synchronization never edits annotations.

## Storage and deduplication

Release directories are immutable. Cross-release deduplication is out of scope initially because filesystem linking, garbage collection, seeding commitments, and torrent piece boundaries create correctness risks. A future content-addressed blob store may be evaluated behind an ownership/refcount model; torrents must still observe the exact release file tree.

