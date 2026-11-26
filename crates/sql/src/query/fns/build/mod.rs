//! SQL-string builders for the SQL wire's queries; every identifier is validated and every value bound as `$N`.
//!
//! The executor, not the builders, refuses an unaddressed query, so the constraint lives in one place.

mod count_sql;
mod delete_sql;
mod filter_condition;
mod order_clause;
mod page_clause;
mod select_claimable_sql;
mod select_sql;
mod update_sql;
mod where_clause;

pub(crate) use count_sql::build_count_sql;
pub(crate) use delete_sql::build_delete_sql;
pub(crate) use filter_condition::build_filter_condition;
pub(crate) use order_clause::build_order_clause;
pub(crate) use page_clause::build_page_clause;
pub(crate) use select_claimable_sql::build_select_claimable_sql;
pub(crate) use select_sql::build_select_sql;
pub(crate) use update_sql::build_update_sql;
pub(crate) use where_clause::build_where_clause;
