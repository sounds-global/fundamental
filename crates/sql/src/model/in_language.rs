//! A read of a model in one language: its `Localized` fields filtered and sorted by their text in
//! that language, through a subquery adding each field's text as `Localized::resolve` reads it.

use fundamental_core::i18n::Locale;
use fundamental_core::model::{SqlDb, SqlModel};
use fundamental_db::{Filter, FilterValue, OrderBy, QueryBuilder, SchemaName};

use crate::SqlError;
use crate::query::{build_order_clause, build_where_clause};

const ROW: &str = "fundamental_row";

/// The column a read in a language adds for the translatable `column`.
pub(crate) fn shown(column: &str) -> String {
    format!("fundamental_in_{column}")
}

/// The column a read in a language reads for `column`, when it is one of `M`'s translatable columns.
fn shown_name<M: SqlModel>(column: &str) -> Option<String> {
    M::translatable_columns()
        .contains(&column)
        .then(|| shown(column))
}

/// `filters` with every translatable column of `M` they name read in the language.
pub(crate) fn shown_filters<M: SqlModel>(filters: &[Filter]) -> Vec<Filter> {
    filters
        .iter()
        .map(|filter| {
            let mut condition = filter.condition.clone();
            condition.rename_columns(&shown_name::<M>);
            Filter { condition }
        })
        .collect()
}

/// `orders` with every translatable column of `M` they name read in the language.
pub(crate) fn shown_orders<M: SqlModel>(orders: &[OrderBy]) -> Vec<OrderBy> {
    orders
        .iter()
        .map(|order| OrderBy {
            column: shown_name::<M>(&order.column).unwrap_or_else(|| order.column.clone()),
            ..order.clone()
        })
        .collect()
}

/// A language tag as a literal, refused unless it is letters, digits and hyphens.
fn literal(language: &Locale) -> Result<String, SqlError> {
    let tag = language.to_string();
    if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(SqlError::query(format!("`{tag}` is not a language tag")));
    }
    Ok(format!("'{tag}'"))
}

