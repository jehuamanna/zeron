//! The Library's record of this computer's learner key (ADR 0017
//! decision 3). Only the public identity is stored, as the JSON the
//! `learner-identity` crate defines; the secret stays in the keychain.

use crate::{CourseLibrary, LibraryError};

const LEARNER_IDENTITY_KEY: &str = "learner_identity";

impl CourseLibrary {
    /// The recorded public learner identity, as JSON, if one was created or
    /// restored on this computer.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query failure.
    pub fn learner_identity_record(&self) -> Result<Option<String>, LibraryError> {
        Ok(self.storage.setting(LEARNER_IDENTITY_KEY)?)
    }

    /// Record the public learner identity, as JSON.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on write failure.
    pub fn set_learner_identity_record(&mut self, json: &str) -> Result<(), LibraryError> {
        Ok(self.storage.set_setting(LEARNER_IDENTITY_KEY, json)?)
    }
}

#[cfg(test)]
mod tests {
    use crate::{CourseLibrary, ImportLimits};

    #[test]
    fn the_learner_identity_record_round_trips() {
        let mut library = CourseLibrary::open_in_memory(ImportLimits::default()).unwrap();
        assert_eq!(library.learner_identity_record().unwrap(), None);
        library.set_learner_identity_record("{\"a\":1}").unwrap();
        assert_eq!(
            library.learner_identity_record().unwrap().as_deref(),
            Some("{\"a\":1}")
        );
    }
}
