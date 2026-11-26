//! Every way a stored row can refuse to decode is a typed `SqlError::RowRefused`, naming its column and why.
//!
//! Two models, because row binding refuses to bind an enum or a JSON list.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use fundamental_core::outbox::RefusalKind;
use fundamental_core::secrets::{Secret, SecretString};
use fundamental_db::{DatabaseError, KeyedRepository, Repository};
use fundamental_macros::Model;
use fundamental_sql::{SqlConnections, SqlError, SqlPool};
use fundamental_test_containers::sql::SqlSchema;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Colour {
    Red,
    Blue,
}

impl fmt::Display for Colour {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Red => "red",
            Self::Blue => "blue",
        })
    }
}

impl FromStr for Colour {
    type Err = String;

    fn from_str(token: &str) -> Result<Self, Self::Err> {
        match token {
            "red" => Ok(Self::Red),
            "blue" => Ok(Self::Blue),
            other => Err(format!("no colour named {other}")),
        }
    }
}

#[derive(Clone, Debug, Model)]
#[model(sql)]
struct Probe {
    #[model(id)]
    id: uuid::Uuid,
    #[model(enum)]
    colour: Colour,
    tags: Vec<String>,
    count: i64,
}

#[derive(Clone, Debug, Model)]
#[model(sql)]
struct SealedProbe {
    #[model(id)]
    id: uuid::Uuid,
    label: String,
    #[model(encrypted = "pii")]
    token: SecretString,
}

#[derive(Clone, Debug, Model)]
#[model(sql)]
struct SparseProbe {
    #[model(id)]
    id: uuid::Uuid,
    count: i64,
}

async fn a_schema_with(ddl: &str) -> (SqlSchema, SqlPool) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    pool.execute_ddl(ddl).await.expect("create the table");
    (schema, pool)
}

fn key_manager() -> fundamental_encryption::SharedKeyManager {
    Arc::new(
        fundamental_encryption::KeyManager::builder()
            .with_local_key("pii", &[7u8; 32])
            .build()
            .expect("build key manager"),
    )
}

async fn tamper(pool: &SqlPool, id: uuid::Uuid, sql: &str) {
    pool.execute(&sql.replace("{id}", &id.to_string()))
        .await
        .expect("tamper with the stored row");
}

fn refusal(error: &DatabaseError) -> Option<(&'static str, RefusalKind)> {
    let DatabaseError::Other(inner) = error else {
        return None;
    };
    match inner.downcast_ref::<SqlError>() {
        Some(SqlError::RowRefused { column, kind, .. }) => Some((*column, *kind)),
        _ => None,
    }
}

async fn a_stored_probe() -> (SqlSchema, SqlPool, SqlConnections, uuid::Uuid) {
    let (schema, pool) = a_schema_with(
        "CREATE TABLE IF NOT EXISTS probes \
         (id UUID PRIMARY KEY, colour TEXT NOT NULL, tags JSONB NOT NULL, count BIGINT)",
    )
    .await;
    let connections = SqlConnections::new(Some(pool.clone()), HashMap::new());
    let id = uuid::Uuid::new_v4();
    connections
        .repository::<Probe>()
        .expect("repository")
        .insert(&Probe {
            id,
            colour: Colour::Blue,
            tags: vec!["first".to_owned(), "second".to_owned()],
            count: 3,
        })
        .await
        .expect("insert an intact probe");
    (schema, pool, connections, id)
}

async fn a_stored_sealed_probe() -> (SqlSchema, SqlPool, SqlConnections, uuid::Uuid) {
    let (schema, pool) = a_schema_with(
        "CREATE TABLE IF NOT EXISTS sealed_probes \
         (id UUID PRIMARY KEY, label TEXT NOT NULL, token TEXT NOT NULL)",
    )
    .await;
    let connections =
        SqlConnections::new(Some(pool.clone()), HashMap::new()).with_key_manager(key_manager());
    let id = uuid::Uuid::new_v4();
    connections
        .repository::<SealedProbe>()
        .expect("repository")
        .insert(&SealedProbe {
            id,
            label: "probe".to_owned(),
            token: Secret::new("tok-1".to_owned()),
        })
        .await
        .expect("insert an intact sealed probe");
    (schema, pool, connections, id)
}

