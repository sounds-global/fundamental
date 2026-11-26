use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use fundamental_auth::AuthenticatedPrincipal;
use fundamental_core::error::PayloadRejection;
use fundamental_core::validate::Validate;
use fundamental_db::Filter;
use fundamental_sync::{CaptureTable, ServerSyncable, SliceReader};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tracing::{Span, field, instrument};

use crate::slice::{ResolvedSlice, SliceContext, SliceError};

type ResolveFuture<'a> = Pin<Box<dyn Future<Output = Result<Filter, SliceError>> + Send + 'a>>;

/// The server's registry of slices: named, parameterized queries over syncable base tables.
///
/// The predicate is server code and the principal is injected, so a client only names a slice
/// and supplies validated parameters.
///
///
/// ```
/// # extern crate fundamental_sync_server;
/// # extern crate fundamental_core;
/// # extern crate fundamental_offline;
/// # extern crate fundamental_sync;
/// # extern crate fundamental_db;
/// # extern crate fundamental_macros;
/// use fundamental_db::Filter;
/// use fundamental_macros::Model;
/// use fundamental_sync_server::{NoParams, SliceRegistry};
///
/// #[derive(Clone, Debug, Model)]
/// #[model(offline, syncable, table = "animals")]
/// struct Animal {
///     #[model(id)]
///     id: String,
///     name: String,
///     owner_id: String,
/// }
///
/// // "my animals" — scoped to the principal, no client parameters.
/// let registry = SliceRegistry::new().define::<Animal, NoParams, _, _>(
///     "mine",
///     |ctx, _params| async move {
///         Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
///     },
/// );
/// assert!(registry.contains("mine"));
/// ```
#[derive(Default)]
#[must_use]
pub struct SliceRegistry {
    slices: HashMap<Arc<str>, Box<dyn SliceResolver>>,
}

impl SliceRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a slice `name` over base table `M`, resolved by `closure`.
    ///
    /// The closure receives a [`SliceContext`] and the validated parameters, and returns the
    /// [`Filter`] that scopes the slice over `M`.
    pub fn define<M, P, F, Fut>(mut self, name: impl Into<String>, closure: F) -> Self
    where
        M: ServerSyncable + Send + Sync + 'static,
        P: DeserializeOwned + Validate + Send + 'static,
        F: Fn(SliceContext, P) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Filter, SliceError>> + Send,
    {
        self.slices.insert(
            Arc::from(name.into()),
            Box::new(DefinedSlice {
                closure,
                _marker: PhantomData::<fn(M, P)>,
            }),
        );
        self
    }

    /// Whether a slice `name` is registered.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.slices.contains_key(name)
    }

    /// Every distinct base table a registered slice draws from, owner-less ones included.
    #[must_use]
    pub fn slice_tables(&self) -> Vec<&'static str> {
        let mut seen = std::collections::BTreeSet::new();
        for resolver in self.slices.values() {
            seen.insert(resolver.table());
        }
        seen.into_iter().collect()
    }

    /// The [`CaptureTable`] for every served table with an owner column, one per table.
    ///
    /// Pass it to
    /// [`CqlSliceStore::new`](https://docs.rs/fundamental/latest/fundamental/cql/struct.CqlSliceStore.html).
    #[must_use]
    pub fn capture_tables(&self) -> Vec<CaptureTable> {
        let mut seen = HashMap::new();
        for resolver in self.slices.values() {
            if let Some(capture) = resolver.capture_table() {
                seen.entry(resolver.table()).or_insert(capture);
            }
        }
        seen.into_values().collect()
    }

    /// Resolves a subscription to `name` for `principal` with `params` into the base table and
    /// its scoped [`Filter`].
    ///
    /// `reader` is the read view a relational slice reads a second model through.
    ///
    /// # Errors
    /// - [`SliceError::Unknown`] if no slice `name` is registered.
    /// - [`SliceError::Params`] / [`SliceError::Invalid`] if `params` do not
    ///   deserialize into / validate as the slice's parameter type.
    /// - [`SliceError::Forbidden`] (or any error the closure returns) if the
    ///   principal is not authorized for the slice.
    ///
    /// Records the declared name once it matches, nothing of one that does not.
    #[instrument(name = "sync.slice.resolve", skip_all, fields(slice = field::Empty))]
    pub async fn resolve(
        &self,
        name: &str,
        principal: &AuthenticatedPrincipal,
        params: Value,
        reader: Arc<dyn SliceReader>,
    ) -> Result<ResolvedSlice, SliceError> {
        let (declared, slice) = self.slices.get_key_value(name).ok_or(SliceError::Unknown)?;
        Span::current().record("slice", &**declared);
        let filter = slice
            .resolve(SliceContext::new(principal.clone(), reader), params)
            .await?;
        Ok(ResolvedSlice {
            name: Arc::clone(declared),
            table: slice.table(),
            key_columns: slice.key_columns(),
            data_columns: slice.data_columns(),
            bidirectional: slice.bidirectional(),
            filter,
        })
    }
}