/// What a read of `M` in `language` selects from: its table, with each translatable column's text beside it.
fn source<M: SqlModel>(schema: &SchemaName, language: &Locale) -> Result<String, SqlError> {
    let mut languages = vec![literal(language)?];
    if !language.is_language_only() {
        languages.push(literal(&language.language_fallback())?);
    }
    let companion = M::translation_table().map(|table| schema.qualify(table));
    let key = M::KEY_COLUMNS
        .iter()
        .map(|column| format!("i.{column} = {ROW}.{column}"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let mut columns = vec![format!("{ROW}.*")];
    for column in M::translatable_columns() {
        crate::ident::validate_identifier(column, false)?;
        let mut texts: Vec<String> = match &companion {
            Some(companion) => languages
                .iter()
                .map(|tag| {
                    format!(
                        "(SELECT i.value FROM {companion} AS i WHERE {key} AND i.field = \
                         '{column}' AND i.locale = {tag})"
                    )
                })
                .collect(),
            None => Vec::new(),
        };
        texts.push(format!("{ROW}.{column}"));
        columns.push(format!(
            "COALESCE({}) AS {}",
            texts.join(", "),
            shown(column)
        ));
    }
    Ok(format!(
        "(SELECT {} FROM {} AS {ROW}) AS {}",
        columns.join(", "),
        schema.qualify(M::TABLE),
        M::TABLE
    ))
}

/// The `WHERE` of a read in a language: the query's filters read in it, and the soft delete.
fn where_in<M: SqlModel, State>(
    query: &QueryBuilder<M, SqlDb, State>,
) -> Result<(String, Vec<FilterValue>), SqlError> {
    let (clause, values, _) = build_where_clause(&shown_filters::<M>(query.get_filters()), 1)?;
    let mut parts: Vec<String> = Vec::new();
    if !clause.is_empty() {
        parts.push(clause);
    }
    if M::SOFT_DELETE {
        parts.push(format!("{} IS NULL", M::DELETED_AT_COLUMN));
    }
    let clause = if parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", parts.join(" AND "))
    };
    Ok((clause, values))
}

/// `query`'s `SELECT` with `M`'s translatable fields filtered and sorted in `language`.
pub(crate) fn build_select_sql_in<M: SqlModel, State>(
    schema: &SchemaName,
    query: &QueryBuilder<M, SqlDb, State>,
    language: &Locale,
) -> Result<(String, Vec<FilterValue>), SqlError> {
    let selected = match query.get_select() {
        Some(columns) => {
            for column in columns {
                crate::ident::validate_identifier(column, true)?;
            }
            columns.join(", ")
        }
        None => "*".to_owned(),
    };
    let select = super::translations::select_list::<M>(schema, &selected, M::TABLE);
    let (filters, values) = where_in(query)?;
    let mut sql = format!(
        "SELECT {select} FROM {}{filters}",
        source::<M>(schema, language)?
    );
    let orders = shown_orders::<M>(query.get_order_by());
    if !orders.is_empty() {
        sql.push_str(" ORDER BY ");
        sql.push_str(&build_order_clause(&orders)?);
    }
    if let Some(pagination) = query.get_pagination() {
        sql.push_str(&format!(" LIMIT {}", pagination.limit));
        if pagination.offset > 0 {
            sql.push_str(&format!(" OFFSET {}", pagination.offset));
        }
    }
    Ok((sql, values))
}

/// `query`'s count with `M`'s translatable fields filtered in `language`.
pub(crate) fn build_count_sql_in<M: SqlModel, State>(
    schema: &SchemaName,
    query: &QueryBuilder<M, SqlDb, State>,
    language: &Locale,
) -> Result<(String, Vec<FilterValue>), SqlError> {
    let (filters, values) = where_in(query)?;
    Ok((
        format!(
            "SELECT COUNT(*) FROM {}{filters}",
            source::<M>(schema, language)?
        ),
        values,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::model::{BoundTo, Model};
    use fundamental_db::FilterCondition;

    struct Notice;
    impl Model for Notice {
        const TYPE_NAME: &'static str = "Notice";
    }
    impl BoundTo<SqlDb> for Notice {}
    impl SqlModel for Notice {
        type Id = ();
        fn id(&self) -> &Self::Id {
            &()
        }
        const TABLE: &'static str = "notices";
        const KEY_COLUMNS: &'static [&'static str] = &["organisation_id", "id"];
        fn translatable_columns() -> &'static [&'static str] {
            &["title"]
        }
        fn translation_table() -> Option<&'static str> {
            Some("notices_i18n")
        }
    }

    fn language(tag: &str) -> Locale {
        tag.parse().expect("a language")
    }

    #[test]
    fn a_translatable_field_is_filtered_and_sorted_by_its_text_in_the_language() {
        let query = QueryBuilder::<Notice, SqlDb>::new()
            .filter(Filter::icontains("title", "fiets"))
            .filter(Filter::eq("position", 1))
            .order_by(OrderBy::asc("title"))
            .order_by(OrderBy::asc("id"))
            .limit(11);
        let (sql, values) = build_select_sql_in(
            &SchemaName::new("s").expect("a name"),
            &query,
            &language("nl-BE"),
        )
        .expect("a statement");

        assert_eq!(
            sql,
            "SELECT *, (SELECT json_agg(json_build_array(i.field, i.locale, i.value, \
             i.source_hash, i.translation_hash, i.checked_by, round(extract(epoch FROM \
             i.checked_at) * 1000)::int8)) FROM s.notices_i18n AS i WHERE i.organisation_id = \
             notices.organisation_id AND i.id = notices.id) AS fundamental_translations FROM \
             (SELECT fundamental_row.*, COALESCE((SELECT i.value FROM s.notices_i18n AS i WHERE \
             i.organisation_id = fundamental_row.organisation_id AND i.id = fundamental_row.id \
             AND i.field = 'title' AND i.locale = 'nl-BE'), (SELECT i.value FROM s.notices_i18n \
             AS i WHERE i.organisation_id = fundamental_row.organisation_id AND i.id = \
             fundamental_row.id AND i.field = 'title' AND i.locale = 'nl'), \
             fundamental_row.title) AS fundamental_in_title FROM s.notices AS fundamental_row) \
             AS notices WHERE fundamental_in_title ILIKE $1 AND position = $2 ORDER BY \
             fundamental_in_title ASC, id ASC LIMIT 11"
        );
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn a_page_boundary_on_a_translatable_field_is_read_in_the_language() {
        let boundary = Filter {
            condition: FilterCondition::RowValueGt {
                columns: vec!["title".to_owned(), "id".to_owned()],
                values: vec![FilterValue::String("Fiets".to_owned()), FilterValue::Int(3)],
            },
        };
        let shown = shown_filters::<Notice>(&[boundary]);
        assert!(matches!(
            &shown[0].condition,
            FilterCondition::RowValueGt { columns, .. } if columns == &["fundamental_in_title", "id"]
        ));
    }

    #[test]
    fn a_language_tag_that_is_not_one_is_refused() {
        let forged = Locale::new("nl'; drop table notices; --", None::<&str>);
        let query = QueryBuilder::<Notice, SqlDb>::new();
        assert!(build_count_sql_in(&SchemaName::default(), &query, &forged).is_err());
    }
}
