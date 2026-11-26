//! [`recognise`]: reading one migration statement into [`DdlStatement`].

use std::collections::BTreeMap;

use super::{
    DdlColumn, DdlError, DdlStatement, KeyDeclaration, KeyElement, Sharding, split_top_level,
};

/// Reads one statement, as produced by
/// [`split_statements`](crate::migration::split_statements), into the vocabulary the
/// migration guards judge; of an `ALTER TABLE` with several actions, the first
/// ([`recognise_all`] reads every one).
///
/// A `CREATE TABLE` or `CREATE INDEX` that does not parse is [`DdlError`], never
/// [`DdlStatement::Other`].
///
/// # Errors
/// Returns [`DdlError::Unreadable`] when a `CREATE TABLE`/`CREATE INDEX` cannot be read,
/// and [`DdlError::MalformedKey`] when one of its key elements is outside the grammar the
/// engine accepts.
pub fn recognise(statement: &str) -> Result<DdlStatement, DdlError> {
    let mut all = recognise_all(statement)?;
    Ok(all.swap_remove(0))
}

/// Reads one statement as [`recognise`] does, every action of an `ALTER TABLE` on its own,
/// in order. Never empty.
///
/// # Errors
/// As [`recognise`]; and [`DdlError::Unreadable`] for an `ALTER TABLE` that drops or renames
/// without naming its table or what it drops.
pub fn recognise_all(statement: &str) -> Result<Vec<DdlStatement>, DdlError> {
    let trimmed = statement.trim().trim_end_matches(';').trim();
    let upper = trimmed.to_ascii_uppercase();

    if upper.starts_with("CREATE TABLE") {
        return create_table(trimmed, &upper).map(|one| vec![one]);
    }
    if upper.starts_with("CREATE INDEX") || upper.starts_with("CREATE UNIQUE INDEX") {
        return create_index(trimmed, &upper).map(|one| vec![one]);
    }
    if upper.starts_with("DROP ") || upper.starts_with("TRUNCATE") {
        return Ok(vec![destructive(trimmed, &upper)]);
    }
    if upper.starts_with("ALTER TABLE") {
        return alter_table(trimmed);
    }
    if let Some(data) = data(trimmed, &upper) {
        return Ok(vec![data]);
    }
    if upper.starts_with("CREATE ROLE")
        || (upper.starts_with("DO ") && after_keyword(&upper, "CREATE ROLE").is_some())
    {
        return create_role(trimmed).map(|one| vec![one]);
    }

    Ok(vec![DdlStatement::Other {
        lead: lead(trimmed),
    }])
}

/// The role a `CREATE ROLE` makes and whether a `pg_roles` lookup guards it; of several, the
/// first made without a lookup.
fn create_role(statement: &str) -> Result<DdlStatement, DdlError> {
    // One space between words, so a lookup reads the same however its lines are broken.
    let spaced = statement.split_whitespace().collect::<Vec<_>>().join(" ");
    let upper = spaced.to_ascii_uppercase();
    let mut made = Vec::new();
    let mut from = 0;
    while let Some(at) = after_keyword(&upper[from..], "CREATE ROLE") {
        let end = from + at;
        let start = end - "CREATE ROLE".len();
        let role =
            ident_after(&spaced[start..], &upper[start..], "CREATE ROLE").ok_or_else(|| {
                DdlError::Unreadable {
                    kind: "CREATE ROLE".to_owned(),
                    reason: "no role is named".to_owned(),
                }
            })?;
        let looked_up = ["SELECT", "SELECT 1", "SELECT *"].iter().any(|select| {
            upper[..start].trim_end().ends_with(&format!(
                "IF NOT EXISTS ({select} FROM PG_ROLES WHERE ROLNAME = '{}') THEN",
                role.to_ascii_uppercase()
            ))
        });
        made.push((role, looked_up));
        from = end;
    }
    let (role, looked_up) = made
        .iter()
        .find(|(_, looked_up)| !looked_up)
        .or_else(|| made.first())
        .cloned()
        .ok_or_else(|| DdlError::Unreadable {
            kind: "CREATE ROLE".to_owned(),
            reason: "no role is made".to_owned(),
        })?;
    Ok(DdlStatement::CreateRole { role, looked_up })
}

fn lead(statement: &str) -> String {
    statement
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase()
}

/// The byte offset just past `keyword` in `upper`, matched at word boundaries (a newline or
/// a name holding the keyword must not fool it).
fn after_keyword(upper: &str, keyword: &str) -> Option<usize> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut from = 0;
    while let Some(offset) = upper[from..].find(keyword) {
        let start = from + offset;
        let end = start + keyword.len();
        let before_ok = start == 0 || !upper[..start].chars().next_back().is_some_and(is_word);
        let after_ok = !upper[end..].chars().next().is_some_and(is_word);
        if before_ok && after_ok {
            return Some(end);
        }
        from = end;
    }
    None
}

/// The identifier following `after` in `upper`, read from the original-case `statement`.
fn ident_after(statement: &str, upper: &str, after: &str) -> Option<String> {
    let start = after_keyword(upper, after)?;
    let rest = &statement[start..];
    let rest = strip_leading_keywords(rest, &["IF NOT EXISTS", "IF EXISTS"]);
    let name: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '"'))
        .collect();
    let name = name.trim_matches('"');
    (!name.is_empty()).then(|| unqualified(name))
}

