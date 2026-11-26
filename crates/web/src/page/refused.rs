use fundamental_core::error::ShownError;

use super::gate_reason::GateReason;

/// The refusal a gate page answers: its status, and what the reader can do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused {
    status: u16,
    reason: GateReason,
}

/// The code a loader's step-up refusal carries (`PageError::StepUpRequired`).
const STEP_UP: &str = "FW_PAGE_012";

impl Refused {
    /// A refusal with `status` for `reason`.
    #[must_use]
    pub fn new(status: u16, reason: GateReason) -> Self {
        Self { status, reason }
    }

    /// The refusal a navigation ended in, as the router shows it to its gate page.
    #[must_use]
    pub fn from_shown(shown: ShownError) -> Self {
        let reason = match (shown.code(), shown.status()) {
            (STEP_UP, _) => GateReason::StepUp,
            (_, 401) => GateReason::SignIn,
            _ => GateReason::Forbidden,
        };
        Self::new(shown.status(), reason)
    }

    /// The response status, `401` or `403`.
    #[must_use]
    pub fn status(&self) -> u16 {
        self.status
    }

    /// What the reader can do about it.
    #[must_use]
    pub fn reason(&self) -> GateReason {
        self.reason
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::{GateReason, Refused};
    use crate::page::PageError;
    use fundamental_core::Error as _;
    use fundamental_core::error::ShownError;
    use fundamental_server::ServerError;

    #[test]
    fn each_refusal_tells_the_gate_what_the_reader_can_do() {
        let step_up = Refused::from_shown(ShownError::of(&PageError::StepUpRequired));
        assert_eq!(
            (step_up.status(), step_up.reason()),
            (401, GateReason::StepUp)
        );

        let sign_in = Refused::from_shown(ShownError::of(&PageError::SignInRequired));
        assert_eq!(
            (sign_in.status(), sign_in.reason()),
            (401, GateReason::SignIn)
        );

        let anonymous = Refused::from_shown(ShownError::of(&ServerError::unauthenticated()));
        assert_eq!(anonymous.reason(), GateReason::SignIn);

        let forbidden = Refused::from_shown(ShownError::of(
            &fundamental_authz::middleware::Forbidden::Denied,
        ));
        assert_eq!(
            (forbidden.status(), forbidden.reason()),
            (403, GateReason::Forbidden)
        );
    }
}
