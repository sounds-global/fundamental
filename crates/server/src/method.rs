use std::fmt;
use std::str::FromStr;

use crate::ServerError;

/// An HTTP request method.
///
/// `HEAD` is absent: the backend routes it as a `GET`, so the two cannot disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Method {
    /// `GET` — retrieve a resource.
    Get,
    /// `POST` — create or submit.
    Post,
    /// `PUT` — replace a resource.
    Put,
    /// `PATCH` — partially update a resource.
    Patch,
    /// `DELETE` — remove a resource.
    Delete,
}

impl Method {
    /// Returns the uppercase wire token for this method (for example `"GET"`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }

    /// Whether the method is safe (RFC 9110 §9.2.1); `GET` is the whole set here.
    #[must_use]
    pub fn is_safe(self) -> bool {
        matches!(self, Method::Get)
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Method {
    type Err = ServerError;

    /// Parses a method token, accepting any letter case.
    ///
    /// # Errors
    /// Returns [`ServerError::UnsupportedMethod`] if `s` is not one of the
    /// supported methods.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_uppercase().as_str() {
            "GET" => Ok(Method::Get),
            "POST" => Ok(Method::Post),
            "PUT" => Ok(Method::Put),
            "PATCH" => Ok(Method::Patch),
            "DELETE" => Ok(Method::Delete),
            _ => Err(ServerError::UnsupportedMethod),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_str() {
        for method in [
            Method::Get,
            Method::Post,
            Method::Put,
            Method::Patch,
            Method::Delete,
        ] {
            assert_eq!(method.as_str().parse::<Method>().expect("parses"), method);
        }
    }

    #[test]
    fn parses_case_insensitively() {
        assert_eq!("get".parse::<Method>().expect("parses"), Method::Get);
        assert_eq!("Patch".parse::<Method>().expect("parses"), Method::Patch);
    }

    #[test]
    fn rejects_unknown_method_with_code() {
        use fundamental_core::Error as _;
        let err = "CONNECT".parse::<Method>().expect_err("unknown method");
        assert_eq!(err.error_code(), "API_003");
        assert!(matches!(err, ServerError::UnsupportedMethod));
        assert!(!err.to_string().contains("CONNECT"));
    }

    #[test]
    fn display_matches_wire_token() {
        assert_eq!(Method::Delete.to_string(), "DELETE");
    }

    #[test]
    fn only_get_is_safe() {
        assert!(Method::Get.is_safe());
        for method in [Method::Post, Method::Put, Method::Patch, Method::Delete] {
            assert!(!method.is_safe(), "{method}");
        }
    }
}
