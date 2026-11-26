//! [`EvalContext`]: the state shared across one check evaluation.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use tracing::error;

use super::check_path::CheckKey;
use super::{EvalLimits, Justification};
use crate::error::AuthzError;
use crate::tuple::{ReadScope, WallTime};

/// Per-request evaluation state: the read scope, the read budget, the instant caveats read
/// as `now`, and the memo of decided sub-questions.
///
/// Shared across concurrent branches, so the budget is atomic and the memo locked; both are
/// path-independent, unlike cycle detection. `now` is read once, so the memo stays sound.
pub(super) struct EvalContext<'s, K: ReadScope, J: Justification = ()> {
    limits: EvalLimits,
    scope: &'s K,
    reads_used: AtomicUsize,
    now: i64,
    memo: Mutex<HashMap<CheckKey, Option<J>>>,
}

impl<'s, K: ReadScope, J: Justification> EvalContext<'s, K, J> {
    /// Creates a fresh context for one evaluation, reading through `scope`.
    ///
    /// # Errors
    ///
    /// [`AuthzError::ImplausibleClock`] if the scope's `now` is before 2026-01-01.
    pub(super) fn new(limits: EvalLimits, scope: &'s K) -> Result<Self, AuthzError> {
        let now = plausible_now(scope.now())?;
        Ok(Self {
            limits,
            scope,
            reads_used: AtomicUsize::new(0),
            now,
            memo: Mutex::new(HashMap::new()),
        })
    }

    pub(super) fn limits(&self) -> &EvalLimits {
        &self.limits
    }

    pub(super) fn scope(&self) -> &'s K {
        self.scope
    }

    /// The instant caveats read as `now`, in whole seconds since the Unix epoch.
    pub(super) fn now(&self) -> i64 {
        self.now
    }

    /// Charges one tuple read against the budget.
    ///
    /// Bounds the total reads across all concurrent branches.
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError::BudgetExceeded`] once the read budget is exhausted.
    pub(super) fn charge_read(&self) -> Result<(), AuthzError> {
        // The read that pushes the count over the limit is rejected, so exactly `max_reads` pass.
        if self.reads_used.fetch_add(1, Ordering::Relaxed) >= self.limits.max_reads {
            return Err(AuthzError::BudgetExceeded {
                budget: self.limits.max_reads,
            });
        }
        Ok(())
    }

    pub(super) fn reads_used(&self) -> usize {
        self.reads_used.load(Ordering::Relaxed)
    }

    /// A memoized decision for `key`: `Some(None)` a denial, `Some(Some(j))` an allow.
    pub(super) fn memoized(&self, key: &CheckKey) -> Option<Option<J>> {
        self.memo
            .lock()
            .expect("eval memo mutex poisoned")
            .get(key)
            .cloned()
    }

    pub(super) fn memoize(&self, key: CheckKey, decision: Option<J>) {
        self.memo
            .lock()
            .expect("eval memo mutex poisoned")
            .insert(key, decision);
    }
}

/// The earliest instant an evaluation's `now` may be: 2026-01-01T00:00:00Z.
///
/// The epoch, which an uncommitted transaction stamps its zookie with, would make
/// `now < until` admit forever.
const EARLIEST_PLAUSIBLE_NOW: i64 = 1_767_225_600;

/// `now` in whole seconds since the epoch, once it is plausible.
///
/// # Errors
///
/// [`AuthzError::ImplausibleClock`] if `now` is before 2026-01-01.
pub(super) fn plausible_now(now: WallTime) -> Result<i64, AuthzError> {
    let now = now.unix_seconds();
    if now < EARLIEST_PLAUSIBLE_NOW {
        error!(
            now,
            floor = EARLIEST_PLAUSIBLE_NOW,
            "authorization evaluation refused: its clock is not plausible"
        );
        return Err(AuthzError::ImplausibleClock {
            unix_seconds: now,
            floor: EARLIEST_PLAUSIBLE_NOW,
        });
    }
    Ok(now)
}
