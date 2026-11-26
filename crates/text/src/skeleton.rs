use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;

use unicode_normalization::UnicodeNormalization;

use crate::{ProfileFingerprint, SkeletonError, SkeletonProfile};

/// What a name looks like, rather than what it spells: confusable names share one skeleton.
///
/// The fold, in order: bound at [`SkeletonProfile::MAX_BYTES`], delete
/// [`SkeletonProfile::REMOVED`], NFKC, the UTS #39 skeleton, [`SkeletonProfile::CASE`]
/// (after the mapping, as a prototype may be upper case), then [`SkeletonProfile::FOLDS`]
/// to a fixed point. Store [`ProfileFingerprint`] beside it.
pub struct Skeleton<P: SkeletonProfile> {
    text: String,
    profile: PhantomData<fn() -> P>,
}

impl<P: SkeletonProfile> Skeleton<P> {
    /// The skeleton of `input` under `P`.
    ///
    /// # Errors
    /// [`SkeletonError::TooLong`] when `input` exceeds [`SkeletonProfile::MAX_BYTES`];
    /// [`SkeletonError::FoldsToNothing`] when every character folds away;
    /// [`SkeletonError::FoldsDoNotConverge`] when `P`'s folds cycle.
    pub fn of(input: &str) -> Result<Self, SkeletonError> {
        if input.len() > P::MAX_BYTES {
            return Err(SkeletonError::TooLong {
                len: input.len(),
                max: P::MAX_BYTES,
            });
        }

        let kept: String = input.chars().filter(|c| !P::REMOVED.contains(c)).collect();
        let compatible: String = kept.nfkc().collect();
        let unconfused: String =
            unicode_security::confusable_detection::skeleton(&compatible).collect();
        let cased = P::CASE.apply(&unconfused);
        let text = apply_folds::<P>(&cased)?;

        if text.is_empty() {
            return Err(SkeletonError::FoldsToNothing);
        }
        Ok(Self {
            text,
            profile: PhantomData,
        })
    }

    /// The skeleton as stored.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The one name of the rules that produced this skeleton.
    #[must_use]
    pub fn fingerprint() -> ProfileFingerprint {
        ProfileFingerprint::of::<P>()
    }
}

/// Fold `text` to a fixed point, since one fold can create another's source; longest source wins.
fn apply_folds<P: SkeletonProfile>(text: &str) -> Result<String, SkeletonError> {
    if P::FOLDS.is_empty() {
        return Ok(text.to_owned());
    }
    // Every effective pass applies at least one fold. A fold that shortens can do so at
    // most `len` times, so anything still changing past that many passes is cycling.
    let budget = text.len() + 1;
    let mut current = text.to_owned();
    for _ in 0..budget {
        let next = one_pass::<P>(&current);
        if next == current {
            return Ok(current);
        }
        current = next;
    }
    Err(SkeletonError::FoldsDoNotConverge {
        passes: budget,
        profile: P::NAME,
    })
}

/// One left-to-right pass, preferring the longest source that matches at each position.
fn one_pass<P: SkeletonProfile>(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let mut best: Option<&crate::Fold> = None;
        for fold in P::FOLDS {
            if !fold.from.is_empty()
                && rest.starts_with(fold.from)
                && best.is_none_or(|current| fold.from.len() > current.from.len())
            {
                best = Some(fold);
            }
        }
        if let Some(fold) = best {
            out.push_str(fold.to);
            rest = &rest[fold.from.len()..];
            continue;
        }
        let mut characters = rest.chars();
        if let Some(character) = characters.next() {
            out.push(character);
            rest = characters.as_str();
        }
    }
    out
}

impl<P: SkeletonProfile> Clone for Skeleton<P> {
    fn clone(&self) -> Self {
        Self {
            text: self.text.clone(),
            profile: PhantomData,
        }
    }
}

impl<P: SkeletonProfile> PartialEq for Skeleton<P> {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl<P: SkeletonProfile> Eq for Skeleton<P> {}

impl<P: SkeletonProfile> PartialOrd for Skeleton<P> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<P: SkeletonProfile> Ord for Skeleton<P> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.text.cmp(&other.text)
    }
}

impl<P: SkeletonProfile> Hash for Skeleton<P> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state);
    }
}

impl<P: SkeletonProfile> fmt::Debug for Skeleton<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Skeleton")
            .field(&P::NAME)
            .field(&self.text)
            .finish()
    }
}

