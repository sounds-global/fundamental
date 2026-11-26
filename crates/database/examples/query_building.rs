//! Backend-agnostic query building with `fundamental_db`, with no connection and no I/O.
//!
//! Run it with:
//!
//! ```sh
//! cargo run --example query_building -p fundamental_db --all-features
//! ```

use fundamental_core::model::{BoundTo, Model, SqlDb};
use fundamental_db::{Filter, OrderBy, Pagination, QueryBuilder};

/// A minimal model, implemented by hand so the example needs no backend crate.
#[allow(dead_code)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
struct User {
    id: String,
    email: String,
}

impl Model for User {
    const TYPE_NAME: &'static str = "User";
}

// `BoundTo<SqlDb>` makes `QueryBuilder<User, SqlDb>` valid and any other backend a type error.
impl BoundTo<SqlDb> for User {}

fn main() {
    // Nothing executes until a backend's query executor runs the spec.
    let query = QueryBuilder::<User, SqlDb>::new()
        .filter(Filter::eq("status", "active"))
        .filter(Filter::gte("age", 18))
        .filter(Filter::or(vec![
            Filter::like("email", "%@example.com"),
            Filter::like("email", "%@fundamental.dev"),
        ]))
        .filter(Filter::between("score", 50, 100))
        .filter(Filter::is_not_null("verified_at"))
        .order_by(OrderBy::desc("created_at"))
        .order_by(OrderBy::asc("email").nulls_last())
        .select(["id", "email", "status", "created_at"])
        .paginate(Pagination::page(2, 25));

    println!("Query for model `{}`:", User::TYPE_NAME);
    println!("  filters ({}):", query.get_filters().len());
    for filter in query.get_filters() {
        println!("    - {:?}", filter.condition);
    }

    println!("  order_by ({}):", query.get_order_by().len());
    for order in query.get_order_by() {
        println!(
            "    - {} {:?} (nulls: {:?})",
            order.column, order.direction, order.nulls
        );
    }

    if let Some(columns) = query.get_select() {
        println!("  select: {:?}", columns);
    }

    if let Some(page) = query.get_pagination() {
        println!(
            "  pagination: limit={}, offset={}, page={}",
            page.limit,
            page.offset,
            page.current_page()
        );
    }

    // The builder is `Clone`, so a base query can be reused and specialized.
    let recent_only = query
        .clone()
        .clear_filters()
        .filter(Filter::gt("created_at", "2026-01-01"));
    println!(
        "\nDerived query keeps {} order clause(s) but only {} filter(s).",
        recent_only.get_order_by().len(),
        recent_only.get_filters().len()
    );
}
