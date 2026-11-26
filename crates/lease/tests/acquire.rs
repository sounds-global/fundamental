//! Taking, keeping, losing and releasing a lease. Requires Docker.

mod support;

use fundamental_core::Error as _;
use fundamental_db::KeyedRepository as _;
use fundamental_lease::{Acquisition, HeldLease, LeaseRow, LeaseState, Released};
use futures::future::join_all;
use support::{PAST_TTL, lease, schema_with_leases};

fn held(acquisition: Acquisition) -> HeldLease {
    match acquisition {
        Acquisition::Held(proof) => proof,
        Acquisition::HeldElsewhere { epoch, .. } => panic!("held elsewhere at epoch {epoch}"),
    }
}

#[tokio::test]
async fn the_first_acquirer_takes_the_lease_at_a_new_epoch() {
    let schema = schema_with_leases().await;
    let alpha = lease(schema.pool(), "keeper");
    let proof = held(alpha.try_acquire().await.expect("an answer"));
    assert!(proof.began());
    assert!(proof.epoch() >= 1);
    assert_eq!(proof.handover_gap(), None, "nobody held it before");
}

#[tokio::test]
async fn a_second_instance_does_not_get_a_live_lease() {
    let schema = schema_with_leases().await;
    let alpha = lease(schema.pool(), "keeper");
    let beta = lease(schema.pool(), "keeper");
    let proof = held(alpha.try_acquire().await.expect("an answer"));
    match beta.try_acquire().await.expect("an answer") {
        Acquisition::HeldElsewhere { epoch, .. } => assert_eq!(epoch, proof.epoch()),
        Acquisition::Held(_) => panic!("two holders of one live lease"),
    }
}

