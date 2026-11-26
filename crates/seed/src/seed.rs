use std::fmt;
use std::future::Future;
use std::pin::Pin;

use crate::SeedError;

/// The boxed async function a [`Seed`] runs, over an application context `C`.
pub type SeedFn<C> = fn(&C) -> Pin<Box<dyn Future<Output = Result<(), SeedError>> + Send + '_>>;

/// A registered seed: a name, its ordering metadata, and the function to run.
#[must_use]
pub struct Seed<C> {
    name: &'static str,
    order: u32,
    after: Option<&'static str>,
    run_fn: SeedFn<C>,
}

impl<C> Seed<C> {
    /// Creates a seed; `after` names a seed that must run first, and `order` ranks independent seeds.
    pub fn new(
        name: &'static str,
        order: u32,
        after: Option<&'static str>,
        run_fn: SeedFn<C>,
    ) -> Self {
        Self {
            name,
            order,
            after,
            run_fn,
        }
    }

    /// The seed's name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The seed's run order (lower runs first among independent seeds).
    #[must_use]
    pub fn order(&self) -> u32 {
        self.order
    }

    /// The name of the seed this one must run after, if any.
    #[must_use]
    pub fn after(&self) -> Option<&'static str> {
        self.after
    }

    pub(crate) fn run<'a>(
        &self,
        ctx: &'a C,
    ) -> Pin<Box<dyn Future<Output = Result<(), SeedError>> + Send + 'a>> {
        (self.run_fn)(ctx)
    }
}

impl<C> fmt::Debug for Seed<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Seed")
            .field("name", &self.name)
            .field("order", &self.order)
            .field("after", &self.after)
            .field("run_fn", &"<fn>")
            .finish()
    }
}
