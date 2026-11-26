use fundamental_macros::Identifier;
use uuid::{Builder, Uuid};

use crate::{JobKind, JobSubject};

/// A job's key: a digest of its kind and subject, so every writer of one job addresses one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Identifier)]
pub struct JobId(Uuid);

impl JobId {
    const CONTEXT: &'static str = "fundamental.jobs.v1 job-id";

    /// The id of the job of `kind` about `subject`.
    #[must_use]
    pub fn of<K: JobKind>(kind: K, subject: &JobSubject) -> Self {
        Self::of_names(kind.value(), subject.as_str())
    }

    /// The id of the job stored as `kind` and `subject`.
    pub(crate) fn of_names(kind: &str, subject: &str) -> Self {
        let mut material = Vec::with_capacity(kind.len() + 1 + subject.len());
        material.extend_from_slice(kind.as_bytes());
        material.push(0);
        material.extend_from_slice(subject.as_bytes());
        let digest = blake3::derive_key(Self::CONTEXT, &material);
        let mut bytes = [0_u8; 16];
        bytes.copy_from_slice(&digest[..16]);
        Self(Builder::from_custom_bytes(bytes).into_uuid())
    }

    /// The id as a UUID.
    #[must_use]
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_id_is_deterministic_and_a_version_8_uuid() {
        let first = JobId::of_names("hold.lapse", "hold/1");
        assert_eq!(first, JobId::of_names("hold.lapse", "hold/1"));
        assert_eq!(first.as_uuid().get_version_num(), 8);
    }

    #[test]
    fn kind_and_subject_cannot_be_shifted_into_one_another() {
        assert_ne!(
            JobId::of_names("hold.lapse", "hold/1"),
            JobId::of_names("hold.laps", "ehold/1")
        );
        assert_ne!(
            JobId::of_names("hold.lapse", "hold/1"),
            JobId::of_names("hold.lapse", "hold/2")
        );
    }
}
