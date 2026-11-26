use serde::Deserialize;

/// Whether this surface's responses may appear in a search engine or a crawler's corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Indexing {
    /// Indexable. No `X-Robots-Tag` is sent, so a page's own `noindex` still applies.
    #[default]
    Public,
    /// Never indexed: `X-Robots-Tag: noindex, nofollow` on every response, assets included.
    Private,
}
