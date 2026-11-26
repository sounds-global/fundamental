use core::future::Future;

use fundamental_server::AuthorizedObjects;

use super::granted::Granted;
use super::load_cx::LoadCx;
use super::page_error::PageError;
use super::page_refusal::PageRefusal;
use super::requirement::Requirement;

/// A function that loads a page's model on the server, with up to four [`Granted`] arguments.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a page loader",
    note = "a loader is `async fn load(cx: LoadCx<PageState>, ..) -> Result<Model, PageRefusal>`, taking up to four `Authorized<N, R>` arguments after the context, each with a named relation"
)]
pub trait Loader<S, Args>: Send + Sync + 'static {
    /// What the loader produces.
    type Model: Send + 'static;

    /// The objects the loader takes, read from its argument types.
    fn requirements() -> Vec<Requirement>;

    /// Runs the loader for one request.
    fn load(
        &self,
        cx: LoadCx<S>,
        objects: &AuthorizedObjects,
    ) -> impl Future<Output = Result<Self::Model, PageRefusal>> + Send;
}

/// The object a granted argument needs, taken now, before the loader's future is built.
fn take<A: Granted>(objects: &AuthorizedObjects) -> Result<A, PageRefusal> {
    A::take(objects).ok_or_else(|| {
        let required = A::requirement();
        PageRefusal::from(PageError::ObjectNotGranted {
            namespace: required.namespace,
            relation: required.relation,
        })
    })
}

impl<S, F, Fut, M> Loader<S, ()> for F
where
    S: Send + Sync + 'static,
    F: Fn(LoadCx<S>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<M, PageRefusal>> + Send,
    M: Send + 'static,
{
    type Model = M;

    fn requirements() -> Vec<Requirement> {
        Vec::new()
    }

    fn load(
        &self,
        cx: LoadCx<S>,
        _objects: &AuthorizedObjects,
    ) -> impl Future<Output = Result<M, PageRefusal>> + Send {
        self(cx)
    }
}

macro_rules! loader_with_objects {
    ($($arg:ident),+) => {
        impl<S, F, Fut, M, $($arg),+> Loader<S, ($($arg,)+)> for F
        where
            S: Send + Sync + 'static,
            F: Fn(LoadCx<S>, $($arg),+) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = Result<M, PageRefusal>> + Send,
            M: Send + 'static,
            $($arg: Granted),+
        {
            type Model = M;

            fn requirements() -> Vec<Requirement> {
                vec![$($arg::requirement()),+]
            }

            #[expect(non_snake_case, reason = "each argument is named after its type parameter")]
            fn load(
                &self,
                cx: LoadCx<S>,
                objects: &AuthorizedObjects,
            ) -> impl Future<Output = Result<M, PageRefusal>> + Send {
                let taken = (|| Ok::<_, PageRefusal>(($(take::<$arg>(objects)?,)+)))();
                let called = taken.map(|($($arg,)+)| self(cx, $($arg),+));
                async move { called?.await }
            }
        }
    };
}

loader_with_objects!(A1);
loader_with_objects!(A1, A2);
loader_with_objects!(A1, A2, A3);
loader_with_objects!(A1, A2, A3, A4);

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fundamental_authz::middleware::Authorized;
    use fundamental_authz::schema::{Namespace, Relation, RelationOf};
    use fundamental_server::{AuthorizedObjects, HttpContext};

    use super::Loader;
    use crate::page::load_cx::LoadCx;
    use crate::page::page_refusal::PageRefusal;
    use crate::page::public_params::PublicParams;

    struct Team;

    impl Relation for Team {
        fn relation_name(&self) -> &'static str {
            "member"
        }
    }

    impl Namespace for Team {
        const NAME: &'static str = "team";
        type Relation = Self;

        fn ir() -> fundamental_authz::schema::NamespaceIr {
            fundamental_authz::schema::NamespaceIr::new(
                fundamental_authz::schema::NamespaceName::new("team").expect("a name"),
                Vec::new(),
            )
        }
    }

    struct Member;

    impl RelationOf<Team> for Member {
        const RELATION: Team = Team;
    }

    async fn load(_cx: LoadCx<()>, team: Authorized<Team, Member>) -> Result<String, PageRefusal> {
        Ok(team.id().to_owned())
    }

    fn requirements<L: Loader<(), A>, A>(_loader: &L) -> Vec<super::Requirement> {
        L::requirements()
    }

    fn cx() -> LoadCx<()> {
        LoadCx::new(
            HttpContext::new((), "req"),
            Arc::new(PublicParams::default()),
        )
    }

    #[test]
    fn a_loaders_requirements_are_its_argument_types() {
        let required = requirements(&load);
        assert_eq!(required.len(), 1);
        assert_eq!(
            (required[0].namespace, required[0].relation),
            ("team", "member")
        );
    }

    #[tokio::test]
    async fn an_object_no_guard_granted_is_never_handed_to_a_loader() {
        let refused = Loader::load(&load, cx(), &AuthorizedObjects::default())
            .await
            .expect_err("nothing was granted");
        let PageRefusal::Failed(error) = refused else {
            panic!("a failure, not a refusal about the object");
        };
        assert_eq!(error.get().error_code(), "FW_PAGE_015");

        let granted = AuthorizedObjects::default();
        granted.record("team", "member", "t1");
        let loaded = Loader::load(&load, cx(), &granted).await.expect("granted");
        assert_eq!(loaded, "t1");
    }
}
