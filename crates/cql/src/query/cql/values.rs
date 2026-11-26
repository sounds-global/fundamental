//! The [`CqlValues`] wrapper and its `scylla` driver `SerializeRow` implementation.

use crate::CqlValue;

/// A wrapper for a vector of CqlValues that implements SerializeRow.
#[derive(Debug, Clone)]
pub struct CqlValues(pub Vec<CqlValue>);

impl scylla::serialize::row::SerializeRow for CqlValues {
    fn serialize(
        &self,
        ctx: &scylla::serialize::row::RowSerializationContext<'_>,
        writer: &mut scylla::serialize::writers::RowWriter<'_>,
    ) -> Result<(), scylla::serialize::SerializationError> {
        use scylla::serialize::value::SerializeValue as SV;

        for (value, col_spec) in self.0.iter().zip(ctx.columns()) {
            let cell_writer = writer.make_cell_writer();
            SV::serialize(value, col_spec.typ(), cell_writer)?;
        }

        Ok(())
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
