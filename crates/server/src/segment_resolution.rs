use crate::ResolvedSegment;

/// What a [`SegmentResolver`](crate::SegmentResolver) found a path segment to name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentResolution {
    /// The segment is its record's canonical spelling.
    Canonical(ResolvedSegment),
    /// The segment names its record in another spelling; served in place until authorization affirms it.
    Moved(ResolvedSegment),
    /// Nothing this resolver may answer for; the route sees a void id.
    Unknown,
}
