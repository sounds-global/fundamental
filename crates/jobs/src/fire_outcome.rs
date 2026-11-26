/// What one attempt to fire a claimed job found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireOutcome {
    /// The handler ran and its effect committed with the job's fired mark.
    Fired,
    /// The handler moved the job to a later due time ([`Fire::NotBefore`](crate::Fire::NotBefore)).
    Postponed,
    /// The handler could not decide yet ([`Fire::Defer`](crate::Fire::Defer)).
    Deferred,
    /// The occurrence had already fired.
    AlreadyFired,
    /// The job moved on since it was claimed: rescheduled, or claimed again after this lease
    /// lapsed.
    Superseded,
    /// The job was cancelled since it was claimed.
    Cancelled,
    /// By the database's clock the job is not due yet; it waits for its due time again.
    NotYetDue,
}

impl FireOutcome {
    /// The outcome's name in logs.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fired => "fired",
            Self::Postponed => "postponed",
            Self::Deferred => "deferred",
            Self::AlreadyFired => "already_fired",
            Self::Superseded => "superseded",
            Self::Cancelled => "cancelled",
            Self::NotYetDue => "not_yet_due",
        }
    }
}
