use std::fmt;

/// The listener of an app an origin is routed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OriginSurface {
    /// `[apps.<app>.web]`: the server-rendered site.
    Web,
    /// `[apps.<app>.api]`: the HTTP API.
    Api,
}

impl OriginSurface {
    /// Parses the surface as the manifest names it, `web` or `api`.
    ///
    /// # Errors
    ///
    /// A sentence naming the value, for [`LocalError::OriginsInvalid`].
    ///
    /// [`LocalError::OriginsInvalid`]: crate::LocalError::OriginsInvalid
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "web" => Ok(Self::Web),
            "api" => Ok(Self::Api),
            other => Err(format!(
                "surface `{other}` cannot be behind the local edge: an origin routes to `web` or `api`"
            )),
        }
    }

    /// The surface's key in the manifest and in the configuration's override namespace.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Api => "api",
        }
    }
}

impl fmt::Display for OriginSurface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

#[cfg(test)]
mod tests {
    use super::OriginSurface;

    #[test]
    fn only_web_and_api_can_be_behind_the_edge() {
        assert_eq!(OriginSurface::parse("web"), Ok(OriginSurface::Web));
        assert_eq!(OriginSurface::parse("api"), Ok(OriginSurface::Api));
        for refused in ["sync", "socket", "admin", "Web", ""] {
            assert!(OriginSurface::parse(refused).is_err(), "{refused:?}");
        }
    }
}