fn strip_leading_keywords<'a>(rest: &'a str, keywords: &[&str]) -> &'a str {
    let trimmed = rest.trim_start();
    let upper = trimmed.to_ascii_uppercase();
    for keyword in keywords {
        if upper.starts_with(keyword) {
            return &trimmed[keyword.len()..];
        }
    }
    trimmed
}

/// `public.widgets` → `widgets`: the guards judge tables, not schemas.
fn unqualified(name: &str) -> String {
    name.rsplit('.').next().unwrap_or(name).to_owned()
}

fn outer_parens(text: &str) -> Option<&str> {
    let open = text.find('(')?;
    let mut depth = 0_usize;
    let mut quote: Option<char> = None;
    for (offset, c) in text[open..].char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '\'' | '"' => quote = Some(c),
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&text[open + 1..open + offset]);
                    }
                }
                _ => {}
            },
        }
    }
    None
}

fn create_table(statement: &str, upper: &str) -> Result<DdlStatement, DdlError> {
    let unreadable = |reason: &str| DdlError::Unreadable {
        kind: "CREATE TABLE".to_owned(),
        reason: reason.to_owned(),
    };

    let table = ident_after(statement, upper, "CREATE TABLE")
        .ok_or_else(|| unreadable("no table name follows CREATE TABLE"))?;
    let body = outer_parens(statement)
        .ok_or_else(|| unreadable("no balanced parenthesised body follows the table name"))?;

    let mut primary_key = None;
    let mut inline_primary_key_columns = Vec::new();
    let mut unique_constraints = Vec::new();
    let mut columns = Vec::new();

    for item in split_top_level(body) {
        let item_upper = item.to_ascii_uppercase();

        if item_upper.starts_with("PRIMARY KEY") {
            let list = outer_parens(&item)
                .ok_or_else(|| unreadable("the PRIMARY KEY constraint has no column list"))?;
            primary_key = Some(key_declaration(list, &table)?);
            continue;
        }
        // A table-level `UNIQUE (a, b)` is a key site with no room to declare sharding.
        if item_upper.starts_with("UNIQUE") {
            let list = outer_parens(&item)
                .ok_or_else(|| unreadable("the UNIQUE constraint has no column list"))?;
            unique_constraints.push(
                split_top_level(list)
                    .into_iter()
                    .map(|c| c.trim_matches('"').to_owned())
                    .collect(),
            );
            continue;
        }
        // A table-level CHECK / CONSTRAINT / FOREIGN KEY is not a key site.
        if starts_with_any(&item_upper, &["CHECK", "CONSTRAINT", "FOREIGN KEY"]) {
            continue;
        }

        let column = item
            .split_whitespace()
            .next()
            .ok_or_else(|| unreadable("a column definition with no name"))?;
        let column = unqualified(column.trim_matches('"'));
        columns.push(column_shape(&column, &item));

        if item_upper.contains("PRIMARY KEY") {
            inline_primary_key_columns.push(column.clone());
        }
        // An inline `UNIQUE` becomes its own unique index, hashed on that column.
        if after_keyword(&item_upper, "UNIQUE").is_some() {
            unique_constraints.push(vec![column]);
        }
    }

    let (colocated, tablets) = placement(statement, body);
    Ok(DdlStatement::CreateTable {
        table,
        primary_key,
        inline_primary_key_columns,
        unique_constraints,
        value_bounds: value_bounds(body),
        columns,
        colocated,
        tablets,
    })
}

const COLUMN_CLAUSES: [&str; 10] = [
    "NOT NULL",
    "NULL",
    "DEFAULT",
    "PRIMARY KEY",
    "UNIQUE",
    "CHECK",
    "REFERENCES",
    "CONSTRAINT",
    "GENERATED",
    "COLLATE",
];

/// The column `name` that `item` (`name TYPE [NOT NULL] [DEFAULT …] …`) defines.
fn column_shape(name: &str, item: &str) -> DdlColumn {
    let after_name = item
        .trim_start()
        .split_once(char::is_whitespace)
        .map_or("", |(_, rest)| rest);
    let upper = after_name.to_ascii_uppercase();
    let clause_at = |from: usize| {
        COLUMN_CLAUSES
            .iter()
            .filter_map(|clause| {
                after_keyword(&upper[from..], clause).map(|end| from + end - clause.len())
            })
            .min()
            .unwrap_or(upper.len())
    };
    let data_type = upper[..clause_at(0)].trim().to_owned();
    let default = after_keyword(&upper, "DEFAULT")
        .map(|start| after_name[start..clause_at(start)].trim().to_owned())
        .or_else(|| after_keyword(&upper, "AS IDENTITY").map(|_| "identity".to_owned()));
    DdlColumn {
        name: name.to_owned(),
        data_type,
        default,
    }
}

/// What a `CREATE TABLE` says of its placement: `WITH (COLOCATION = …)` and
/// `SPLIT INTO <n> TABLETS`.
fn placement(statement: &str, body: &str) -> (Option<bool>, Option<u32>) {
    let start = statement
        .find(body)
        .map_or(statement.len(), |at| at + body.len());
    let tail = statement[start..].to_ascii_uppercase();
    let words: Vec<&str> = tail
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '=' | ',' | ';'))
        .filter(|word| !word.is_empty())
        .collect();
    let colocated = words
        .windows(2)
        .find(|pair| matches!(pair[0], "COLOCATION" | "COLOCATED"))
        .and_then(|pair| match pair[1] {
            "TRUE" | "1" | "ON" => Some(true),
            "FALSE" | "0" | "OFF" => Some(false),
            _ => None,
        });
    let tablets = words
        .windows(4)
        .find(|four| four[0] == "SPLIT" && four[1] == "INTO" && four[3] == "TABLETS")
        .and_then(|four| four[2].parse().ok());
    (colocated, tablets)
}

