use core::future::Future;

use fundamental_server::AuthorizedObjects;

use super::load_cx::LoadCx;
use super::loader::Loader;
use super::page_refusal::PageRefusal;
use super::requirement::Requirement;
use super::sourced::Sourced;

/// The loaders of one route as a tuple, run concurrently; the first refusal answers.
pub trait Loads<S, Args>: Send + Sync + 'static {
    /// The loaded values, as a tuple in the loaders' order.
    type Models: Send + 'static;

    /// Every object each loader takes, with the file it is declared in.
    fn requirements(&self) -> Vec<(&'static str, Requirement)>;

    /// Runs every loader for one request.
    fn load(
        &self,
        cx: &LoadCx<S>,
        objects: &AuthorizedObjects,
    ) -> impl Future<Output = Result<Self::Models, PageRefusal>> + Send;
}

macro_rules! loads {
    ($($loader:ident $args:ident $index:tt),+) => {
        impl<S, $($loader, $args),+> Loads<S, ($($args,)+)> for ($(Sourced<$loader>,)+)
        where
            S: Clone + Send + Sync + 'static,
            $($loader: Loader<S, $args>, $args: 'static),+
        {
            type Models = ($($loader::Model,)+);

            fn requirements(&self) -> Vec<(&'static str, Requirement)> {
                let mut all = Vec::new();
                $(
                    all.extend(
                        <$loader as Loader<S, $args>>::requirements()
                            .into_iter()
                            .map(|required| (self.$index.file, required)),
                    );
                )+
                all
            }

            fn load(
                &self,
                cx: &LoadCx<S>,
                objects: &AuthorizedObjects,
            ) -> impl Future<Output = Result<Self::Models, PageRefusal>> + Send {
                let futures = ($(self.$index.loader.load(cx.clone(), objects),)+);
                async move {
                    #[expect(non_snake_case, reason = "each future is named after its loader type")]
                    let ($($loader,)+) = futures;
                    tokio::try_join!($($loader),+)
                }
            }
        }
    };
}

loads!(L0 A0 0);
loads!(L0 A0 0, L1 A1 1);
loads!(L0 A0 0, L1 A1 1, L2 A2 2);
loads!(L0 A0 0, L1 A1 1, L2 A2 2, L3 A3 3);
loads!(L0 A0 0, L1 A1 1, L2 A2 2, L3 A3 3, L4 A4 4);
