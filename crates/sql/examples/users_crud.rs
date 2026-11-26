//! End-to-end CRUD example for the YugabyteDB YSQL adapter; returns early without `DATABASE_URL`.
//!
//! ```sh
//! docker run -d -p 5433:5433 yugabytedb/yugabyte bin/yugabyted start --background=false
//! DATABASE_URL='postgresql://yugabyte@localhost:5433/yugabyte?sslmode=disable' \
//!     cargo run --example users_crud -p fundamental_sql
//! ```

use fundamental_core::model::SqlDb;
use fundamental_db::{Filter, KeyedRepository, OrderBy, QueryBuilder, QueryExecutor, Repository};
use fundamental_macros::Model;
use fundamental_sql::{SqlConfig, SqlPool};

/// A user record stored in YSQL.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql, table = "users")]
struct User {
    #[model(id)]
    id: uuid::Uuid,
    email: String,
    name: String,
}

// Boxed, so `?` converts both `SqlError` and `DatabaseError`.
type Error = Box<dyn std::error::Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            println!(
                "DATABASE_URL is not set; skipping the live demo.\n\
                 Set it to a YSQL connection string to run this example, e.g.\n  \
                 DATABASE_URL='postgresql://yugabyte@localhost:5433/yugabyte?sslmode=disable'"
            );
            return Ok(());
        }
    };

    // Development config, TLS off; use `SqlConfig::new(url)` in production.
    let config = SqlConfig::development(url);
    let pool = SqlPool::connect(&config).await?;

    pool.execute(
        "CREATE TABLE IF NOT EXISTS users (\
            id UUID PRIMARY KEY, \
            email TEXT UNIQUE NOT NULL, \
            name TEXT NOT NULL)",
    )
    .await?;

    let repo = pool.repository::<User>();

    let user = User {
        id: uuid::Uuid::new_v4(),
        email: "ada@example.com".into(),
        name: "Ada Lovelace".into(),
    };
    let inserted = repo.insert(&user).await?;
    println!("inserted: {inserted:?}");

    if let Some(found) = repo.find(&user.id).await? {
        println!("found by id: {found:?}");
    }

    let renamed = User {
        name: "Ada King, Countess of Lovelace".into(),
        ..user.clone()
    };
    let updated = repo.update(&renamed).await?;
    println!("updated name: {}", updated.name);

    let executor = pool.query_executor::<User>();
    let query = QueryBuilder::<User, SqlDb>::new()
        .filter(Filter::eq("name", "Ada King, Countess of Lovelace"))
        .order_by(OrderBy::asc("email"))
        .limit(10);

    let matches = executor.execute(&query).await?;
    println!("query matched {} row(s)", matches.len());
    println!("total matching count: {}", executor.count(&query).await?);

    let deleted = repo.delete(&user.id).await?;
    println!("deleted: {deleted}");

    Ok(())
}