impl<P: SkeletonProfile> fmt::Display for Skeleton<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::Skeleton;
    use crate::{CaseFold, Fold, SkeletonError, SkeletonProfile};

    struct Name;
    impl SkeletonProfile for Name {
        const NAME: &'static str = "test name";
        const MAX_BYTES: usize = 253;
        const REMOVED: &'static [char] = &['-'];
        const CASE: CaseFold = CaseFold::Lowercase;
        const FOLDS: &'static [Fold] = &[
            Fold::new("rn", "m"),
            Fold::new("vv", "w"),
            Fold::new("cl", "d"),
            Fold::new("0", "o"),
            Fold::new("1", "l"),
            Fold::new("5", "s"),
        ];
    }

    struct NoFolds;
    impl SkeletonProfile for NoFolds {
        const NAME: &'static str = "test plain";
        const MAX_BYTES: usize = 16;
        const REMOVED: &'static [char] = &[];
        const CASE: CaseFold = CaseFold::None;
        const FOLDS: &'static [Fold] = &[];
    }

    fn skeleton(input: &str) -> String {
        Skeleton::<Name>::of(input)
            .expect("the test inputs all fold to something")
            .as_str()
            .to_owned()
    }

    #[test]
    fn look_alikes_share_a_skeleton() {
        for (a, b) in [
            ("acme", "acrne"),
            ("lngbank", "1ng-bank"),
            ("google", "g00gle"),
            ("wise", "vvi5e"),
            ("dropbox", "clropbox"),
            ("dropbox", "c1ropbox"),
        ] {
            assert_eq!(skeleton(a), skeleton(b), "{a} / {b}");
        }
    }

    #[test]
    fn distinct_names_keep_distinct_skeletons() {
        for (a, b) in [("acme", "acne"), ("bakker", "backer"), ("ing", "ling")] {
            assert_ne!(skeleton(a), skeleton(b), "{a} / {b}");
        }
    }

    #[test]
    fn a_cyrillic_look_alike_folds_to_its_latin_twin() {
        assert_eq!(skeleton("\u{0441}o\u{0433}e"), skeleton("core"));
        assert_eq!(skeleton("\u{0430}cme"), skeleton("acme"));
    }

    #[test]
    fn a_fullwidth_spelling_folds_to_its_plain_form() {
        assert_eq!(skeleton("\u{ff21}CME"), skeleton("acme"));
    }

    #[test]
    fn case_is_not_a_second_name() {
        assert_eq!(skeleton("ACME"), skeleton("acme"));
    }

    #[test]
    fn a_skeleton_is_its_own_skeleton() {
        for input in [
            "acme",
            "acrne",
            "g00gle",
            "vvi5e",
            "clropbox",
            "c1ropbox",
            "rnrnrn",
            "cl-cl",
            "\u{0441}o\u{0433}e",
        ] {
            let once = skeleton(input);
            assert_eq!(skeleton(&once), once, "{input}");
        }
    }

    #[test]
    fn a_name_over_the_bound_is_refused_before_it_is_folded() {
        let long = "a".repeat(254);
        assert_eq!(
            Skeleton::<Name>::of(&long),
            Err(SkeletonError::TooLong { len: 254, max: 253 })
        );
    }

    #[test]
    fn the_bound_itself_is_accepted() {
        let exact = "a".repeat(253);
        assert!(Skeleton::<Name>::of(&exact).is_ok());
    }

    #[test]
    fn a_name_that_folds_to_nothing_is_refused() {
        assert_eq!(
            Skeleton::<Name>::of("---"),
            Err(SkeletonError::FoldsToNothing)
        );
        assert_eq!(Skeleton::<Name>::of(""), Err(SkeletonError::FoldsToNothing));
    }

    #[test]
    fn a_profile_with_no_folds_still_folds_confusables() {
        let cyrillic = Skeleton::<NoFolds>::of("\u{0430}cme").expect("folds");
        let latin = Skeleton::<NoFolds>::of("acme").expect("folds");
        assert_eq!(cyrillic, latin);
    }

    #[test]
    fn a_profile_that_keeps_a_separator_keeps_the_names_apart() {
        let with = Skeleton::<NoFolds>::of("a-b").expect("folds");
        let without = Skeleton::<NoFolds>::of("ab").expect("folds");
        assert_ne!(with, without);
    }

    #[test]
    fn the_longest_matching_fold_wins() {
        assert_eq!(skeleton("cl"), "d");
        assert_eq!(skeleton("c1"), "d");
    }

    /// `Chained` has no character UTS #39 maps, so the chain is carried by the profile's folds alone.
    #[test]
    fn a_fold_that_creates_another_folds_source_still_converges() {
        struct Chained;
        impl SkeletonProfile for Chained {
            const NAME: &'static str = "test chained";
            const MAX_BYTES: usize = 64;
            const REMOVED: &'static [char] = &[];
            const CASE: CaseFold = CaseFold::None;
            const FOLDS: &'static [Fold] = &[Fold::new("1", "l"), Fold::new("cl", "d")];
        }

        let once = Skeleton::<Chained>::of("c1ropbox").expect("folds");
        assert_eq!(once.as_str(), "dropbox");

        let twice = Skeleton::<Chained>::of(once.as_str()).expect("folds");
        assert_eq!(twice, once, "folding a skeleton must change nothing");
    }

    #[test]
    fn folds_that_cycle_are_refused() {
        struct Cycling;
        impl SkeletonProfile for Cycling {
            const NAME: &'static str = "test cycling";
            const MAX_BYTES: usize = 64;
            const REMOVED: &'static [char] = &[];
            const CASE: CaseFold = CaseFold::None;
            const FOLDS: &'static [Fold] = &[Fold::new("a", "b"), Fold::new("b", "a")];
        }

        let refusal = Skeleton::<Cycling>::of("ab").expect_err("must not settle");
        assert!(
            matches!(
                refusal,
                SkeletonError::FoldsDoNotConverge {
                    profile: "test cycling",
                    ..
                }
            ),
            "{refusal:?}"
        );
    }

    #[test]
    fn debug_names_the_profile_and_never_the_input() {
        let value = Skeleton::<Name>::of("acrne").expect("folds");
        let shown = format!("{value:?}");
        assert!(shown.contains("test name"), "{shown}");
        assert!(shown.contains("acme"), "{shown}");
        assert!(!shown.contains("acrne"), "{shown}");
    }
}