fn data(statement: &str, upper: &str) -> Option<DdlStatement> {
    let (verb, table_after) = if upper.starts_with("UPDATE ") {
        ("UPDATE", "UPDATE")
    } else if upper.starts_with("DELETE FROM") {
        ("DELETE", "DELETE FROM")
    } else if upper.starts_with("INSERT INTO") {
        ("INSERT", "INSERT INTO")
    } else if upper.starts_with("COPY ") {
        ("COPY", "COPY")
    } else {
        return None;
    };
    let from_select = verb == "INSERT" && after_keyword(upper, "SELECT").is_some();
    if verb == "INSERT" && !from_select {
        return None;
    }
    let table = ident_after(statement, upper, table_after)
        .map(|table| table.trim_start_matches("ONLY").to_owned())
        .unwrap_or_default();
    Some(DdlStatement::Data {
        verb: verb.to_owned(),
        table,
        from_select,
    })
}

fn starts_with_any(upper: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| upper.starts_with(p))
}

fn create_index(statement: &str, upper: &str) -> Result<DdlStatement, DdlError> {
    let unreadable = |reason: &str| DdlError::Unreadable {
        kind: "CREATE INDEX".to_owned(),
        reason: reason.to_owned(),
    };

    let after = if upper.starts_with("CREATE UNIQUE INDEX") {
        "CREATE UNIQUE INDEX"
    } else {
        "CREATE INDEX"
    };
    // `NONCONCURRENTLY` builds with writes held; `CONCURRENTLY` is the default.
    let after_create = after_keyword(upper, after).unwrap_or(after.len());
    let concurrency = upper[after_create..].trim_start();
    let online = !concurrency.starts_with("NONCONCURRENTLY");
    let after = if concurrency.starts_with("NONCONCURRENTLY") {
        "NONCONCURRENTLY"
    } else if concurrency.starts_with("CONCURRENTLY") {
        "CONCURRENTLY"
    } else {
        after
    };
    let name = ident_after(statement, upper, after).ok_or_else(|| unreadable("no index name"))?;
    let table =
        ident_after(statement, upper, "ON").ok_or_else(|| unreadable("no table follows ON"))?;

    // The key list is the first group after `ON <table>`, not a partial index's `WHERE (…)`.
    let on = after_keyword(upper, "ON").ok_or_else(|| unreadable("no ON clause"))?;
    let list = outer_parens(&statement[on..])
        .ok_or_else(|| unreadable("no parenthesised key list follows the table"))?;

    Ok(DdlStatement::CreateIndex {
        key: key_declaration(list, &name)?,
        name,
        table,
        online,
    })
}

fn destructive(statement: &str, upper: &str) -> DdlStatement {
    let verb = if upper.starts_with("TRUNCATE") {
        "TRUNCATE".to_owned()
    } else {
        statement
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_uppercase()
    };
    // A policy or trigger is named on its table, so as `<table>.<name>`.
    let verb_words = verb.split_whitespace().count();
    let mut rest = statement
        .split_whitespace()
        .skip(verb_words)
        .skip_while(|word| {
            matches!(
                word.to_ascii_uppercase().as_str(),
                "TABLE" | "IF" | "EXISTS" | "ONLY" | "CONCURRENTLY"
            )
        });
    let dropped = unqualified(&name(rest.next().unwrap_or("")));
    let object = match (rest.next(), rest.next()) {
        (Some(on), Some(table)) if on.eq_ignore_ascii_case("ON") => {
            format!("{}.{dropped}", unqualified(&name(table)))
        }
        _ => dropped,
    };

    DdlStatement::Destructive { verb, object }
}

fn alter_table(statement: &str) -> Result<Vec<DdlStatement>, DdlError> {
    let (table, actions) = alter_header(statement);
    split_top_level(actions)
        .iter()
        .map(|action| alter_action(statement, &table, action))
        .collect::<Result<Vec<_>, _>>()
        .map(|all| {
            if all.is_empty() {
                vec![DdlStatement::Other {
                    lead: lead(statement),
                }]
            } else {
                all
            }
        })
}

fn alter_header(statement: &str) -> (String, &str) {
    let mut rest = statement["ALTER TABLE".len()..].trim_start();
    loop {
        let upper = rest.to_ascii_uppercase();
        let Some(keyword) = ["IF EXISTS", "ONLY"]
            .into_iter()
            .find(|keyword| upper.starts_with(keyword) && word_ends(&upper, keyword.len()))
        else {
            break;
        };
        rest = rest[keyword.len()..].trim_start();
    }
    let (table, actions) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    (unqualified(&name(table)), actions)
}

/// Each column `statement` renames, as `(table, before, after)`.
pub(crate) fn column_renames(statement: &str) -> Vec<(String, String, String)> {
    let trimmed = statement.trim().trim_end_matches(';').trim();
    if !trimmed.to_ascii_uppercase().starts_with("ALTER TABLE") {
        return Vec::new();
    }
    let (table, actions) = alter_header(trimmed);
    split_top_level(actions)
        .iter()
        .filter_map(|action| {
            let words: Vec<&str> = action.split_whitespace().collect();
            let upper: Vec<String> = words.iter().map(|w| w.to_ascii_uppercase()).collect();
            let from = match upper
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice()
            {
                ["RENAME", "COLUMN", _, "TO", _] => 2,
                ["RENAME", first, "TO", _] if !matches!(*first, "TO" | "CONSTRAINT") => 1,
                _ => return None,
            };
            Some((table.clone(), name(words[from]), name(words[from + 2])))
        })
        .collect()
}

