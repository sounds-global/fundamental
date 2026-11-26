use fundamental_macros::Error;

/// Why text has no skeleton under a profile; never echoes the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SkeletonError {
    /// The input is longer than the profile folds, refused before any of it was read.
    #[error(
        code = "SKEL_001",
        message = "That name is {len} bytes; at most {max} are accepted",
        developer_message = "The profile bounds input at {max} bytes so folding cannot be \
                             made expensive by a long name; refuse at the boundary that \
                             accepts it rather than here",
        status = 413
    )]
    TooLong {
        /// The input's length in bytes.
        len: usize,
        /// The bound it exceeded.
        max: usize,
    },

    /// Every character folded away, leaving nothing.
    #[error(
        code = "SKEL_002",
        message = "That name has no readable characters",
        developer_message = "Every character was removed or folded away; an empty skeleton \
                             would collide with every other such name",
        status = 422
    )]
    FoldsToNothing,

    /// The profile's folds still changed the text after every pass, so they cycle.
    #[error(
        code = "SKEL_003",
        message = "That name could not be folded",
        developer_message = "Profile `{profile}` still changed the text after {passes} \
                             passes, so two of its folds rewrite each other's output; make \
                             the set converge",
        status = 500
    )]
    FoldsDoNotConverge {
        /// How many passes ran before the attempt was abandoned.
        passes: usize,
        /// The profile whose folds cycle, so the defect is findable from a log line.
        profile: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::SkeletonError;

    #[test]
    fn every_refusal_has_its_own_code_and_is_never_retryable() {
        let refusals = [
            SkeletonError::TooLong { len: 300, max: 253 },
            SkeletonError::FoldsToNothing,
            SkeletonError::FoldsDoNotConverge {
                passes: 9,
                profile: "cycling",
            },
        ];

        let mut codes: Vec<&str> = refusals.iter().map(|r| r.error_code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), refusals.len(), "two variants share a code");

        for refusal in &refusals {
            assert!(
                !refusal.is_retryable(),
                "{refusal:?} is retryable, but the same name folds the same way every time"
            );
        }
    }

    #[test]
    fn a_bad_name_blames_the_caller_and_a_bad_profile_blames_us() {
        assert_eq!(
            SkeletonError::TooLong { len: 300, max: 253 }.http_status_code(),
            413
        );
        assert_eq!(SkeletonError::FoldsToNothing.http_status_code(), 422);
        assert_eq!(
            SkeletonError::FoldsDoNotConverge {
                passes: 9,
                profile: "cycling"
            }
            .http_status_code(),
            500
        );
    }

    #[test]
    fn a_refusal_names_the_length_and_never_the_name() {
        let refusal = SkeletonError::TooLong { len: 300, max: 253 };
        let message = refusal.to_string();
        assert!(message.contains("300"), "{message}");
        assert!(message.contains("253"), "{message}");
    }
}