#[tokio::test]
async fn losing_the_lease_is_not_an_error() {
    let schema = schema_with_leases().await;
    held(
        lease(schema.pool(), "keeper")
            .try_acquire()
            .await
            .expect("an answer"),
    );
    let outcome = lease(schema.pool(), "keeper").try_acquire().await;
    assert!(
        matches!(outcome, Ok(Acquisition::HeldElsewhere { .. })),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn the_holder_renews_and_keeps_the_same_epoch() {
    let schema = schema_with_leases().await;
    let alpha = lease(schema.pool(), "keeper");
    let first = held(alpha.try_acquire().await.expect("an answer"));
    let second = held(alpha.try_acquire().await.expect("an answer"));
    assert!(!second.began(), "a renewal is not a new tenure");
    assert_eq!(
        second.epoch(),
        first.epoch(),
        "renewing never moves the fencing token"
    );
    assert!(second.expires_at() > first.expires_at());
}

#[tokio::test]
async fn an_expired_lease_is_taken_over_at_the_next_epoch() {
    let schema = schema_with_leases().await;
    let alpha = lease(schema.pool(), "keeper");
    let beta = lease(schema.pool(), "keeper");
    let first = held(alpha.try_acquire().await.expect("an answer"));
    tokio::time::sleep(PAST_TTL).await;
    let second = held(beta.try_acquire().await.expect("an answer"));
    assert!(second.began());
    assert!(second.epoch() > first.epoch());
    assert!(
        second.handover_gap().is_some(),
        "a takeover measures how long nobody held it"
    );
}

#[tokio::test]
async fn a_paused_holder_cannot_renew_a_lease_taken_over() {
    let schema = schema_with_leases().await;
    let alpha = lease(schema.pool(), "keeper");
    let beta = lease(schema.pool(), "keeper");
    held(alpha.try_acquire().await.expect("an answer"));
    tokio::time::sleep(PAST_TTL).await;
    let taken = held(beta.try_acquire().await.expect("an answer"));
    match alpha.try_acquire().await.expect("an answer") {
        Acquisition::HeldElsewhere { epoch, .. } => assert_eq!(epoch, taken.epoch()),
        Acquisition::Held(_) => panic!("a paused holder took back a lease another holds"),
    }
}

#[tokio::test]
async fn only_one_of_many_racing_instances_wins_an_absent_lease() {
    let schema = schema_with_leases().await;
    let racers: Vec<_> = (0..16).map(|_| lease(schema.pool(), "keeper")).collect();
    let outcomes = join_all(racers.iter().map(|racer| racer.try_acquire())).await;
    let winners = outcomes
        .into_iter()
        .map(|outcome| outcome.expect("every racer gets an answer"))
        .filter(|outcome| matches!(outcome, Acquisition::Held(_)))
        .count();
    assert_eq!(winners, 1);
}

#[tokio::test]
async fn only_one_of_many_racing_instances_wins_an_expired_lease() {
    let schema = schema_with_leases().await;
    held(
        lease(schema.pool(), "keeper")
            .try_acquire()
            .await
            .expect("an answer"),
    );
    tokio::time::sleep(PAST_TTL).await;
    let racers: Vec<_> = (0..16).map(|_| lease(schema.pool(), "keeper")).collect();
    let outcomes = join_all(racers.iter().map(|racer| racer.try_acquire())).await;
    let winners = outcomes
        .into_iter()
        .map(|outcome| outcome.expect("every racer gets an answer"))
        .filter(|outcome| matches!(outcome, Acquisition::Held(_)))
        .count();
    assert_eq!(winners, 1);
}

#[tokio::test]
async fn an_unreachable_store_reports_slease_001_and_is_retryable() {
    let schema = schema_with_leases().await;
    let alpha = lease(schema.pool(), "keeper");
    schema.pool().inner().close().await;
    let error = alpha.try_acquire().await.expect_err("the store is gone");
    assert_eq!(error.error_code(), "SLEASE_001", "{error:?}");
    assert!(
        error.is_retryable(),
        "a blind holder tries again next renewal"
    );
}

#[tokio::test]
async fn a_released_lease_is_taken_at_the_next_epoch() {
    let schema = schema_with_leases().await;
    let alpha = lease(schema.pool(), "keeper");
    let beta = lease(schema.pool(), "keeper");
    let first = held(alpha.try_acquire().await.expect("an answer"));
    assert_eq!(
        alpha.release().await.expect("released"),
        Released::Vacated {
            epoch: first.epoch()
        }
    );
    let second = held(
        beta.try_acquire()
            .await
            .expect("taken at once, no TTL to wait"),
    );
    assert!(second.epoch() > first.epoch());
}

#[tokio::test]
async fn release_by_a_superseded_holder_changes_nothing() {
    let schema = schema_with_leases().await;
    let alpha = lease(schema.pool(), "keeper");
    let beta = lease(schema.pool(), "keeper");
    held(alpha.try_acquire().await.expect("an answer"));
    tokio::time::sleep(PAST_TTL).await;
    let taken = held(beta.try_acquire().await.expect("an answer"));
    assert_eq!(alpha.release().await.expect("an answer"), Released::NotHeld);
    let (state, _) = beta.current().await.expect("an answer");
    assert!(
        matches!(state, Some(LeaseState::Held { epoch, .. }) if epoch == taken.epoch()),
        "{state:?}"
    );
}

#[tokio::test]
async fn the_epoch_never_repeats_after_the_row_is_deleted() {
    let schema = schema_with_leases().await;
    let alpha = lease(schema.pool(), "keeper");
    let first = held(alpha.try_acquire().await.expect("an answer"));
    alpha.release().await.expect("released");
    let removed = schema
        .pool()
        .repository::<LeaseRow>()
        .delete(&"keeper".to_owned())
        .await
        .expect("the row is deleted");
    assert!(removed);
    let beta = lease(schema.pool(), "keeper");
    let second = held(beta.try_acquire().await.expect("an answer"));
    assert!(
        second.epoch() > first.epoch(),
        "a deleted row must not reissue epoch {}: got {}",
        first.epoch(),
        second.epoch()
    );
}
