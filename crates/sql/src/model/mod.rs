//! Model-centric query builder for YSQL.

mod cursor;
mod cursor_boundary;
mod in_language;
// Crate-visible, so the locked keyed read builds the same predicate as the read.
pub(crate) mod key_predicate;
mod page;
mod query;
pub(crate) mod translations;
mod update;

pub use page::SqlPage;
/// Model-centric query builder for YSQL.
///
/// # Example
///
/// ```rust,no_run
/// use fundamental_sql::SqlModelQuery;
/// use fundamental_db::{Filter, IsolationLevel, OrderBy, Transaction, Transactional};
/// # #[derive(Clone, fundamental_macros::Model)]
/// # #[model(sql, table = "users")]
/// # struct User {
/// #     #[model(id)]
/// #     id: uuid::Uuid,
/// #     email: String,
/// #     org_id: String,
/// #     created_at: i64,
/// # }
/// # async fn example(
/// #     pool: &fundamental_sql::SqlPool,
/// #     org_id: &str,
/// # ) -> Result<(), Box<dyn std::error::Error>> {
/// // Simple query with filters (pool usage)
/// let users = SqlModelQuery::<User>::new(pool)
///     .filter(Filter::eq("email", "alice@example.com"))
///     .execute().await?;
///
/// // Transaction usage (same API!)
/// let mut tx = pool.begin(IsolationLevel::Serializable).await?;
/// let users = SqlModelQuery::<User>::new(&mut tx)
///     .filter(Filter::eq("org_id", org_id))
///     .execute().await?;
/// tx.commit().await?;
///
/// // Paginated query
/// let page = SqlModelQuery::<User>::new(pool)
///     .order_by(OrderBy::desc("created_at"))
///     .page_size(50)
///     .execute_page(None).await?;
/// # let _ = (users, page);
/// # Ok(())
/// # }
/// ```
pub use query::SqlModelQuery;
pub use translations::{find_translation_ids, insert_translated_model, upsert_translated_model};
pub use update::SqlUpdateBuilder;

pub use cursor::*;
