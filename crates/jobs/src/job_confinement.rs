//! [`JobConfinement`] — the jobs about one owner, for a test that drains a table others share.

use fundamental_db::Filter;
use uuid::Uuid;

use crate::JobError;

/// The jobs whose subject names one owner's id, so a test's drain fires only that owner's jobs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobConfinement(Uuid);

impl JobConfinement {
    /// The jobs about `owner`.
    ///
    /// # Errors
    ///
    /// [`JobError::NobodyToConfineTo`] for the nil id, which subjects about no one in particular
    /// use.
    pub fn naming(owner: Uuid) -> Result<Self, JobError> {
        if owner.is_nil() {
            return Err(JobError::NobodyToConfineTo);
        }
        Ok(Self(owner))
    }

    /// The owner.
    #[must_use]
    pub const fn owner(&self) -> Uuid {
        self.0
    }

    /// The filter on `column` that a subject naming the owner passes; a hex id holds no wildcard.
    pub(crate) fn subject_filter(&self, column: &str) -> Filter {
        Filter::or(vec![
            Filter::like(column, format!("%{}%", self.0.hyphenated())),
            Filter::like(column, format!("%{}%", self.0.simple())),
        ])
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;
    use uuid::Uuid;

    use super::JobConfinement;

    #[test]
    fn a_confinement_names_its_owner() {
        let owner = Uuid::from_u128(0x0192_8f3c_7a2b_7c11_9d4e_5f60_7182_93a4);
        let confined = JobConfinement::naming(owner).expect("an owner");
        assert_eq!(confined.owner(), owner);
        let filter = format!("{:?}", confined.subject_filter("subject"));
        for form in [
            "%01928f3c-7a2b-7c11-9d4e-5f60718293a4%",
            "%01928f3c7a2b7c119d4e5f60718293a4%",
        ] {
            assert!(filter.contains(form), "{form} in {filter}");
        }
    }

    #[test]
    fn nobody_is_not_an_owner() {
        let refused = JobConfinement::naming(Uuid::nil()).expect_err("the nil id");
        assert_eq!(refused.error_code(), "JOBS_015");
    }
}
