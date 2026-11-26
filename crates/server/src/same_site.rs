use std::fmt;

/// The `SameSite` attribute a [`HostCookie`](crate::HostCookie) carries; there is no `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SameSite {
    /// Sent only with requests the site itself makes.
    Strict,
    /// Also sent with a top-level navigation from another site.
    Lax,
}

impl fmt::Display for SameSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Strict => "Strict",
            Self::Lax => "Lax",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::SameSite;

    #[test]
    fn each_value_is_written_as_the_attribute_spells_it() {
        assert_eq!(SameSite::Strict.to_string(), "Strict");
        assert_eq!(SameSite::Lax.to_string(), "Lax");
    }
}