fn word_ends(upper: &str, at: usize) -> bool {
    !upper[at..]
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
}

fn name(word: &str) -> String {
    word.trim_end_matches(';').trim_matches('"').to_owned()
}

fn alter_action(statement: &str, table: &str, action: &str) -> Result<DdlStatement, DdlError> {
    let upper = action.to_ascii_uppercase();
    let words: Vec<&str> = action.split_whitespace().collect();
    let upper_words: Vec<&str> = upper.split_whitespace().collect();
    let named = |verb: &str, from: usize| {
        let at = if upper_words.get(from..from + 2) == Some(&["IF", "EXISTS"][..]) {
            from + 2
        } else {
            from
        };
        match words.get(at).map(|word| name(word)) {
            Some(object) if !object.is_empty() && !table.is_empty() => {
                Ok(DdlStatement::Destructive {
                    verb: verb.to_owned(),
                    object: format!("{table}.{object}"),
                })
            }
            _ => Err(DdlError::Unreadable {
                kind: verb.to_owned(),
                reason: format!("no table, or nothing named, in {statement:?}"),
            }),
        }
    };
    match upper_words.as_slice() {
        ["DROP", "CONSTRAINT", ..] => named("DROP CONSTRAINT", 2),
        ["DROP", "COLUMN", ..] => named("DROP COLUMN", 2),
        ["DROP", ..] => named("DROP COLUMN", 1),
        ["RENAME", "COLUMN", ..] => named("RENAME COLUMN", 2),
        ["RENAME", "TO" | "CONSTRAINT", ..] => Ok(DdlStatement::Other {
            lead: lead(statement),
        }),
        ["RENAME", ..] => named("RENAME COLUMN", 1),
        ["ADD", "COLUMN", ..] => {
            let column = ident_after(action, &upper, "ADD COLUMN").unwrap_or_default();
            let definition = strip_leading_keywords(
                &action[after_keyword(&upper, "ADD COLUMN").unwrap_or(0)..],
                &["IF NOT EXISTS"],
            );
            let shape = column_shape(&column, definition);
            Ok(DdlStatement::AddColumn {
                data_type: shape.data_type,
                default: shape.default,
                table: table.to_owned(),
                column,
                nullable: !upper.contains("NOT NULL"),
                has_default: upper.contains("DEFAULT"),
            })
        }
        ["ADD", "PRIMARY", "KEY", ..] | ["ADD", "CONSTRAINT", _, "PRIMARY", "KEY", ..] => {
            add_primary_key(statement, table, action)
        }
        ["ALTER", "COLUMN", column, rest @ ..] | ["ALTER", column, rest @ ..]
            if *column != "COLUMN" =>
        {
            let column = name(words[upper_words.len() - rest.len() - 1]).to_ascii_lowercase();
            Ok(alter_column(statement, table, column, rest))
        }
        _ => Ok(DdlStatement::Other {
            lead: lead(statement),
        }),
    }
}

fn add_primary_key(statement: &str, table: &str, action: &str) -> Result<DdlStatement, DdlError> {
    let list = after_keyword(&action.to_ascii_uppercase(), "PRIMARY KEY")
        .and_then(|at| outer_parens(&action[at..]))
        .ok_or_else(|| DdlError::Unreadable {
            kind: "ADD PRIMARY KEY".to_owned(),
            reason: format!("the key has no column list in {statement:?}"),
        })?;
    Ok(DdlStatement::AddPrimaryKey {
        table: table.to_owned(),
        key: key_declaration(list, table)?,
    })
}

/// `ALTER [COLUMN] <column> <rest>`: a retype or a `SET NOT NULL`, else
/// [`DdlStatement::Other`].
fn alter_column(statement: &str, table: &str, column: String, rest: &[&str]) -> DdlStatement {
    match rest {
        ["SET", "NOT", "NULL"] => DdlStatement::SetNotNull {
            table: table.to_owned(),
            column,
        },
        ["TYPE", ..] | ["SET", "DATA", "TYPE", ..] => {
            let skip = if rest[0] == "TYPE" { 1 } else { 3 };
            let to = rest[skip..]
                .iter()
                .take_while(|word| !matches!(**word, "USING" | "COLLATE"))
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            DdlStatement::AlterColumnType {
                table: table.to_owned(),
                column,
                to,
            }
        }
        _ => DdlStatement::Other {
            lead: lead(statement),
        },
    }
}

/// Reads a key list, such as `(zone, grant_id) HASH, seq ASC`, into a [`KeyDeclaration`].
///
/// An undeclared element is recorded, not refused; only shapes the engine rejects are
/// [`DdlError::MalformedKey`].
fn key_declaration(list: &str, object: &str) -> Result<KeyDeclaration, DdlError> {
    let mut elements = Vec::new();

    for (position, raw) in split_top_level(list).into_iter().enumerate() {
        let element = key_element(&raw, object)?;
        if element.is_hash() && position > 0 {
            return Err(DdlError::MalformedKey {
                object: object.to_owned(),
                element: raw,
                reason: "only the leading element of a key may be HASH".to_owned(),
            });
        }
        elements.push(element);
    }

    Ok(KeyDeclaration::new(elements))
}

