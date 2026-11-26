use std::fmt;
use std::sync::Arc;

/// A page title kept as a `Display`, so a message is said in each request's language.
#[derive(Clone)]
pub(crate) struct PageTitle(Arc<dyn fmt::Display + Send + Sync>);

impl PageTitle {
    pub(crate) fn new(title: impl fmt::Display + Send + Sync + 'static) -> Self {
        Self(Arc::new(title))
    }
}

impl fmt::Display for PageTitle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Debug for PageTitle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PageTitle")
    }
}
