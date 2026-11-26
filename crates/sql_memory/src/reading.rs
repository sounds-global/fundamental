use std::cmp::Ordering;

use fundamental_db::{
    DatabaseResult, Filter, FilterValue, OrderBy, OrderDirection, OrderNulls, Pagination,
};

use crate::Held;
use crate::cells::column;
use crate::compare::{compared, same};
use crate::kinds::kinds;
use crate::matching::{checked, kept};
use crate::memory_database::Row;

/// Where each of `M`'s key columns stands among its cells, in key order.
pub(crate) fn key<M: Held>() -> DatabaseResult<Vec<usize>> {
    M::KEY_COLUMNS
        .iter()
        .map(|name| column::<M>(name))
        .collect()
}

pub(crate) fn same_key(key: &[usize], left: &[FilterValue], right: &[FilterValue]) -> bool {
    key.iter()
        .all(|at| matches!((left.get(*at), right.get(*at)), (Some(a), Some(b)) if same(a, b)))
}

/// The `rows` every one of `filters` keeps, soft-deleted ones left out, in key order so no test leans on an unordered read.
pub(crate) fn selected<M: Held>(
    rows: &[Row<M>],
    filters: &[Filter],
) -> DatabaseResult<Vec<Row<M>>> {
    chosen(rows, filters, Deleted::Left)
}

/// Whether a row deleted softly is read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Deleted {
    /// Left out, as a read leaves it.
    Left,
    /// Read, as a delete by filter reads it.
    Read,
}

pub(crate) fn chosen<M: Held>(
    rows: &[Row<M>],
    filters: &[Filter],
    deleted: Deleted,
) -> DatabaseResult<Vec<Row<M>>> {
    let kinds = kinds::<M>();
    checked::<M>(&kinds, filters)?;
    let mut found = rows
        .iter()
        .filter(|row| deleted == Deleted::Read || !row.deleted())
        .filter_map(|row| match kept::<M>(&kinds, filters, &row.cells) {
            Ok(true) => Some(Ok(row.clone())),
            Ok(false) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<DatabaseResult<Vec<_>>>()?;
    let key: Vec<OrderBy> = M::KEY_COLUMNS
        .iter()
        .map(|column| OrderBy::asc(*column))
        .collect();
    ordered::<M>(&mut found, &key)?;
    Ok(found)
}

/// `rows` sorted by `order`, NULLs last going up and first going down unless the order says.
pub(crate) fn ordered<M: Held>(rows: &mut [Row<M>], order: &[OrderBy]) -> DatabaseResult<()> {
    let columns = order
        .iter()
        .map(|by| column::<M>(&by.column))
        .collect::<DatabaseResult<Vec<usize>>>()?;
    rows.sort_by(|left, right| {
        order
            .iter()
            .zip(&columns)
            .map(|(by, at)| sorted(by, left.cells.get(*at), right.cells.get(*at)))
            .find(|order| order.is_ne())
            .unwrap_or(Ordering::Equal)
    });
    Ok(())
}

fn sorted(by: &OrderBy, left: Option<&FilterValue>, right: Option<&FilterValue>) -> Ordering {
    let descending = by.direction == OrderDirection::Desc;
    let nulls_first = by
        .nulls
        .map_or(descending, |nulls| nulls == OrderNulls::First);
    let null = |cell: Option<&FilterValue>| matches!(cell, None | Some(FilterValue::Null));
    match (null(left), null(right)) {
        (true, true) => Ordering::Equal,
        (true, false) if nulls_first => Ordering::Less,
        (true, false) => Ordering::Greater,
        (false, true) if nulls_first => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => {
            let order = match (left, right) {
                (Some(left), Some(right)) => compared(left, right).unwrap_or(Ordering::Equal),
                _ => Ordering::Equal,
            };
            if descending { order.reverse() } else { order }
        }
    }
}

pub(crate) fn paged<M>(rows: Vec<Row<M>>, page: Option<&Pagination>) -> Vec<Row<M>> {
    let (offset, limit) = page.map_or((0, u64::MAX), |page| (page.offset, page.limit));
    rows.into_iter()
        .skip(usize::try_from(offset).unwrap_or(usize::MAX))
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
        .collect()
}
