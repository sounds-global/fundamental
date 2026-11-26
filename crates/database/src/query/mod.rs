//! Type-safe query construction and execution.

mod builder;
mod executor;
mod filter;
mod order;
mod pagination;

pub use builder::QueryBuilder;
pub use executor::QueryExecutor;
pub use pagination::Pagination;

pub use filter::*;
pub use order::*;

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::model::{TestDb, TestModel};

    #[test]
    fn test_query_builder() {
        let query = QueryBuilder::<TestModel, TestDb>::new()
            .filter(Filter::eq("status", "active"))
            .filter(Filter::gt("age", 18))
            .order_by(OrderBy::desc("created_at"))
            .limit(10)
            .offset(20);

        assert_eq!(query.get_filters().len(), 2);
        assert_eq!(query.get_order_by().len(), 1);
        assert_eq!(query.get_pagination().unwrap().limit, 10);
        assert_eq!(query.get_pagination().unwrap().offset, 20);
    }

    #[test]
    fn test_pagination_page() {
        let p = Pagination::page(1, 10);
        assert_eq!(p.limit, 10);
        assert_eq!(p.offset, 0);

        let p = Pagination::page(3, 10);
        assert_eq!(p.limit, 10);
        assert_eq!(p.offset, 20);

        assert_eq!(p.current_page(), 3);
    }

    #[test]
    fn test_filter_values() {
        let _ = Filter::eq("name", "test");
        let _ = Filter::eq("count", 42);
        let _ = Filter::eq("active", true);
        let _ = Filter::between("age", 18, 65);
    }
}
