//! A test's turn for DDL on the shared node: the next one waits until it drops.

use std::time::Duration;

use fundamental_test_containers::sql::DdlTurn;

#[tokio::test]
async fn the_next_turn_waits_until_the_one_before_it_drops() {
    let first = DdlTurn::take().await.expect("the first turn");
    let waiting = tokio::spawn(DdlTurn::take());
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !waiting.is_finished(),
        "a second turn was taken while the first was held"
    );
    drop(first);
    let second = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .expect("the second turn follows the first")
        .expect("the task ran");
    assert!(second.is_ok());
}
