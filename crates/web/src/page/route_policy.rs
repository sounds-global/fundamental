use std::sync::Arc;

use fundamental_server::template_params;

use super::cache::Cache;
use super::guard_object::GuardObject;
use super::guard_spec::GuardSpec;
use super::page_policy_error::PagePolicyError;
use super::page_title::PageTitle;
use super::policy_chain::PolicyChain;
use super::public_params::PublicParams;
use super::requirement::Requirement;

/// Everything enforced on one route, composed once at boot from its chain.
pub(crate) struct RoutePolicy {
    pub(crate) route: String,
    pub(crate) signed_in: bool,
    pub(crate) guards: Vec<GuardSpec>,
    pub(crate) public: Arc<PublicParams>,
    pub(crate) cache: Cache,
    pub(crate) title: Option<PageTitle>,
}

impl RoutePolicy {
    /// Composes `chain` for the route `template`, refusing a chain that cannot be served.
    ///
    /// # Errors
    /// `FW_PAGE_002`–`FW_PAGE_005`, the first one found.
    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub(crate) fn compose<G>(
        template: &str,
        chain: PolicyChain<G>,
    ) -> Result<Self, PagePolicyError> {
        let params = template_params(template);
        let route = template.to_owned();
        let mut composed = Self {
            route: route.clone(),
            signed_in: false,
            guards: Vec::new(),
            public: Arc::new(PublicParams::default()),
            cache: Cache::NoStore,
            title: None,
        };
        let mut public = PublicParams::default();
        let mut declared_cache: Option<Cache> = None;

        for layer in chain.layers {
            for guard in layer.guards() {
                if let GuardObject::Param(param) = guard.object
                    && !params.iter().any(|name| name == param)
                {
                    return Err(PagePolicyError::GuardOnAbsentParam {
                        route,
                        file: layer.file,
                        param,
                    });
                }
            }
            let policy = layer.policy;
            composed.signed_in |= policy.signed_in;
            for guard in policy.guards {
                if !composed.guards.iter().any(|known| known.same_as(&guard)) {
                    composed.guards.push(guard);
                }
            }
            public.raw.extend(policy.public_params);
            public.segments.extend(policy.public_segments);
            if let Some(cache) = policy.cache {
                declared_cache = Some(declared_cache.map_or(cache, |known| known.stricter(cache)));
            }
            if policy.title.is_some() {
                composed.title = policy.title;
            }
        }

        for (index, guard) in composed.guards.iter().enumerate() {
            let namespace = guard.check.object_namespace();
            let other_object = composed.guards[..index].iter().any(|earlier| {
                earlier.check.object_namespace() == namespace && earlier.object != guard.object
            });
            if other_object {
                return Err(PagePolicyError::TwoObjectsInOneNamespace {
                    route,
                    namespace: namespace.to_owned(),
                });
            }
        }

        let guards_a_param = composed
            .guards
            .iter()
            .any(|guard| matches!(guard.object, GuardObject::Param(_)));
        if guards_a_param && let Some(param) = public.raw.first() {
            return Err(PagePolicyError::RawParamUnderObjectGuard { route, param });
        }

        for param in params {
            let guarded = composed
                .guards
                .iter()
                .any(|guard| matches!(guard.object, GuardObject::Param(named) if named == param));
            if !guarded && !public.declares(&param) {
                return Err(PagePolicyError::UnguardedParam { route, param });
            }
        }

        composed.cache = declared_cache.unwrap_or(Cache::NoStore);
        composed.public = Arc::new(public);
        Ok(composed)
    }

    /// Refuses a loader in `file` that asks for an object no guard checks at exactly that relation.
    ///
    /// # Errors
    /// `FW_PAGE_001`.
    pub(crate) fn grants(
        &self,
        file: &'static str,
        required: Requirement,
    ) -> Result<(), PagePolicyError> {
        let granted = self.guards.iter().any(|guard| {
            guard.check.object_namespace() == required.namespace
                && guard.check.relation() == required.relation
        });
        if granted {
            return Ok(());
        }
        Err(PagePolicyError::LoaderOutranksGuards {
            route: self.route.clone(),
            file,
            namespace: required.namespace,
            relation: required.relation,
        })
    }

    /// Whether every answer on this route is personal: it guards an object or needs a signed-in reader.
    pub(crate) fn personal(&self) -> bool {
        self.signed_in || !self.guards.is_empty()
    }
}
