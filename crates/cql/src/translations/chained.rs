//! A record's key bound ahead of the rest of a statement's values.

use scylla::serialize::SerializationError;
use scylla::serialize::row::{RowSerializationContext, SerializeRow};
use scylla::serialize::writers::RowWriter;

use crate::CqlValues;

/// A record's key values (`K`, its [`ToRow::KeyValues`](crate::ToRow::KeyValues)), followed by
/// further values: the binding of a companion statement whose leading columns are the record's
/// key, in the same order. The key is bound as the record binds it, never rendered.
pub(crate) struct Chained<'a, K>(pub(crate) &'a K, pub(crate) CqlValues);

impl<K: SerializeRow> SerializeRow for Chained<'_, K> {
    fn serialize(
        &self,
        ctx: &RowSerializationContext<'_>,
        writer: &mut RowWriter<'_>,
    ) -> Result<(), SerializationError> {
        let columns = ctx.columns();
        let (key, rest) = columns.split_at(columns.len().saturating_sub(self.1.0.len()));
        self.0
            .serialize(&RowSerializationContext::from_specs(key), writer)?;
        self.1
            .serialize(&RowSerializationContext::from_specs(rest), writer)
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty() && self.1.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use scylla::frame::response::result::{ColumnSpec, ColumnType, NativeType, TableSpec};
    use scylla::serialize::row::{RowSerializationContext, SerializeRow};
    use scylla::serialize::writers::RowWriter;

    use super::Chained;
    use crate::{CqlValue, CqlValues};

    fn specs() -> Vec<ColumnSpec<'static>> {
        let table = || TableSpec::borrowed("ks", "notices_i18n");
        vec![
            ColumnSpec::borrowed(
                "organisation_id",
                ColumnType::Native(NativeType::Uuid),
                table(),
            ),
            ColumnSpec::borrowed("id", ColumnType::Native(NativeType::BigInt), table()),
            ColumnSpec::borrowed("field", ColumnType::Native(NativeType::Text), table()),
            ColumnSpec::borrowed("locale", ColumnType::Native(NativeType::Text), table()),
        ]
    }

    fn bound(row: &impl SerializeRow) -> (Vec<u8>, usize) {
        let specs = specs();
        let mut bytes = Vec::new();
        let mut writer = RowWriter::new(&mut bytes);
        row.serialize(&RowSerializationContext::from_specs(&specs), &mut writer)
            .expect("the row binds");
        let count = writer.value_count();
        (bytes, count)
    }

    #[test]
    fn the_key_binds_its_columns_and_the_rest_bind_theirs() {
        let organisation = uuid::Uuid::from_u128(7);
        let key = (organisation, 5_i64);
        let rest = || CqlValues(vec![CqlValue::text("title"), CqlValue::text("nl")]);

        let chained = bound(&Chained(&key, rest()));

        let mut every = vec![CqlValue::uuid(organisation), CqlValue::bigint(5)];
        every.extend(rest().0);
        assert_eq!(chained, bound(&CqlValues(every)));
        assert_eq!(chained.1, 4);
    }

    #[test]
    fn a_key_with_too_few_values_for_its_columns_is_refused() {
        let specs = specs();
        let mut bytes = Vec::new();
        let mut writer = RowWriter::new(&mut bytes);
        let key = (uuid::Uuid::from_u128(7),);
        let rest = CqlValues(vec![CqlValue::text("title"), CqlValue::text("nl")]);

        let bound = Chained(&key, rest)
            .serialize(&RowSerializationContext::from_specs(&specs), &mut writer);

        assert!(bound.is_err(), "one key value cannot bind two key columns");
    }
}
