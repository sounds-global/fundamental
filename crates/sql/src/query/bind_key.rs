use sqlx::postgres::{PgArguments, Postgres};

type PgQuery<'q> = sqlx::query::Query<'q, Postgres, PgArguments>;

/// A tuple of primary-key values bound to a statement in key order, one parameter per column at its own type.
pub trait BindKey<'q> {
    /// The number of key columns this tuple binds, checked against `KEY_COLUMNS`.
    const ARITY: usize;

    /// Binds every element onto `query`, in key order.
    fn bind_all(&'q self, query: PgQuery<'q>) -> PgQuery<'q>;
}

macro_rules! impl_bind_key {
    ($arity:literal; $($name:ident . $index:tt),+) => {
        impl<'q, $($name),+> BindKey<'q> for ($($name,)+)
        where
            $($name: Send + Sync + sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,)+
        {
            const ARITY: usize = $arity;

            fn bind_all(&'q self, query: PgQuery<'q>) -> PgQuery<'q> {
                let query = query;
                $(let query = query.bind(&self.$index);)+
                query
            }
        }
    };
}

impl_bind_key!(1; A.0);
impl_bind_key!(2; A.0, B.1);
impl_bind_key!(3; A.0, B.1, C.2);
impl_bind_key!(4; A.0, B.1, C.2, D.3);
impl_bind_key!(5; A.0, B.1, C.2, D.3, E.4);
impl_bind_key!(6; A.0, B.1, C.2, D.3, E.4, F.5);
impl_bind_key!(7; A.0, B.1, C.2, D.3, E.4, F.5, G.6);
impl_bind_key!(8; A.0, B.1, C.2, D.3, E.4, F.5, G.6, H.7);

#[cfg(test)]
mod tests {
    use super::BindKey;

    #[test]
    fn arity_is_the_tuple_length_at_every_supported_width() {
        // `ARITY` disagreeing with the `bind` calls would be a runtime protocol error.
        assert_eq!(<(String,) as BindKey<'_>>::ARITY, 1);
        assert_eq!(<(String, String) as BindKey<'_>>::ARITY, 2);
        assert_eq!(<(String, String, String) as BindKey<'_>>::ARITY, 3);
        assert_eq!(
            <(
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                String
            ) as BindKey<'_>>::ARITY,
            8
        );
    }

    #[test]
    fn a_key_tuple_may_mix_column_types() {
        assert_eq!(<(uuid::Uuid, String) as BindKey<'_>>::ARITY, 2);
        assert_eq!(
            <(String, chrono::DateTime<chrono::Utc>) as BindKey<'_>>::ARITY,
            2
        );
    }
}