fn key_element(raw: &str, object: &str) -> Result<KeyElement, DdlError> {
    let trimmed = raw.trim();
    let malformed = |reason: &str| DdlError::MalformedKey {
        object: object.to_owned(),
        element: raw.to_owned(),
        reason: reason.to_owned(),
    };

    if trimmed.starts_with('(') {
        let inner = outer_parens(trimmed).ok_or_else(|| malformed("unbalanced parentheses"))?;
        let close = trimmed
            .rfind(')')
            .ok_or_else(|| malformed("unbalanced parentheses"))?;
        let token = trimmed[close + 1..].trim();
        let columns: Vec<String> = split_top_level(inner)
            .into_iter()
            .map(|c| c.trim_matches('"').to_owned())
            .collect();
        if columns.is_empty() {
            return Err(malformed("a parenthesised key component names no column"));
        }
        return Ok(KeyElement::new(columns, Sharding::parse(token)));
    }

    let mut words = trimmed.split_whitespace();
    let column = words
        .next()
        .ok_or_else(|| malformed("an empty key element"))?;
    let token = words.next().unwrap_or("");
    let nulls = match (words.next(), words.next()) {
        (Some(word), Some(place)) if word.eq_ignore_ascii_case("NULLS") => {
            crate::migration::Nulls::parse(place)
        }
        _ => None,
    };

    Ok(KeyElement::new(
        vec![column.trim_matches('"').to_owned()],
        Sharding::parse(token),
    )
    .with_nulls(nulls))
}

/// Upper bounds that `CHECK (col IN ('a', 'b', …))` puts on a column's distinct values.
fn value_bounds(body: &str) -> BTreeMap<String, u64> {
    let mut bounds = BTreeMap::new();
    let upper = body.to_ascii_uppercase();

    let mut search_from = 0;
    while let Some(offset) = upper[search_from..].find("CHECK") {
        let start = search_from + offset;
        search_from = start + "CHECK".len();

        let Some(inner) = outer_parens(&body[start..]) else {
            continue;
        };
        let inner_upper = inner.to_ascii_uppercase();
        let Some(in_at) = inner_upper.find(" IN ") else {
            continue;
        };
        let column: String = inner[..in_at]
            .trim()
            .trim_matches('"')
            .split_whitespace()
            .next_back()
            .unwrap_or("")
            .to_owned();
        if column.is_empty() {
            continue;
        }
        let Some(values) = outer_parens(&inner[in_at..]) else {
            continue;
        };
        let count = split_top_level(values).len() as u64;
        if count > 0 {
            bounds.insert(column, count);
        }
    }

    bounds
}

#[cfg(test)]
mod tests {
    use super::{column_renames, key_declaration, recognise, recognise_all};
    use crate::migration::ddl::{DdlError, DdlStatement, Sharding};

    #[test]
    fn a_declared_composite_hash_key_is_read_exactly() {
        let statement = "CREATE TABLE IF NOT EXISTS permission_grants (\n\
             zone TEXT NOT NULL,\n\
             grant_id TEXT NOT NULL,\n\
             seq INT8 NOT NULL,\n\
             PRIMARY KEY ((zone, grant_id) HASH, seq ASC)\n\
             );";

        let DdlStatement::CreateTable {
            table, primary_key, ..
        } = recognise(statement).expect("readable")
        else {
            panic!("expected a CREATE TABLE");
        };

        assert_eq!(table, "permission_grants");
        let key = primary_key.expect("a primary key");
        assert_eq!(key.hash_columns(), ["zone", "grant_id"]);
        assert_eq!(key.elements()[1].sharding(), Some(Sharding::Asc));
    }

    #[test]
    fn an_undeclared_key_is_reported_as_undeclared_not_as_hashed() {
        // The engine reads this back hashed; the recogniser reports what the migration says: nothing.
        let statement =
            "CREATE TABLE t (zone TEXT NOT NULL, id TEXT NOT NULL, PRIMARY KEY (zone, id));";

        let DdlStatement::CreateTable { primary_key, .. } =
            recognise(statement).expect("a legal statement parses")
        else {
            panic!("expected a CREATE TABLE");
        };

        let key = primary_key.expect("a primary key");
        assert_eq!(key.undeclared().len(), 2);
        assert!(key.hash_columns().is_empty());
    }

    #[test]
    fn an_inline_primary_key_is_named_because_it_cannot_declare_sharding() {
        // An inline key has no way to say how it distributes.
        let statement = "CREATE TABLE audit_records (id UUID PRIMARY KEY, kind TEXT NOT NULL);";

        let DdlStatement::CreateTable {
            primary_key,
            inline_primary_key_columns,
            ..
        } = recognise(statement).expect("readable")
        else {
            panic!("expected a CREATE TABLE");
        };

        assert!(primary_key.is_none());
        assert_eq!(inline_primary_key_columns, ["id"]);
    }