#[tokio::test]
async fn intact_rows_decode_on_every_path() {
    let (_schema, _pool, connections, id) = a_stored_probe().await;
    let probe = connections
        .repository::<Probe>()
        .expect("repository")
        .find(&id)
        .await
        .expect("an intact row reads")
        .expect("and is present");
    assert_eq!(probe.colour, Colour::Blue);
    assert_eq!(probe.tags, ["first", "second"]);
    assert_eq!(probe.count, 3);

    let (_sealed_schema, _sealed_pool, sealed, sealed_id) = a_stored_sealed_probe().await;
    let sealed_probe = sealed
        .repository::<SealedProbe>()
        .expect("repository")
        .find(&sealed_id)
        .await
        .expect("an intact sealed row reads")
        .expect("and is present");
    assert_eq!(sealed_probe.token.expose().as_str(), "tok-1");
}

#[tokio::test]
async fn an_unknown_enum_token_is_refused_as_a_value_of_its_column() {
    let (_schema, pool, connections, id) = a_stored_probe().await;
    tamper(
        &pool,
        id,
        "UPDATE probes SET colour = 'mauve' WHERE id = '{id}'",
    )
    .await;
    let error = connections
        .repository::<Probe>()
        .expect("repository")
        .find(&id)
        .await
        .expect_err("a token the enum does not know must not decode");
    assert_eq!(
        refusal(&error),
        Some(("colour", RefusalKind::ValueRefused)),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_malformed_json_list_is_refused_as_a_value_of_its_column() {
    let (_schema, pool, connections, id) = a_stored_probe().await;
    tamper(
        &pool,
        id,
        "UPDATE probes SET tags = '42'::jsonb WHERE id = '{id}'",
    )
    .await;
    let error = connections
        .repository::<Probe>()
        .expect("repository")
        .find(&id)
        .await
        .expect_err("a JSON number is not a list of strings");
    assert_eq!(
        refusal(&error),
        Some(("tags", RefusalKind::ValueRefused)),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_null_in_a_required_column_is_refused_as_a_value_of_its_column() {
    let (_schema, pool, connections, id) = a_stored_probe().await;
    tamper(
        &pool,
        id,
        "UPDATE probes SET count = NULL WHERE id = '{id}'",
    )
    .await;
    let error = connections
        .repository::<Probe>()
        .expect("repository")
        .find(&id)
        .await
        .expect_err("a NULL cannot become an i64");
    assert_eq!(
        refusal(&error),
        Some(("count", RefusalKind::ValueRefused)),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_malformed_envelope_is_refused_as_an_envelope() {
    let (_schema, pool, connections, id) = a_stored_sealed_probe().await;
    tamper(
        &pool,
        id,
        "UPDATE sealed_probes SET token = 'not an envelope' WHERE id = '{id}'",
    )
    .await;
    let error = connections
        .repository::<SealedProbe>()
        .expect("repository")
        .find(&id)
        .await
        .expect_err("text that is not an envelope must not decrypt");
    assert_eq!(
        refusal(&error),
        Some(("token", RefusalKind::EnvelopeRefused)),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_missing_column_is_the_schemas_fault_not_a_refusal() {
    let (_schema, pool) =
        a_schema_with("CREATE TABLE IF NOT EXISTS sparse_probes (id UUID PRIMARY KEY)").await;
    let id = uuid::Uuid::new_v4();
    pool.execute(&format!("INSERT INTO sparse_probes (id) VALUES ('{id}')"))
        .await
        .expect("insert a sparse row");
    let error = SqlConnections::new(Some(pool), HashMap::new())
        .repository::<SparseProbe>()
        .expect("repository")
        .find(&id)
        .await
        .expect_err("a column the table does not have cannot be read");
    assert_eq!(
        refusal(&error),
        None,
        "schema drift must stay a whole-read failure, never a row's refusal: {error:?}"
    );
}
