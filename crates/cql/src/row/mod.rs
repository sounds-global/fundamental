//! Row conversion traits for YCQL.

mod builder;
mod from;
mod to;

pub use from::{FromCqlValue, FromRow};
pub use to::ToRow;

pub(crate) use builder::*;

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct TestEvent {
        user_id: Uuid,
        timestamp: i64,
        event_type: String,
        data: Vec<u8>,
    }

    impl ToRow for TestEvent {
        fn columns() -> &'static [&'static str] {
            &["user_id", "timestamp", "event_type", "data"]
        }

        fn partition_key_columns() -> &'static [&'static str] {
            &["user_id"]
        }

        fn clustering_key_columns() -> &'static [&'static str] {
            &["timestamp"]
        }

        type InsertValues = (Uuid, i64, String, Vec<u8>);
        type UpdateValues = (String, Vec<u8>, Uuid, i64);
        type KeyValues = (Uuid, i64);

        fn to_values(&self) -> Self::InsertValues {
            (
                self.user_id,
                self.timestamp,
                self.event_type.clone(),
                self.data.clone(),
            )
        }

        fn to_update_values(&self) -> Self::UpdateValues {
            (
                self.event_type.clone(),
                self.data.clone(),
                self.user_id,
                self.timestamp,
            )
        }

        fn to_key_values(&self) -> Self::KeyValues {
            (self.user_id, self.timestamp)
        }
    }

    #[test]
    fn test_insert_builder() {
        let cql = InsertBuilder::new("events", "user_events")
            .columns_from::<TestEvent>()
            .build();

        assert_eq!(
            cql,
            "INSERT INTO events.user_events (user_id, timestamp, event_type, data) VALUES (?, ?, ?, ?)"
        );
    }

    #[test]
    fn test_insert_builder_with_ttl() {
        let cql = InsertBuilder::new("events", "user_events")
            .columns_from::<TestEvent>()
            .ttl(86400)
            .build();

        assert!(cql.contains("USING TTL 86400"));
    }

    #[test]
    fn test_insert_builder_if_not_exists() {
        let cql = InsertBuilder::new("events", "user_events")
            .columns_from::<TestEvent>()
            .if_not_exists()
            .build();

        assert!(cql.contains("IF NOT EXISTS"));
    }

    #[test]
    fn test_update_builder() {
        let cql = UpdateBuilder::new("events", "user_events")
            .set_columns_from::<TestEvent>()
            .where_columns_from::<TestEvent>()
            .build();

        assert_eq!(
            cql,
            "UPDATE events.user_events SET event_type = ?, data = ? WHERE user_id = ? AND timestamp = ?"
        );
    }

    #[test]
    fn test_update_builder_with_ttl_and_if_exists() {
        let cql = UpdateBuilder::new("events", "user_events")
            .set_columns_from::<TestEvent>()
            .where_columns_from::<TestEvent>()
            .ttl(3600)
            .if_exists()
            .build();

        assert!(cql.contains("USING TTL 3600"));
        assert!(cql.contains("IF EXISTS"));
    }

    #[test]
    fn test_delete_builder() {
        let cql = DeleteBuilder::new("events", "user_events")
            .where_columns_from::<TestEvent>()
            .build();

        assert_eq!(
            cql,
            "DELETE FROM events.user_events WHERE user_id = ? AND timestamp = ?"
        );
    }

    #[test]
    fn test_delete_builder_if_exists() {
        let cql = DeleteBuilder::new("events", "user_events")
            .where_columns_from::<TestEvent>()
            .if_exists()
            .build();

        assert!(cql.contains("IF EXISTS"));
    }

    #[test]
    fn test_value_columns() {
        let value_cols = TestEvent::value_columns();
        assert_eq!(value_cols, vec!["event_type", "data"]);
    }

    #[test]
    fn test_counter_update_builder() {
        let cql = CounterUpdateBuilder::new("fundamental", "rate_limits", "count")
            .where_columns(&["key", "window_start"])
            .build();

        assert_eq!(
            cql,
            "UPDATE fundamental.rate_limits SET count = count + ? WHERE key = ? AND window_start = ?"
        );
    }

    #[test]
    fn test_counter_update_builder_with_ttl() {
        let cql = CounterUpdateBuilder::new("fundamental", "rate_limits", "count")
            .where_columns(&["key", "window_start"])
            .ttl(3600)
            .build();

        assert!(cql.contains("USING TTL 3600"));
        assert!(cql.contains("SET count = count + ?"));
    }

    #[test]
    fn test_counter_select_builder() {
        let cql = CounterSelectBuilder::new("fundamental", "rate_limits", "count")
            .where_columns(&["key", "window_start"])
            .build();

        assert_eq!(
            cql,
            "SELECT count FROM fundamental.rate_limits WHERE key = ? AND window_start = ?"
        );
    }
}
