use fundamental_core::secrets::SecretString;

/// The credential a boundary read off a request, tagged with the place it came from.
#[derive(Debug, Clone)]
pub enum PresentedCredential {
    /// Read from the `Authorization: Bearer` header.
    BearerHeader(SecretString),
    /// Read from a named cookie.
    Cookie(SecretString),
}

impl PresentedCredential {
    /// The token itself, wherever it was read from.
    #[must_use]
    pub fn token(&self) -> &SecretString {
        match self {
            Self::BearerHeader(token) | Self::Cookie(token) => token,
        }
    }

    /// Whether the credential rode in on a cookie, carrying a browser's ambient authority.
    #[must_use]
    pub fn is_cookie(&self) -> bool {
        matches!(self, Self::Cookie(_))
    }

    /// A stable label for logs and spans: the **place**, never the token.
    #[must_use]
    pub fn source_label(&self) -> &'static str {
        match self {
            Self::BearerHeader(_) => "bearer_header",
            Self::Cookie(_) => "cookie",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_is_readable_from_either_place() {
        let bearer = PresentedCredential::BearerHeader(SecretString::new("t".to_owned()));
        let cookie = PresentedCredential::Cookie(SecretString::new("t".to_owned()));
        assert_eq!(bearer.token().expose(), "t");
        assert_eq!(cookie.token().expose(), "t");
    }

    #[test]
    fn only_a_cookie_reports_itself_as_one() {
        assert!(PresentedCredential::Cookie(SecretString::new("t".to_owned())).is_cookie());
        assert!(!PresentedCredential::BearerHeader(SecretString::new("t".to_owned())).is_cookie());
    }

    #[test]
    fn the_labels_are_stable() {
        assert_eq!(
            PresentedCredential::BearerHeader(SecretString::new("t".to_owned())).source_label(),
            "bearer_header"
        );
        assert_eq!(
            PresentedCredential::Cookie(SecretString::new("t".to_owned())).source_label(),
            "cookie"
        );
    }

    /// The label is for logs, so the thing it must never do is carry the token.
    #[test]
    fn debug_redacts_the_token() {
        let credential = PresentedCredential::Cookie(SecretString::new("super-secret".to_owned()));
        let rendered = format!("{credential:?}");
        assert!(!rendered.contains("super-secret"), "{rendered}");
    }
}
