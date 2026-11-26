//! [`Tab`] — a tab of a team, a public segment.

use fundamental::PublicSegment;

/// A tab of a team's pages.
#[derive(Clone, Copy, PublicSegment)]
pub enum Tab {
    /// The overview tab.
    Overview,
    /// The files tab.
    #[segment("bestanden")]
    Files,
}