    #[test]
    fn both_spellings_of_unique_are_key_sites_the_statement_cannot_annotate() {
        // A `UNIQUE` constraint cannot declare sharding, so both spellings must reach the guard.
        let statement = "CREATE TABLE t (\n\
             id UUID NOT NULL,\n\
             commitment TEXT NOT NULL UNIQUE,\n\
             zone TEXT NOT NULL,\n\
             offer_id TEXT NOT NULL,\n\
             PRIMARY KEY (id HASH),\n\
             UNIQUE (zone, offer_id)\n\
             );";

        let DdlStatement::CreateTable {
            unique_constraints, ..
        } = recognise(statement).expect("readable")
        else {
            panic!("expected a CREATE TABLE");
        };

        assert_eq!(unique_constraints.len(), 2);
        assert!(unique_constraints.contains(&vec!["zone".to_owned(), "offer_id".to_owned()]));
        assert!(unique_constraints.contains(&vec!["commitment".to_owned()]));
    }

    #[test]
    fn a_check_constraint_bounds_a_columns_cardinality() {
        let statement = "CREATE TABLE t (\n\
             kind TEXT NOT NULL CHECK (kind IN ('issuance', 'grant', 'disclosure')),\n\
             id TEXT NOT NULL,\n\
             PRIMARY KEY (id HASH)\n\
             );";

        let DdlStatement::CreateTable { value_bounds, .. } =
            recognise(statement).expect("readable")
        else {
            panic!("expected a CREATE TABLE");
        };

        assert_eq!(value_bounds.get("kind"), Some(&3));
    }

    #[test]
    fn a_partial_index_key_is_read_without_swallowing_its_where_clause() {
        let statement = "CREATE INDEX IF NOT EXISTS audit_outboxes_due\n\
             ON audit_outboxes (next_attempt_at ASC)\n\
             WHERE leaf_index IS NULL;";

        let DdlStatement::CreateIndex {
            name, table, key, ..
        } = recognise(statement).expect("readable")
        else {
            panic!("expected a CREATE INDEX");
        };

        assert_eq!(name, "audit_outboxes_due");
        assert_eq!(table, "audit_outboxes");
        assert_eq!(key.columns(), ["next_attempt_at"]);
        assert!(key.hash_columns().is_empty());
    }

    #[test]
    fn a_unique_index_is_a_key_site_too() {
        let statement = "CREATE UNIQUE INDEX u ON t (a ASC, b DESC);";

        let DdlStatement::CreateIndex { key, .. } = recognise(statement).expect("readable") else {
            panic!("expected a CREATE INDEX");
        };

        assert_eq!(key.elements()[1].sharding(), Some(Sharding::Desc));
    }

    #[test]
    fn hash_after_the_leading_element_is_refused_as_the_engine_refuses_it() {
        let statement = "CREATE TABLE t (a TEXT NOT NULL, b TEXT NOT NULL, \
                         PRIMARY KEY (a ASC, b HASH));";

        let error = recognise(statement).expect_err("the engine rejects this too");
        assert!(matches!(error, DdlError::MalformedKey { .. }), "{error:?}");
    }

    #[test]
    fn destructive_verbs_are_recognised_as_such() {
        for (sql, verb) in [
            ("DROP TABLE IF EXISTS widgets;", "DROP TABLE"),
            ("TRUNCATE widgets;", "TRUNCATE"),
        ] {
            let DdlStatement::Destructive { verb: got, object } = recognise(sql).expect("readable")
            else {
                panic!("expected Destructive for {sql}");
            };
            assert_eq!(got, verb);
            assert_eq!(object, "widgets");
        }
    }

    #[test]
    fn an_alter_that_drops_or_renames_is_destructive_and_names_table_and_object() {
        for (sql, verb, object) in [
            (
                "ALTER TABLE widgets DROP COLUMN IF EXISTS legacy;",
                "DROP COLUMN",
                "widgets.legacy",
            ),
            (
                "alter table public.widgets drop column legacy",
                "DROP COLUMN",
                "widgets.legacy",
            ),
            (
                "ALTER TABLE widgets\n    DROP CONSTRAINT IF EXISTS widgets_kind;",
                "DROP CONSTRAINT",
                "widgets.widgets_kind",
            ),
            (
                "ALTER TABLE widgets RENAME COLUMN state TO visibility;",
                "RENAME COLUMN",
                "widgets.state",
            ),
            (
                "ALTER TABLE IF EXISTS ONLY widgets DROP legacy;",
                "DROP COLUMN",
                "widgets.legacy",
            ),
        ] {
            assert_eq!(
                recognise(sql).expect("readable"),
                DdlStatement::Destructive {
                    verb: verb.to_owned(),
                    object: object.to_owned(),
                },
                "{sql}"
            );
        }
    }

    #[test]
    fn every_action_of_one_alter_is_judged() {
        let all = recognise_all(
            "ALTER TABLE widgets ADD COLUMN owner TEXT, DROP COLUMN legacy, \
             ADD CONSTRAINT widgets_kind CHECK (kind IN ('a', 'b'));",
        )
        .expect("readable");

        assert_eq!(all.len(), 3, "{all:#?}");
        assert!(matches!(&all[0], DdlStatement::AddColumn { column, .. } if column == "owner"));
        assert_eq!(
            all[1],
            DdlStatement::Destructive {
                verb: "DROP COLUMN".to_owned(),
                object: "widgets.legacy".to_owned(),
            }
        );
        assert!(matches!(&all[2], DdlStatement::Other { .. }));
    }

