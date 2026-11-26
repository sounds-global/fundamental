use fundamental::web::{GuardedPolicy, PagePolicy};

use crate::Tab;
use crate::team::Team;

pub fn policy() -> GuardedPolicy {
    PagePolicy::new()
        .signed_in()
        .authorize(Team::Member)
        .object("team")
        .public_segment::<Tab>("tab")
}