trait SliceResolver: Send + Sync {
    fn table(&self) -> &'static str;

    fn key_columns(&self) -> &'static [&'static str];

    fn partition_columns(&self) -> &'static [&'static str];

    fn data_columns(&self) -> &'static [&'static str];

    fn bidirectional(&self) -> bool;

    fn owner_column(&self) -> Option<&'static str>;

    fn capture_table(&self) -> Option<CaptureTable> {
        self.owner_column().map(|owner| {
            CaptureTable::new(
                self.table(),
                self.key_columns().iter().copied(),
                self.partition_columns().iter().copied(),
                self.data_columns().iter().copied(),
                owner,
            )
        })
    }

    fn resolve<'a>(&'a self, ctx: SliceContext, params: Value) -> ResolveFuture<'a>;
}

struct DefinedSlice<M, P, F> {
    closure: F,
    _marker: PhantomData<fn(M, P)>,
}

impl<M, P, F, Fut> SliceResolver for DefinedSlice<M, P, F>
where
    M: ServerSyncable + Send + Sync + 'static,
    P: DeserializeOwned + Validate + Send + 'static,
    F: Fn(SliceContext, P) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Filter, SliceError>> + Send,
{
    fn table(&self) -> &'static str {
        M::TABLE
    }

    fn key_columns(&self) -> &'static [&'static str] {
        M::KEY_COLUMNS
    }

    fn partition_columns(&self) -> &'static [&'static str] {
        M::PARTITION_COLUMNS
    }

    fn data_columns(&self) -> &'static [&'static str] {
        M::data_columns()
    }

    fn bidirectional(&self) -> bool {
        M::BIDIRECTIONAL
    }

    fn owner_column(&self) -> Option<&'static str> {
        M::OWNER_COLUMN
    }

    fn resolve<'a>(&'a self, ctx: SliceContext, params: Value) -> ResolveFuture<'a> {
        Box::pin(async move {
            // An absent payload is `null`; read it as `{}` so a `NoParams` slice accepts it.
            let params = if params.is_null() {
                Value::Object(serde_json::Map::new())
            } else {
                params
            };
            let typed: P = serde_json::from_value(params)
                .map_err(|e| SliceError::Params(PayloadRejection::of(&e)))?;
            typed.validate().map_err(|errors| {
                let codes: Vec<&str> = errors.errors().iter().map(|e| e.code.as_str()).collect();
                SliceError::Invalid(codes.join(", "))
            })?;
            (self.closure)(ctx, typed).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoParams;
    use fundamental_auth::UserId;
    use fundamental_core::validate::{ValidationErrors, ValidationResult};
    use fundamental_db::{FilterCondition, FilterValue, RecordKey};
    use fundamental_macros::Model;
    use fundamental_sync::{SyncError, SyncMeta, SyncRecord};
    use serde::Deserialize;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[derive(Clone, Debug, Model)]
    #[model(offline, syncable, table = "animals")]
    struct Animal {
        #[model(id)]
        id: String,
        name: String,
        #[model(owner)]
        owner_id: String,
    }

    #[derive(Clone, Debug, Model)]
    #[model(offline, syncable, table = "announcements")]
    struct Announcement {
        #[model(id)]
        id: String,
        body: String,
    }

    #[derive(Clone, Debug, Model)]
    #[model(offline, syncable, table = "memberships")]
    struct Membership {
        #[model(id)]
        id: String,
        #[model(owner)]
        user_id: String,
        channel_id: String,
    }

    /// A read view that returns preset rows for any query.
    struct FakeReader {
        rows: Vec<SyncRecord>,
    }

    impl SliceReader for FakeReader {
        fn read_records<'a>(
            &'a self,
            _table: &'a str,
            _data_columns: &'a [&'a str],
            _key_columns: &'a [&'a str],
            _filters: &'a [Filter],
        ) -> Pin<Box<dyn Future<Output = Result<Vec<SyncRecord>, SyncError>> + Send + 'a>> {
            let rows = self.rows.clone();
            Box::pin(async move { Ok(rows) })
        }
    }

    fn reader() -> Arc<dyn SliceReader> {
        Arc::new(FakeReader { rows: Vec::new() })
    }

    fn membership_reader(channel_ids: &[&str]) -> Arc<dyn SliceReader> {
        let rows = channel_ids
            .iter()
            .map(|cid| SyncRecord {
                id: RecordKey::single(format!("m-{cid}")),
                meta: SyncMeta::new(1, 1),
                payload: BTreeMap::from([("channel_id".to_string(), (*cid).to_string())]),
            })
            .collect();
        Arc::new(FakeReader { rows })
    }

    fn principal(user: &str) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            user_id: UserId::new(user),
            device_id: None,
            scopes: Vec::new(),
            expires_at: 0,
        }
    }

    #[derive(Deserialize)]
    struct ChannelParams {
        channel_id: String,
    }
    impl Validate for ChannelParams {
        fn validate(&self) -> ValidationResult<()> {
            Ok(())
        }
    }

    fn eq_column(filter: &Filter) -> (&str, &str) {
        match &filter.condition {
            FilterCondition::Eq { column, value } => (column.as_str(), value_text(value)),
            other => panic!("expected an Eq condition, got {other:?}"),
        }
    }

    fn value_text(value: &FilterValue) -> &str {
        match value {
            FilterValue::String(s) => s.as_str(),
            other => panic!("expected a string filter value, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mine_slice_scopes_to_the_principal() {
        let registry = SliceRegistry::new()
            .define::<Animal, NoParams, _, _>("mine", |ctx, _params| async move {
                Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
            });

        let resolved = registry
            .resolve("mine", &principal("alice"), Value::Null, reader())
            .await
            .expect("mine resolves");
        assert_eq!(resolved.table, "animals");
        assert_eq!(resolved.key_columns, ["id"]);
        assert_eq!(resolved.data_columns, &["name", "owner_id"]);
        assert_eq!(eq_column(&resolved.filter), ("owner_id", "alice"));
    }

    #[tokio::test]
    async fn parameterized_slice_uses_validated_params() {
        let registry = SliceRegistry::new()
            .define::<Animal, ChannelParams, _, _>("channel", |_ctx, p| async move {
                Ok(Filter::eq("channel_id", p.channel_id))
            });

        let resolved = registry
            .resolve(
                "channel",
                &principal("bob"),
                json!({ "channel_id": "c-7" }),
                reader(),
            )
            .await
            .expect("channel resolves");
        assert_eq!(eq_column(&resolved.filter), ("channel_id", "c-7"));
    }

    #[tokio::test]
    async fn unknown_slice_is_rejected() {
        let registry = SliceRegistry::new();
        let err = registry
            .resolve("ghost", &principal("alice"), Value::Null, reader())
            .await
            .expect_err("unknown slice");
        assert!(matches!(err, SliceError::Unknown));
    }

    #[tokio::test]
    async fn malformed_params_are_rejected() {
        let registry = SliceRegistry::new()
            .define::<Animal, ChannelParams, _, _>("channel", |_ctx, p| async move {
                Ok(Filter::eq("channel_id", p.channel_id))
            });
        let err = registry
            .resolve("channel", &principal("bob"), json!({}), reader())
            .await
            .expect_err("malformed params");
        assert!(matches!(err, SliceError::Params(_)));
    }

    #[tokio::test]
    async fn unauthorized_subscription_is_forbidden() {
        let registry = SliceRegistry::new().define::<Animal, NoParams, _, _>(
            "admins-only",
            |ctx, _params| async move {
                ctx.require(ctx.has_scope("admin"))?;
                Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
            },
        );
        let err = registry
            .resolve("admins-only", &principal("alice"), Value::Null, reader())
            .await
            .expect_err("not an admin");
        assert!(matches!(err, SliceError::Forbidden));
    }

    struct AlwaysInvalid;
    impl<'de> Deserialize<'de> for AlwaysInvalid {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            serde::de::IgnoredAny::deserialize(d)?;
            Ok(AlwaysInvalid)
        }
    }
    impl Validate for AlwaysInvalid {
        fn validate(&self) -> ValidationResult<()> {
            Err(ValidationErrors::new())
        }
    }

    #[tokio::test]
    async fn invalid_params_fail_validation() {
        let registry = SliceRegistry::new()
            .define::<Animal, AlwaysInvalid, _, _>("guarded", |_ctx, _p| async move {
                Ok(Filter::eq("owner_id", "x"))
            });
        let err = registry
            .resolve("guarded", &principal("alice"), Value::Null, reader())
            .await
            .expect_err("validation fails");
        assert!(matches!(err, SliceError::Invalid(_)));
    }

    #[test]
    fn capture_tables_describe_each_owned_table_once() {
        let registry = SliceRegistry::new()
            .define::<Animal, NoParams, _, _>("mine", |ctx, _p| async move {
                Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
            })
            .define::<Animal, ChannelParams, _, _>("by-channel", |_ctx, p| async move {
                Ok(Filter::eq("channel_id", p.channel_id))
            })
            .define::<Membership, NoParams, _, _>("memberships", |ctx, _p| async move {
                Ok(Filter::eq("user_id", ctx.user_id().as_str()))
            });

        let mut captures = registry.capture_tables();
        captures.sort_by(|a, b| a.table().cmp(b.table()));

        assert_eq!(captures.len(), 2, "{captures:?}");
        let animals = &captures[0];
        assert_eq!(animals.table(), "animals");
        assert_eq!(animals.key_columns(), ["id".to_string()]);
        assert_eq!(animals.owner_column(), "owner_id");
        assert_eq!(animals.data_columns(), &["name", "owner_id"]);
        assert_eq!(captures[1].table(), "memberships");
        assert_eq!(captures[1].owner_column(), "user_id");
    }

    #[test]
    fn capture_tables_skip_a_model_without_an_owner_column() {
        let registry = SliceRegistry::new()
            .define::<Announcement, NoParams, _, _>("all-announcements", |_ctx, _p| async move {
                Ok(Filter::eq("id", "any"))
            });
        assert!(registry.capture_tables().is_empty());
    }

    #[tokio::test]
    async fn relational_slice_reads_a_second_model_to_scope() {
        let registry = SliceRegistry::new().define::<Animal, NoParams, _, _>(
            "in-my-channels",
            |ctx, _params| async move {
                let channels = ctx
                    .read_column::<Membership>(
                        "channel_id",
                        &[Filter::eq("user_id", ctx.user_id().as_str())],
                    )
                    .await?;
                Ok(Filter::in_list("channel_id", channels))
            },
        );

        let resolved = registry
            .resolve(
                "in-my-channels",
                &principal("alice"),
                Value::Null,
                membership_reader(&["c-1", "c-2"]),
            )
            .await
            .expect("relational slice resolves");

        match &resolved.filter.condition {
            FilterCondition::In { column, values } => {
                assert_eq!(column, "channel_id");
                let texts: Vec<&str> = values
                    .iter()
                    .map(|value| match value {
                        FilterValue::String(s) => s.as_str(),
                        other => panic!("expected a string value, got {other:?}"),
                    })
                    .collect();
                assert_eq!(texts, vec!["c-1", "c-2"]);
            }
            other => panic!("expected an In condition, got {other:?}"),
        }
    }
}