    #[test]
    fn a_rename_names_the_column_before_and_after() {
        assert_eq!(
            column_renames("ALTER TABLE public.widgets RENAME COLUMN \"state\" TO visibility;"),
            [(
                "widgets".to_owned(),
                "state".to_owned(),
                "visibility".to_owned()
            )]
        );
        assert_eq!(
            column_renames("alter table widgets rename state to visibility"),
            [(
                "widgets".to_owned(),
                "state".to_owned(),
                "visibility".to_owned()
            )]
        );
        assert!(column_renames("ALTER TABLE widgets RENAME TO gadgets;").is_empty());
        assert!(column_renames("DROP TABLE widgets;").is_empty());
    }

    #[test]
    fn a_drop_names_what_it_drops_past_its_keywords_and_a_policy_with_its_table() {
        for (sql, verb, object) in [
            (
                "DROP INDEX CONCURRENTLY IF EXISTS widgets_by_kind;",
                "DROP INDEX",
                "widgets_by_kind",
            ),
            (
                "DROP POLICY IF EXISTS widgets_read_by_www ON public.widgets;",
                "DROP POLICY",
                "widgets.widgets_read_by_www",
            ),
            (
                "drop policy widgets_written on widgets",
                "DROP POLICY",
                "widgets.widgets_written",
            ),
        ] {
            assert_eq!(
                recognise(sql).expect("readable"),
                DdlStatement::Destructive {
                    verb: verb.to_owned(),
                    object: object.to_owned(),
                },
                "{sql}"
            );
        }
    }

    #[test]
    fn an_alter_that_keeps_every_name_is_not_destructive() {
        for sql in [
            "ALTER TABLE widgets ADD CONSTRAINT widgets_kind CHECK (kind <> 'drop column');",
            "ALTER TABLE widgets ENABLE ROW LEVEL SECURITY;",
            "ALTER TABLE widgets ALTER COLUMN note DROP DEFAULT;",
            "ALTER TABLE widgets ALTER COLUMN note DROP NOT NULL;",
        ] {
            assert!(
                !matches!(
                    recognise(sql).expect("readable"),
                    DdlStatement::Destructive { .. }
                ),
                "{sql}"
            );
        }
    }

    #[test]
    fn a_non_nullable_add_column_without_a_default_is_visible_to_the_safety_rule() {
        let DdlStatement::AddColumn {
            column,
            nullable,
            has_default,
            ..
        } = recognise("ALTER TABLE widgets ADD COLUMN owner TEXT NOT NULL;").expect("readable")
        else {
            panic!("expected AddColumn");
        };

        assert_eq!(column, "owner");
        assert!(!nullable);
        assert!(!has_default);
    }

    #[test]
    fn a_tables_columns_are_read_with_their_types_and_defaults() {
        let statement = "CREATE TABLE orders (\n\
             id UUID NOT NULL DEFAULT gen_random_uuid(),\n\
             total NUMERIC(10, 2) NOT NULL,\n\
             at TIMESTAMPTZ NOT NULL DEFAULT now(),\n\
             seq INT8 GENERATED ALWAYS AS IDENTITY,\n\
             note TEXT,\n\
             PRIMARY KEY (id HASH)\n\
             );";

        let DdlStatement::CreateTable { columns, .. } = recognise(statement).expect("readable")
        else {
            panic!("expected a CREATE TABLE");
        };

        let read: Vec<(&str, &str, Option<&str>)> = columns
            .iter()
            .map(|c| (c.name.as_str(), c.data_type.as_str(), c.default.as_deref()))
            .collect();
        assert_eq!(
            read,
            [
                ("id", "UUID", Some("gen_random_uuid()")),
                ("total", "NUMERIC(10, 2)", None),
                ("at", "TIMESTAMPTZ", Some("now()")),
                ("seq", "INT8", Some("identity")),
                ("note", "TEXT", None),
            ]
        );
    }

    #[test]
    fn where_a_table_is_placed_is_read_after_its_body() {
        let DdlStatement::CreateTable {
            colocated, tablets, ..
        } = recognise(
            "CREATE TABLE t (id UUID NOT NULL, PRIMARY KEY (id HASH)) \
             WITH (COLOCATION = false) SPLIT INTO 8 TABLETS;",
        )
        .expect("readable")
        else {
            panic!("expected a CREATE TABLE");
        };
        assert_eq!((colocated, tablets), (Some(false), Some(8)));

        let DdlStatement::CreateTable {
            colocated, tablets, ..
        } = recognise("CREATE TABLE t (id UUID NOT NULL, PRIMARY KEY (id HASH));")
            .expect("readable")
        else {
            panic!("expected a CREATE TABLE");
        };
        assert_eq!((colocated, tablets), (None, None));
    }

    #[test]
    fn an_index_built_with_writes_held_is_not_online() {
        let online = |statement: &str| match recognise(statement).expect("readable") {
            DdlStatement::CreateIndex { online, name, .. } => (name, online),
            other => panic!("expected a CREATE INDEX, got {other:?}"),
        };

        assert_eq!(
            online("CREATE INDEX NONCONCURRENTLY o_by_at ON orders (at ASC);"),
            ("o_by_at".to_owned(), false)
        );
        assert_eq!(
            online("CREATE INDEX CONCURRENTLY o_by_at ON orders (at ASC);"),
            ("o_by_at".to_owned(), true)
        );
        assert_eq!(
            online("CREATE INDEX IF NOT EXISTS o_by_at ON orders (at ASC);"),
            ("o_by_at".to_owned(), true)
        );
    }

