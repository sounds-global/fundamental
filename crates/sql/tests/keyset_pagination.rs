//! A cursored read returns every row exactly once, including across a tie.

use chrono::{DateTime, TimeZone as _, Utc};
use fundamental_db::{DatabaseError, OrderBy, Repository};
use fundamental_macros::Model;
use fundamental_sql::{SqlModelQuery, SqlPool};
use fundamental_test_containers::sql::SqlSchema;
use std::collections::BTreeSet;

#[derive(Model, Debug)]
#[model(sql)]
struct Entry {
    #[model(id)]
    entry_id: i64,
    bucket: i64,
}

const ROWS: i64 = 60;
const BUCKETS: i64 = 10;
const PAGE: u64 = 7;

async fn seeded(pool: &SqlPool) {
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS entries (entry_id BIGINT PRIMARY KEY, bucket BIGINT NOT NULL)",
    )
    .await
    .expect("the entries table");
    let repo = pool.repository::<Entry>();
    for entry_id in 0..ROWS {
        repo.insert(&Entry {
            entry_id,
            // Interleaved, so the secondary ordering decides.
            bucket: entry_id % BUCKETS,
        })
        .await
        .expect("seed a row");
    }
}

/// Walks every page and collects the ids, bounded so a cursor that does not advance fails rather than hangs.
async fn page_through(pool: &SqlPool, order: Vec<OrderBy>) -> Result<Vec<i64>, DatabaseError> {
    let mut seen = Vec::new();
    let mut cursor = None;
    let max_pages = ROWS as u64 / PAGE + 2;
    for _ in 0..=max_pages {
        let mut query = SqlModelQuery::<Entry>::new(pool).page_size(PAGE);
        for order in &order {
            query = query.order_by(order.clone());
        }
        let page = query.execute_page(cursor).await?;
        seen.extend(page.items.iter().map(|entry| entry.entry_id));
        match page.next_page {
            Some(next) => cursor = Some(next),
            None => return Ok(seen),
        }
    }
    Err(DatabaseError::query(
        "the cursor never reached the end of the table; it is not advancing",
    ))
}

#[tokio::test]
async fn a_cursored_read_returns_every_row_exactly_once() {
    let Ok(schema) = SqlSchema::mint().await else {
        return;
    };
    let pool = schema.pool();
    seeded(pool).await;

    let seen = page_through(pool, vec![OrderBy::asc("entry_id")])
        .await
        .expect("page through the table");

    assert_eq!(seen.len(), ROWS as usize, "every row appears");
    assert_eq!(
        seen.iter().copied().collect::<BTreeSet<_>>().len(),
        ROWS as usize,
        "and none appears twice"
    );
}

/// Six rows share every `bucket`, so a page of seven ends inside a run.
#[tokio::test]
async fn a_tie_in_the_sort_column_does_not_lose_rows() {
    let Ok(schema) = SqlSchema::mint().await else {
        return;
    };
    let pool = schema.pool();
    seeded(pool).await;

    let seen = page_through(pool, vec![OrderBy::asc("bucket"), OrderBy::asc("entry_id")])
        .await
        .expect("page through the table by a tied column");

    let unique: BTreeSet<i64> = seen.iter().copied().collect();
    assert_eq!(
        unique.len(),
        ROWS as usize,
        "a page boundary inside a run of equal sort values must not drop the rows after it"
    );
    assert_eq!(seen.len(), ROWS as usize, "and must not repeat any");
}

#[tokio::test]
async fn a_descending_page_takes_its_direction_from_the_query() {
    let Ok(schema) = SqlSchema::mint().await else {
        return;
    };
    let pool = schema.pool();
    seeded(pool).await;

    let seen = page_through(pool, vec![OrderBy::desc("entry_id")])
        .await
        .expect("page through the table backwards");

    assert_eq!(seen.len(), ROWS as usize);
    assert_eq!(
        seen.first().copied(),
        Some(ROWS - 1),
        "a descending page starts at the top"
    );
    assert!(
        seen.windows(2).all(|pair| pair[0] > pair[1]),
        "and stays descending across the page boundary"
    );
}

#[tokio::test]
async fn a_page_with_no_order_is_ordered_by_the_key() {
    let Ok(schema) = SqlSchema::mint().await else {
        return;
    };
    let pool = schema.pool();
    seeded(pool).await;

    let seen = page_through(pool, Vec::new())
        .await
        .expect("page through an unordered query");

    assert_eq!(seen.len(), ROWS as usize);
    assert!(
        seen.windows(2).all(|pair| pair[0] < pair[1]),
        "an unordered page falls back to the key, ascending"
    );
}

#[derive(Model, Debug)]
#[model(sql)]
struct Stamped {
    #[model(id)]
    stamped_id: i64,
    seen_at: DateTime<Utc>,
}

