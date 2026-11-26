//! The [`PartitionValues`] trait — a model's partition key, in column order.

use crate::{CqlValue, ToCqlValue};

/// A model's [`PartitionKey`] rendered as the ordered CQL parameters that address one
/// partition.
///
/// Implemented for the bare type and tuples up to five, so a wrong arity is a compile error;
/// order is checked only where the columns' types differ.
///
/// [`PartitionKey`]: fundamental_core::model::CqlModel::PartitionKey
pub trait PartitionValues {
    /// The number of partition columns these values address.
    ///
    /// Compared against `ToRow::partition_key_columns().len()` where the values are bound.
    const ARITY: usize;

    /// These values as CQL parameters, in partition-key column order.
    fn to_partition_values(&self) -> Vec<CqlValue>;
}

/// A one-column partition, addressed by the column's own value.
impl<A> PartitionValues for (A,)
where
    A: ToCqlValue,
{
    const ARITY: usize = 1;

    fn to_partition_values(&self) -> Vec<CqlValue> {
        vec![self.0.to_cql_value()]
    }
}

/// Every arity from two to five, generated from one body so they cannot drift apart.
macro_rules! partition_values_tuple {
    ($arity:expr; $($name:ident . $index:tt),+) => {
        impl<$($name),+> PartitionValues for ($($name),+)
        where
            $($name: ToCqlValue),+
        {
            const ARITY: usize = $arity;

            fn to_partition_values(&self) -> Vec<CqlValue> {
                vec![$(self.$index.to_cql_value()),+]
            }
        }
    };
}

partition_values_tuple!(2; A.0, B.1);
partition_values_tuple!(3; A.0, B.1, C.2);
partition_values_tuple!(4; A.0, B.1, C.2, D.3);
partition_values_tuple!(5; A.0, B.1, C.2, D.3, E.4);

/// A one-column partition whose `PartitionKey` is the bare field type.
/// Per type, since a blanket impl over `ToCqlValue` would overlap the tuple impls.
macro_rules! partition_values_scalar {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl PartitionValues for $ty {
                const ARITY: usize = 1;

                fn to_partition_values(&self) -> Vec<CqlValue> {
                    vec![<Self as ToCqlValue>::to_cql_value(self)]
                }
            }
        )+
    };
}

partition_values_scalar!(String, uuid::Uuid, i64, i32, bool, Vec<u8>, CqlValue);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_one_column_partition_renders_one_value() {
        assert_eq!(<String as PartitionValues>::ARITY, 1);
        let values = "ppid".to_string().to_partition_values();
        assert!(matches!(values.as_slice(), [CqlValue::Text(t)] if t == "ppid"));
    }

    #[test]
    fn a_composite_partition_renders_its_columns_in_declaration_order() {
        assert_eq!(<(String, uuid::Uuid) as PartitionValues>::ARITY, 2);
        let id = uuid::Uuid::from_u128(7);
        let values = ("eu".to_string(), id).to_partition_values();
        assert!(
            matches!(values.as_slice(), [CqlValue::Text(z), CqlValue::Uuid(u)] if z == "eu" && *u == id),
            "the tuple's positions are the partition columns' order, not a set"
        );
    }

    #[test]
    fn arity_is_the_number_of_values_bound() {
        assert_eq!(
            <(String, String, String) as PartitionValues>::ARITY,
            ("a".to_string(), "b".to_string(), "c".to_string())
                .to_partition_values()
                .len()
        );
        assert_eq!(
            <(String, String, String, String, i64) as PartitionValues>::ARITY,
            (
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
                5i64
            )
                .to_partition_values()
                .len()
        );
    }
}
