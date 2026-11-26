use fundamental_authz::middleware::GuardCheck;

use super::guard_object::GuardObject;

/// One guard: the check it asks and the object it asks it about.
#[derive(Clone)]
pub(crate) struct GuardSpec {
    pub(crate) check: GuardCheck,
    pub(crate) object: GuardObject,
}

impl GuardSpec {
    /// Whether two guards ask the same question about the same object.
    pub(crate) fn same_as(&self, other: &Self) -> bool {
        self.check.object_namespace() == other.check.object_namespace()
            && self.check.relation() == other.check.relation()
            && self.object == other.object
    }
}
