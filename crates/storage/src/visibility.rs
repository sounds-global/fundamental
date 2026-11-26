//! Who may read what a bucket holds.

/// Whether a bucket is [`Private`](crate::Private) or [`Public`](crate::Public); sealed.
pub trait Visibility: sealed::Sealed + Send + Sync + 'static {
    /// The kind's name, for a span field.
    const NAME: &'static str;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for crate::Private {}
    impl Sealed for crate::Public {}
}