/// Every row shares one millisecond, so a millisecond-rounded cursor would never advance.
#[tokio::test]
async fn a_microsecond_boundary_is_not_rounded_to_the_millisecond() {
    let Ok(schema) = SqlSchema::mint().await else {
        return;
    };
    let pool = schema.pool();
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS stampeds (stamped_id BIGINT PRIMARY KEY, seen_at TIMESTAMPTZ NOT NULL)",
    )
    .await
    .expect("the stamped table");

    let base = Utc
        .timestamp_micros(1_800_000_000_123_000)
        .single()
        .expect("a representable instant");
    let repo = pool.repository::<Stamped>();
    for stamped_id in 0..ROWS {
        repo.insert(&Stamped {
            stamped_id,
            seen_at: base + chrono::Duration::microseconds(stamped_id),
        })
        .await
        .expect("seed a stamped row");
    }

    let mut seen = Vec::new();
    let mut cursor = None;
    let max_pages = ROWS as u64 / PAGE + 2;
    for _ in 0..=max_pages {
        let page = SqlModelQuery::<Stamped>::new(pool)
            .order_by(OrderBy::asc("seen_at"))
            .order_by(OrderBy::asc("stamped_id"))
            .page_size(PAGE)
            .execute_page(cursor)
            .await
            .expect("a page of stamped rows");
        seen.extend(page.items.iter().map(|row| row.stamped_id));
        match page.next_page {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }

    let unique: BTreeSet<i64> = seen.iter().copied().collect();
    assert_eq!(
        unique.len(),
        ROWS as usize,
        "every row appears once: {} rows, {} distinct",
        seen.len(),
        unique.len()
    );
    assert_eq!(
        seen.len(),
        ROWS as usize,
        "a boundary rounded below the row it came from returns that row again"
    );
}

#[tokio::test]
async fn a_page_read_backwards_from_its_first_row_is_the_page_before() {
    let Ok(schema) = SqlSchema::mint().await else {
        return;
    };
    let pool = schema.pool();
    seeded(pool).await;
    let forward = || {
        SqlModelQuery::<Entry>::new(pool)
            .page_size(PAGE)
            .order_by(OrderBy::asc("bucket"))
            .order_by(OrderBy::asc("entry_id"))
    };
    let first = forward().execute_page(None).await.expect("the first page");
    assert!(
        first.previous_page.is_none(),
        "the first page has none before it"
    );
    let mut pages = vec![
        first
            .items
            .iter()
            .map(|entry| entry.entry_id)
            .collect::<Vec<_>>(),
    ];
    let mut cursor = first.next_page;
    while let Some(after) = cursor {
        let page = forward().execute_page(Some(after)).await.expect("a page");
        let back = page
            .previous_page
            .expect("a page read with a cursor names its start");
        let before = SqlModelQuery::<Entry>::new(pool)
            .page_size(PAGE)
            .order_by(OrderBy::desc("bucket"))
            .order_by(OrderBy::desc("entry_id"))
            .execute_page(Some(back.reversed()))
            .await
            .expect("the page before, read backwards");
        let mut ids: Vec<i64> = before.items.iter().map(|entry| entry.entry_id).collect();
        ids.reverse();
        assert_eq!(
            Some(&ids),
            pages.last(),
            "the page before page {}",
            pages.len() + 1
        );
        pages.push(page.items.iter().map(|entry| entry.entry_id).collect());
        cursor = page.next_page;
    }
    assert_eq!(pages.concat().len(), ROWS as usize);
}

#[derive(Model, Debug)]
#[model(sql)]
struct Ranked {
    #[model(id)]
    ranked_id: i64,
    place: i32,
    shelf: i16,
}

#[tokio::test]
async fn a_page_ordered_by_a_narrower_integer_resumes_after_its_last_row() {
    let Ok(schema) = SqlSchema::mint().await else {
        return;
    };
    let pool = schema.pool();
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS rankeds (ranked_id BIGINT PRIMARY KEY, place INT NOT NULL, \
         shelf SMALLINT NOT NULL)",
    )
    .await
    .expect("the ranked table");
    let repo = pool.repository::<Ranked>();
    for ranked_id in 0..ROWS {
        let place = i32::try_from(ranked_id).expect("a small id");
        repo.insert(&Ranked {
            ranked_id,
            place,
            shelf: i16::try_from(place % 5).expect("a small shelf"),
        })
        .await
        .expect("seed a ranked row");
    }
    for column in ["place", "shelf"] {
        let mut seen = BTreeSet::new();
        let mut cursor = None;
        for _ in 0..=ROWS as u64 / PAGE + 2 {
            let page = SqlModelQuery::<Ranked>::new(pool)
                .order_by(OrderBy::asc(column))
                .order_by(OrderBy::asc("ranked_id"))
                .page_size(PAGE)
                .execute_page(cursor)
                .await
                .unwrap_or_else(|error| panic!("a page ordered by {column}: {error}"));
            seen.extend(page.items.iter().map(|row| row.ranked_id));
            match page.next_page {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        assert_eq!(
            seen.len(),
            ROWS as usize,
            "every row once, ordered by {column}"
        );
    }
}
