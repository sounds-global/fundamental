//! Confusable skeletons: what a name *looks like*, rather than what it spells.
//!
//! ```
//! use fundamental_text::{CaseFold, Fold, Skeleton, SkeletonProfile};
//!
//! struct BrandName;
//! impl SkeletonProfile for BrandName {
//!     const NAME: &'static str = "brand name";
//!     const MAX_BYTES: usize = 253;
//!     const REMOVED: &'static [char] = &['-'];
//!     const CASE: CaseFold = CaseFold::Lowercase;
//!     const FOLDS: &'static [Fold] = &[Fold::new("rn", "m"), Fold::new("0", "o")];
//! }
//!
//! // A look-alike and a homoglyph reach the name they imitate.
//! let real = Skeleton::<BrandName>::of("acme")?;
//! assert_eq!(Skeleton::<BrandName>::of("acrne")?, real);
//! assert_eq!(Skeleton::<BrandName>::of("\u{0430}cme")?, real);
//! # Ok::<(), fundamental_text::SkeletonError>(())
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod case_fold;
mod fold;
mod profile_fingerprint;
mod skeleton;
mod skeleton_error;
mod skeleton_profile;
mod unicode_tables;

pub use case_fold::CaseFold;
pub use fold::Fold;
pub use profile_fingerprint::ProfileFingerprint;
pub use skeleton::Skeleton;
pub use skeleton_error::SkeletonError;
pub use skeleton_profile::SkeletonProfile;
pub use unicode_tables::UnicodeTables;
