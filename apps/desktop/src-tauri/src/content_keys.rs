//! Where the Library's content keys come from (ADR 0017): the keys this
//! computer's Publish area sealed releases with, so a publisher can open
//! their own paid courses, and licenses the learner imported, opened with
//! this computer's learner key. A keychain that is missing or fails reads as
//! "no key", and paid lessons stay locked.

use std::sync::Arc;

use learning_application::{ContentKey, ContentKeys, KeyRequest};

use crate::{learner_key::LearnerCredentials, publish::identity::Credentials};

pub(crate) struct LibraryContentKeys {
    pub(crate) publisher: Arc<Credentials>,
    pub(crate) learner: Arc<LearnerCredentials>,
}

impl LibraryContentKeys {
    fn published(&self, request: &KeyRequest<'_>) -> Option<ContentKey> {
        let store = self.publisher.store().ok()?;
        publisher_application::stored_content_key(
            store.as_ref(),
            request.course_id,
            request.version,
        )
        .ok()
        .flatten()
    }

    fn licensed(&self, request: &KeyRequest<'_>) -> Option<ContentKey> {
        if request.licensed.is_empty() {
            return None;
        }
        let store = self.learner.get().ok()?;
        let keys = learner_identity::stored_keys(store.as_ref()).ok()??;
        let me = keys.signing().public_key().to_string();
        let secret = keys.encryption_secret_bytes();
        request
            .licensed
            .iter()
            .filter(|licensed| licensed.learner_key == me)
            .find_map(|licensed| {
                course_access::unwrap_content_key(
                    &licensed.wrapped_content_key,
                    &secret,
                    request.course_id,
                    request.version,
                )
                .ok()
            })
    }
}

impl ContentKeys for LibraryContentKeys {
    fn content_key(&self, request: &KeyRequest<'_>) -> Option<ContentKey> {
        self.published(request).or_else(|| self.licensed(request))
    }
}

#[cfg(test)]
mod tests {
    use learner_identity::MemoryStore;
    use learning_application::LicensedContentKey;

    use super::*;

    #[test]
    fn a_licensed_key_opens_only_with_the_learner_key_it_was_sealed_to() {
        let learner_store = Arc::new(MemoryStore::default());
        let learner = learner_identity::create(learner_store.as_ref(), 1).unwrap();
        let content_key = ContentKey::from_bytes([5; 32]);
        let wrapped = course_access::wrap_content_key(
            &content_key,
            &learner.encryption_public_key().unwrap(),
            "c",
            "1.0.0",
        )
        .unwrap();
        let keys = LibraryContentKeys {
            publisher: Arc::new(Credentials::with(Arc::new(MemoryStore::default()))),
            learner: Arc::new(LearnerCredentials::with(learner_store)),
        };
        let licensed = [LicensedContentKey {
            learner_key: learner.signing_key.clone(),
            wrapped_content_key: wrapped.clone(),
        }];
        let request = |licensed| KeyRequest {
            course_id: "c",
            version: "1.0.0",
            licensed,
        };
        assert_eq!(keys.content_key(&request(&licensed)), Some(content_key));
        assert_eq!(keys.content_key(&request(&[])), None);
        let someone_else = [LicensedContentKey {
            learner_key: format!("ed25519-{}", "a".repeat(64)),
            wrapped_content_key: wrapped,
        }];
        assert_eq!(keys.content_key(&request(&someone_else)), None);
    }

    #[test]
    fn the_publishing_computer_opens_its_own_releases() {
        let publisher_store = Arc::new(MemoryStore::default());
        let key =
            publisher_application::release_content_key(publisher_store.as_ref(), "c", "1.0.0")
                .unwrap();
        let keys = LibraryContentKeys {
            publisher: Arc::new(Credentials::with(publisher_store)),
            learner: Arc::new(LearnerCredentials::with(Arc::new(MemoryStore::default()))),
        };
        let request = KeyRequest {
            course_id: "c",
            version: "1.0.0",
            licensed: &[],
        };
        assert_eq!(keys.content_key(&request), Some(key));
    }
}