    #[test]
    fn a_column_retyped_or_made_not_null_is_read_with_its_table() {
        let all = recognise_all(
            "ALTER TABLE orders ALTER COLUMN total TYPE INT4 USING total::int4, \
             ALTER note SET DATA TYPE VARCHAR(20), ALTER COLUMN note SET NOT NULL, \
             ALTER COLUMN note SET DEFAULT ''",
        )
        .expect("readable");

        assert_eq!(
            all,
            [
                DdlStatement::AlterColumnType {
                    table: "orders".to_owned(),
                    column: "total".to_owned(),
                    to: "INT4".to_owned(),
                },
                DdlStatement::AlterColumnType {
                    table: "orders".to_owned(),
                    column: "note".to_owned(),
                    to: "VARCHAR(20)".to_owned(),
                },
                DdlStatement::SetNotNull {
                    table: "orders".to_owned(),
                    column: "note".to_owned(),
                },
                DdlStatement::Other {
                    lead: "ALTER TABLE".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_primary_key_added_to_a_table_is_read_with_its_table() {
        for (statement, list) in [
            ("ALTER TABLE orders ADD PRIMARY KEY (id HASH);", "id HASH"),
            (
                "ALTER TABLE public.orders ADD CONSTRAINT orders_pkey PRIMARY KEY (id ASC);",
                "id ASC",
            ),
        ] {
            assert_eq!(
                recognise(statement).expect("readable"),
                DdlStatement::AddPrimaryKey {
                    table: "orders".to_owned(),
                    key: key_declaration(list, "orders").expect("a key"),
                },
                "{statement}"
            );
        }
        assert!(recognise("ALTER TABLE orders ADD PRIMARY KEY;").is_err());
        assert!(matches!(
            recognise("ALTER TABLE orders ADD CONSTRAINT orders_total CHECK (total > 0);")
                .expect("readable"),
            DdlStatement::Other { .. }
        ));
    }

    #[test]
    fn a_statement_that_moves_rows_is_data_and_a_seed_is_not() {
        let data = |statement: &str| match recognise(statement).expect("readable") {
            DdlStatement::Data {
                verb,
                table,
                from_select,
            } => Some((verb, table, from_select)),
            _ => None,
        };

        assert_eq!(
            data("UPDATE orders SET note = '' WHERE note IS NULL;"),
            Some(("UPDATE".to_owned(), "orders".to_owned(), false))
        );
        assert_eq!(
            data("DELETE FROM public.orders WHERE at < now();"),
            Some(("DELETE".to_owned(), "orders".to_owned(), false))
        );
        assert_eq!(
            data("INSERT INTO totals (id) SELECT id FROM orders;"),
            Some(("INSERT".to_owned(), "totals".to_owned(), true))
        );
        assert_eq!(
            data("COPY orders FROM '/tmp/orders.csv';"),
            Some(("COPY".to_owned(), "orders".to_owned(), false))
        );
        assert_eq!(data("INSERT INTO kinds (name) VALUES ('news');"), None);
    }

    #[test]
    fn statements_the_guards_do_not_judge_pass_through() {
        let DdlStatement::Other { lead } =
            recognise("GRANT SELECT ON widgets TO www;").expect("readable")
        else {
            panic!("expected Other");
        };
        assert_eq!(lead, "GRANT SELECT");
    }

    #[test]
    fn an_unreadable_create_table_is_an_error_not_a_pass() {
        let error = recognise("CREATE TABLE widgets").expect_err("no body to read");
        assert!(matches!(error, DdlError::Unreadable { .. }), "{error:?}");
    }

    fn made(statement: &str) -> (String, bool) {
        let DdlStatement::CreateRole { role, looked_up } = recognise(statement).expect("readable")
        else {
            panic!("expected CreateRole from {statement}");
        };
        (role, looked_up)
    }

    #[test]
    fn a_role_made_where_pg_roles_lacks_it_is_looked_up_first() {
        assert_eq!(
            made(
                "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'www') THEN \
                 CREATE ROLE www NOLOGIN; END IF; EXCEPTION WHEN duplicate_object THEN NULL; END $$;"
            ),
            ("www".to_owned(), true)
        );
        assert_eq!(
            made(
                "do $$\nbegin\n  if not exists (select 1 from pg_roles where rolname = 'www') then\n    \
                 create role www nologin;\n  end if;\nend\n$$;"
            ),
            ("www".to_owned(), true)
        );
    }

    #[test]
    fn a_role_made_without_asking_is_not_looked_up() {
        // The server refuses CREATE ROLE without the privilege before checking for a duplicate.
        assert_eq!(made("CREATE ROLE www NOLOGIN;"), ("www".to_owned(), false));
        assert_eq!(
            made(
                "DO $$ BEGIN CREATE ROLE www NOLOGIN; EXCEPTION WHEN duplicate_object THEN NULL; END $$;"
            ),
            ("www".to_owned(), false)
        );
    }

    #[test]
    fn a_lookup_that_does_not_guard_the_role_is_no_lookup() {
        assert_eq!(
            made("DO $$ BEGIN PERFORM 1 FROM pg_roles; CREATE ROLE www NOLOGIN; END $$;"),
            ("www".to_owned(), false)
        );
        assert_eq!(
            made(
                "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'reader') THEN \
                 CREATE ROLE www NOLOGIN; END IF; END $$;"
            ),
            ("www".to_owned(), false)
        );
        assert_eq!(
            made(
                "DO $$ BEGIN IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'www') THEN \
                 CREATE ROLE www NOLOGIN; END IF; END $$;"
            ),
            ("www".to_owned(), false)
        );
    }
}
