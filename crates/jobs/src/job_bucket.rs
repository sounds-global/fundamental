use crate::JobSubject;

/// Which of the job table's 64 index ranges a subject's jobs live in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobBucket(i16);

impl JobBucket {
    /// How many buckets there are; fixed by the table's `CHECK`, so changing it is a migration.
    pub const COUNT: i16 = 64;

    const CONTEXT: &'static str = "fundamental.jobs.v1 bucket";

    /// The bucket `subject`'s jobs live in.
    #[must_use]
    pub fn of(subject: &JobSubject) -> Self {
        Self::of_name(subject.as_str())
    }

    #[expect(
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        reason = "readability baseline"
    )]
    pub(crate) fn of_name(subject: &str) -> Self {
        let digest = blake3::derive_key(Self::CONTEXT, subject.as_bytes());
        let spread = u16::from_be_bytes([digest[0], digest[1]]);
        // `COUNT` is a small positive constant, so the remainder always fits an `i16`.
        #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
        let bucket = (spread % Self::COUNT as u16) as i16;
        Self(bucket)
    }

    /// Every bucket from `first`, wrapping, so replicas starting apart spread their first claims.
    pub(crate) fn all_from(first: u8) -> impl Iterator<Item = Self> {
        let first = i16::from(first) % Self::COUNT;
        (0..Self::COUNT).map(move |offset| Self((first + offset) % Self::COUNT))
    }

    /// The bucket's number, `0..COUNT`.
    #[must_use]
    pub fn get(self) -> i16 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn a_subject_always_lands_in_the_same_bucket_within_range() {
        for index in 0..500 {
            let subject = JobSubject::new(format!("hold/{index}")).expect("a subject");
            let bucket = JobBucket::of(&subject);
            assert_eq!(bucket, JobBucket::of(&subject));
            assert!((0..JobBucket::COUNT).contains(&bucket.get()));
        }
    }

    #[test]
    fn subjects_spread_over_every_bucket() {
        let used: BTreeSet<_> = (0..2_000)
            .map(|index| JobBucket::of_name(&format!("hold/{index}")))
            .collect();
        assert_eq!(used.len(), 64);
    }

    #[test]
    fn walking_from_any_bucket_visits_each_once() {
        let walked: Vec<_> = JobBucket::all_from(250).collect();
        assert_eq!(walked.len(), 64);
        assert_eq!(walked.iter().collect::<BTreeSet<_>>().len(), 64);
        assert_eq!(walked[0].get(), 250 % 64);
    }
}
