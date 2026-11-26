use fundamental_authz::middleware::GuardCheck;

use super::guard_object::GuardObject;
use super::guard_spec::GuardSpec;
use super::guarded::Guarded;
use super::page_policy::PagePolicy;

/// A guard that has its relation and not yet its object.
#[must_use = "a guard checks nothing until it is told its object: call `.object(..)` or `.object_const(..)`"]
pub struct PendingGuard<G> {
    policy: PagePolicy<G>,
    check: GuardCheck,
}

impl<G> PendingGuard<G> {
    pub(crate) fn new(policy: PagePolicy<G>, check: GuardCheck) -> Self {
        Self { policy, check }
    }

    /// Checks the object the route parameter `param` names — `{team}` is `"team"`.
    pub fn object(self, param: &'static str) -> PagePolicy<Guarded> {
        self.finish(GuardObject::Param(param))
    }

    /// Checks one fixed object for every request: a role over every record.
    pub fn object_const(self, id: impl Into<String>) -> PagePolicy<Guarded> {
        self.finish(GuardObject::Const(id.into()))
    }

    fn finish(self, object: GuardObject) -> PagePolicy<Guarded> {
        self.policy.with_guard(GuardSpec {
            check: self.check,
            object,
        })
    }
}
