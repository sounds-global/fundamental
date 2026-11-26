//! [`eventually`] — wait for a condition until a deadline, never for a fixed time.

use core::future::Future;
use core::time::Duration;
use std::time::Instant;

use tracing::{debug, warn};

use super::{Backoff, NeverHeld};

/// How often a condition is asked: 10 ms first, doubling to 250 ms. A window, not a jittered
/// draw: one waiter has no fleet to spread, and a draw of zero would spin.
const ASKING: Backoff =
    match Backoff::new(Duration::from_millis(10), 2.0, Duration::from_millis(250)) {
        Ok(schedule) => schedule,
        Err(_) => panic!("the asking schedule is a valid backoff"),
    };

/// Asks `probe` until it answers `Some`, or until `within` has passed, and returns the answer.
///
/// ```
/// use core::time::Duration;
/// use fundamental_core::retry::eventually;
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mut asked = 0;
/// let third = eventually("the third ask", Duration::from_secs(5), tokio::time::sleep, || {
///     asked += 1;
///     let now = asked;
///     async move { (now == 3).then_some(now) }
/// })
/// .await?;
/// assert_eq!(third, 3);
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// [`NeverHeld`], naming `what`, when `probe` never answered `Some` within `within`.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub async fn eventually<T, Probe, Sleep, Slept>(
    what: &'static str,
    within: Duration,
    mut sleep: Sleep,
    mut probe: Probe,
) -> Result<T, NeverHeld>
where
    Probe: AsyncFnMut() -> Option<T>,
    Sleep: FnMut(Duration) -> Slept,
    Slept: Future<Output = ()>,
{
    let started = Instant::now();
    let mut asked: u32 = 0;
    loop {
        asked = asked.saturating_add(1);
        if let Some(answer) = probe().await {
            debug!(
                what,
                asked,
                waited_ms = millis(started.elapsed()),
                "the condition held"
            );
            return Ok(answer);
        }
        let left = within.saturating_sub(started.elapsed());
        if left.is_zero() {
            warn!(
                what,
                asked,
                within_ms = millis(within),
                "the condition never held"
            );
            return Err(NeverHeld::new(what, within, asked));
        }
        sleep(ASKING.window(asked - 1).min(left)).await;
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[tokio::test(flavor = "current_thread")]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn a_condition_that_holds_at_once_is_asked_once_and_never_waits() {
        let waits = RefCell::new(Vec::new());
        let answer = eventually(
            "at once",
            Duration::from_secs(5),
            |wait| {
                waits.borrow_mut().push(wait);
                async {}
            },
            || async { Some(7) },
        )
        .await;
        assert_eq!(answer, Ok(7));
        assert!(waits.borrow().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn the_waits_between_asks_grow_from_the_first_window() {
        let waits = RefCell::new(Vec::new());
        let mut asked = 0;
        let answer = eventually(
            "the fourth ask",
            Duration::from_secs(5),
            |wait| {
                waits.borrow_mut().push(wait);
                async {}
            },
            || {
                asked += 1;
                let now = asked;
                async move { (now == 4).then_some(now) }
            },
        )
        .await;
        assert_eq!(answer, Ok(4));
        assert_eq!(
            *waits.borrow(),
            [10, 20, 40].map(Duration::from_millis).to_vec()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_probe_may_hold_a_mutable_borrow_across_its_own_await() {
        let mut seen = Vec::new();
        let answer = eventually(
            "the second ask",
            Duration::from_secs(5),
            |_| async {},
            async || {
                seen.push(seen.len());
                tokio::task::yield_now().await;
                (seen.len() == 2).then(|| seen.clone())
            },
        )
        .await;
        assert_eq!(answer, Ok(vec![0, 1]));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_condition_that_never_holds_is_named_after_its_deadline() {
        let within = Duration::from_millis(60);
        let started = Instant::now();
        let waits = RefCell::new(Vec::new());
        let never = eventually(
            "the sealer sealed",
            within,
            |wait| {
                waits.borrow_mut().push(wait);
                tokio::time::sleep(wait)
            },
            || async { None::<()> },
        )
        .await
        .unwrap_err();
        assert!(started.elapsed() >= within);
        assert_eq!(never.what(), "the sealer sealed");
        assert_eq!(never.within(), within);
        assert_eq!(
            never.asked(),
            u32::try_from(waits.borrow().len()).unwrap() + 1
        );
        assert!(waits.borrow().iter().all(|wait| *wait <= within));
    }
}
