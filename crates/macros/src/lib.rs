//! Derive and attribute macros for the [Fundamental](https://docs.rs/fundamental) framework.
//!
//! Depend on the umbrella `fundamental` crate, which re-exports them, rather than on this crate.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

mod attr;
mod derives;
mod lints_macro;
mod migrations_macro;
mod slugs_macro;

use proc_macro::TokenStream;

/// An owner's migrations embedded from its crate: every `migrations/sql/*.up.sql` in name
/// order, the models whose tables they make, and the committed `migrations/schema.json`.
///
/// ```ignore
/// pub static LEASE_MIGRATIONS: EmbeddedMigrations =
///     fundamental_macros::migrations!(owner = "fundamental_lease", models = [LeaseRow, LeaseWorkRow]);
/// ```
///
/// # Attributes
/// - `retired = ["…"]`: tables its files made that no model holds any more; the next
///   generated file drops them.
/// - `drops_by_hand = ["…"]`: hand-written files whose drops no model declares.
/// - `needs_createrole = ["…"]`: files that make a role without looking it up in `pg_roles`.
///
/// A drop or rename passes the migration guard only when a model declares it
/// (`#[model(retired(columns = […]))]`, `#[model(was = "…")]`) or its table is in `retired`.
/// The files are held to the lint levels of the project's `fun.toml` (`[migrations.lints]`).
#[proc_macro]
pub fn migrations(input: TokenStream) -> TokenStream {
    migrations_macro::migrations(input)
}

/// The project's migration lint levels, compiled in from the nearest `fun.toml` at or above
/// the invoking crate, as a `Result<Lints, LintsError>`; the defaults when there is none.
///
/// Compiled in because a migrator runs where its source is not.
///
/// ```ignore
/// guard = MigrationGuard::new(1_000, lints!()?, &[]),
/// ```
#[proc_macro]
pub fn lints(input: TokenStream) -> TokenStream {
    lints_macro::lints(input)
}

/// Declare a page's path segment in each language it is addressed in, as a `pub const
/// PAGE_SLUGS`.
///
/// Each spelling is the word in its own script; the address percent-encodes it. A slug that
/// cannot be read from an address bar is a compile error. A `retired` spelling keeps
/// resolving and is never emitted.
///
/// # Example
/// ```ignore
/// // pages/why.rs  ->  /why, /nl/waarom, /sv/varfor, /ja/%E3%81%AA%E3%81%9C
/// fundamental::slugs! {
///     nl = "waarom",
///     sv = "varfor",
///     ja = "なぜ",
///     retired nl = "waarom-acme",
/// }
///
/// pub fn page() -> impl IntoView { view! { <h1>"Why"</h1> } }
/// ```
#[proc_macro]
pub fn slugs(input: TokenStream) -> TokenStream {
    slugs_macro::slugs(input)
}

/// Derive `fundamental_core::error::Error` for an error enum: a code, message, HTTP status
/// and retryability per variant.
///
/// # Attributes
///
/// - `#[error(code = "...", message = "...", status = N)]` - Error metadata
/// - `#[error(..., retryable)]` - Mark error as retryable
/// - `#[error(..., field = "email")]` - The input field this variant refuses, answered by
///   `field_errors()`
/// - `#[source]` - Mark the source error field for error chaining
/// - `#[from]` - Generate `From<T>` impl for automatic conversion (enables `?` operator)
/// - `#[overrides]` - A field whose value refines the variant's metadata: any Fundamental
///   `Error`, any `ErrorOverride`, or an `Option` of these. The first `Some` wins, in field
///   order.
/// - `#[error_into(Target)]` on the enum - `impl From<Self> for Target` through `AnyError`,
///   losslessly; `Target` needs an `AnyError` catch-all variant.
///
/// A variant without `#[error(...)]` whose sole field is an `AnyError` is the catch-all, with
/// `#[from]` and `#[overrides]` implied; one per enum. Generic bounds are inferred from use;
/// lifetime parameters are refused.
///
/// # Example
///
/// ```ignore
/// use fundamental::Error;
///
/// #[derive(Debug, Error)]
/// pub enum UserError {
///     #[error(code = "USER_001", message = "User not found", status = 404)]
///     NotFound { id: String },
///
///     #[error(code = "USER_002", message = "Email already in use", status = 409)]
///     EmailExists,
///
///     #[error(code = "VALIDATION", message = "Validation failed", status = 400)]
///     Validation(#[from] #[source] ValidationErrors),
/// }
/// ```
///
/// Two variants with the same code do not build:
///
/// ```compile_fail
/// use fundamental::Error;
///
/// #[derive(Debug, Error)]
/// pub enum TwiceError {
///     #[error(code = "TWICE_001", message = "Denied", status = 403)]
///     Denied,
///     #[error(code = "TWICE_001", message = "Missing", status = 403)]
///     Missing,
/// }
/// ```
///
/// ```
/// use fundamental::Error;
///
/// #[derive(Debug, Error)]
/// pub enum OnceError {
///     #[error(code = "ONCE_001", message = "Denied", status = 403)]
///     Denied,
///     #[error(code = "ONCE_002", message = "Missing", status = 403)]
///     Missing,
/// }
/// ```
///
/// ```ignore
/// #[derive(Debug, Error)]
/// #[error_into(DatabaseError)]
/// pub enum SqlError { ... }
///
/// // in DatabaseError:
/// Other(AnyError),
/// ```
#[proc_macro_derive(Error, attributes(error, error_into, source, from, overrides))]
pub fn derive_fundamental_error(input: TokenStream) -> TokenStream {
    derives::error::derive(input)
}

/// Derive `fundamental_core::error::ErrorOverride` for an enum that refines a
/// `#[derive(Error)]` variant's metadata as its `#[overrides]` field.
///
/// A Fundamental `Error` is already an override source and must not derive this.
///
/// # Attributes
///
/// Per-variant `#[overrides(...)]`; every key is optional, and an undeclared key never
/// overrides:
///
/// - `code = "..."` - Override the machine-readable error code
/// - `message = "..."` - Override the user-facing message (static literal)
/// - `developer_message = "..."` - Override the developer message
/// - `status = N` - Override the HTTP status code
/// - `retryable` / `retryable = false` - Override the retryable flag
/// - `exit_code = N` - Override the process exit code
///
/// # Example
///
/// ```no_run
/// use fundamental::{Error, ErrorOverride};
///
/// #[derive(Debug, Clone, Copy, ErrorOverride)]
/// pub enum ConstraintKind {
///     #[overrides(status = 409)]
///     Unique,
///     ForeignKey,
///     NotNull,
///     Check,
/// }
///
/// #[derive(Debug, Error)]
/// pub enum DatabaseError {
///     #[error(code = "DB_003", message = "Query failed", status = 500)]
///     ConstraintViolation {
///         constraint: String,
///         #[overrides]
///         kind: ConstraintKind, // Unique refines the status to 409
///     },
/// }
/// ```
#[proc_macro_derive(ErrorOverride, attributes(overrides))]
pub fn derive_error_override(input: TokenStream) -> TokenStream {
    derives::error_override::derive(input)
}

/// Derive `fundamental_core::Validate` from field attributes, collecting every failure.
///
/// Derivable on a struct with named fields, or on an enum whose variants have named fields;
/// only the active variant's rules run. An error names the field as the wire does
/// (`contact_email`). A tuple variant is refused.
///
/// # Validation Attributes
///
/// - `#[validate(required)]` - Field must not be empty (for strings) or None (for Option)
/// - `#[validate(length(min = N, max = M))]` - String length bounds
/// - `#[validate(range(min = N, max = M))]` - Numeric range bounds
/// - `#[validate(email)]` - Email format validation
/// - `#[validate(url)]` - URL format validation
/// - `#[validate(uuid)]` - UUID format validation
/// - `#[validate(pattern = "regex")]` - Regex pattern validation
/// - `#[validate(alphanumeric)]` - Alphanumeric characters only
/// - `#[validate(ascii)]` - ASCII characters only
/// - `#[validate(one_of("a", "b", "c"))]` - Value must be one of the listed
/// - `#[validate(nested)]` - Validate nested types that implement Validate
/// - `#[validate(custom = "function_name")]` - Custom validation function, named by path
/// - `#[validate(skip)]` - Skip validation for this field
///
/// # Example
///
/// ```no_run
/// use fundamental::Validate;
///
/// #[derive(Validate)]
/// pub struct CreateUserInput {
///     #[validate(email)]
///     pub email: String,
///
///     #[validate(length(min = 8, max = 100))]
///     pub password: String,
///
///     #[validate(length(min = 1, max = 50))]
///     pub name: String,
/// }
/// ```
///
/// ```no_run
/// use fundamental::Validate;
///
/// #[derive(Validate)]
/// pub enum Enquiry {
///     Product {
///         #[validate(required, length(min = 2, max = 200))]
///         organisation: String,
///         #[validate(required, email)]
///         contact_email: String,
///     },
///     MarketOpening {
///         #[validate(required, email)]
///         contact_email: String,
///     },
/// }
/// ```
///
/// A `custom` name that is not a path is a compile error:
///
/// ```
/// # use fundamental::Validate;
/// # use fundamental::core::validate::ValidationErrors;
/// mod rules {
///     pub fn any(_: &str) -> Result<(), fundamental::core::validate::ValidationErrors> { Ok(()) }
/// }
/// #[derive(Validate)]
/// struct Checked {
///     #[validate(custom = "rules::any")]
///     name: String,
/// }
/// ```
///
/// ```compile_fail
/// # use fundamental::Validate;
/// mod rules {
///     pub fn any(_: &str) -> Result<(), fundamental::core::validate::ValidationErrors> { Ok(()) }
/// }
/// #[derive(Validate)]
/// struct Checked {
///     #[validate(custom = "rules::any extra")]
///     name: String,
/// }
/// ```
#[proc_macro_derive(Validate, attributes(validate))]
pub fn derive_validate(input: TokenStream) -> TokenStream {
    derives::validate::derive(input)
}

/// Derive [`Model`] for a domain entity, and its backend traits when a backend is named.
///
/// # Attributes
///
/// Struct-level `#[model(...)]`; `sql` and `cql` are mutually exclusive, `offline` combines
/// with either:
///
/// - `#[model(sql)]` / `#[model(cql)]` / `#[model(offline)]` - Backend binding
/// - `#[model(table = "name")]` - Override the table name (otherwise inferred from the struct name)
/// - `#[model(domain = "name")]` - Named connection / routing key (keyspace or schema comes from that connection)
/// - `#[model(retired(columns = […]))]` - Columns, indexes or checks the model no longer has
/// - `#[model(accept(<lint> = "why"))]` - Accept a warning migration lint on this table
/// - `#[model(colocated = false)]`, `#[model(tablets = N)]`, `#[model(written_by(role = "…"))]` -
///   Placement and writer of the table
///
/// Field-level `#[model(...)]`:
///
/// - `#[model(id)]` - Mark the primary-key field
/// - `#[model(indexed)]` - Add a secondary index on the field
/// - `#[model(partition_key)]` / `#[model(clustering_key)]` - YCQL key parts
/// - `#[model(encrypted)]` - Encrypt the field at rest (see `fundamental_encryption`)
/// - `#[model(was = "old")]` - The column's previous name
///
/// # Example
///
/// The primary-key accessor lives on the backend trait (e.g. `SqlModel::id`), so a model
/// without a backend exposes its fields directly:
///
/// ```
/// use fundamental::Model;
///
/// type UserId = u64;
///
/// #[derive(Model)]
/// pub struct User {
///     #[model(id)]
///     pub id: UserId,
///     pub email: String,
/// }
///
/// // The derive generates the base `Model` impl: the `TYPE_NAME` constant.
/// assert_eq!(User::TYPE_NAME, "User");
/// let user = User { id: 1, email: "ada@example.com".into() };
/// assert_eq!(user.id, 1); // a base model exposes its fields directly
/// ```
///
/// Backend examples are tested in [`fundamental_db`] and the per-backend crates:
///
/// ```ignore
/// #[derive(Model)]
/// #[model(sql, table = "users")]   // distributed SQL
/// pub struct User {
///     #[model(id)]
///     pub id: UserId,
///     #[model(indexed)]
///     pub email: String,
/// }
/// ```
///
/// [`Model`]: fundamental_core::Model
/// [`fundamental_db`]: ../fundamental_db/index.html
#[proc_macro_derive(Model, attributes(id, model))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    derives::model::derive(input)
}

/// Derive `fundamental_authz::schema::Namespace` for an enum of relations: a typed Zanzibar
/// object type.
///
/// The namespace is the enum's name in `snake_case` (`#[namespace("…")]` overrides it); each
/// relation is its variant in `snake_case`.
///
/// # Attributes
/// - `#[relation(direct(User, Group#member))]` — tuples stored directly, restricted to these subject shapes.
/// - `#[relation(computed_userset(rel))]` (or a bare `Rel`) — the subjects of another relation on the same object.
/// - `#[relation(tuple_to_userset(tupleset, Target::relation))]` — the cross-object hop.
/// - `union(...)`, `intersection(...)`, `exclusion(include, exclude)`.
/// - `#[relation(skip)]` — no relation, for a verb that is only a [`Scope`](macro@Scope);
///   checking it fails with `AuthzError::UndefinedRelation`.
/// - `#[locked]` — the relation is checked by `TransactionalAuthorizer::check_in_locked`;
///   it must not reach an `exclusion` (`AUTHZ_021`).
///
/// # Example
/// ```ignore
/// #[derive(Namespace)]
/// enum Document {
///     #[relation(direct(User))]                                        Owner,
///     #[relation(union(Owner, tuple_to_userset(Parent, Folder::Viewer)))] Viewer,
///     #[relation(direct(Folder))]                                      Parent,
/// }
/// ```
#[proc_macro_derive(Namespace, attributes(namespace, relation, locked))]
pub fn derive_namespace(input: TokenStream) -> TokenStream {
    derives::namespace::derive(input)
}

/// Derive `fundamental_auth::ScopeSet` for an enum of authorization scopes, each unit
/// variant named `"{prefix}:{variant}"` in `snake_case`, plus `ALL_SCOPES`.
///
/// A route requires them with [`scope(…)`](../fundamental_server/fn.scope.html) and a token
/// grants them with `fundamental_auth::scopes!`. The prefix is the enum's name in
/// `snake_case` less a trailing `Scope` or `Scopes`.
///
/// # Attributes
/// - `#[scope(prefix = "…")]` on the enum — override the prefix.
/// - `#[scope(all = [A, B])]` — a named conjunction of the listed variants.
/// - `#[scope(skip)]` — not grantable and absent from `ALL_SCOPES`; a route guarding on it
///   fails closed. Its mirror is [`#[relation(skip)]`](macro@Namespace).
///
/// # Example
/// ```ignore
/// #[derive(Scope)]
/// enum Credential {
///     Offer,                                  // "credential:offer"
///     Revoke,                                 // "credential:revoke"
///     Suspend,
///     #[scope(all = [Revoke, Suspend])]
///     Lifecycle,                              // both of the above
/// }
/// ```
#[proc_macro_derive(Scope, attributes(scope))]
pub fn derive_scope(input: TokenStream) -> TokenStream {
    derives::scope::derive(input)
}

/// Derive [`Identifier`](fundamental_core::model::Identifier) for a newtype id, with
/// `From<Inner>`, `Display` and the detected sqlx and CQL bindings.
///
/// # Attributes
/// - `#[identifier(generate = "uuid_v4" | "uuid_v7")]` — implement `GeneratedId`.
/// - `#[identifier(no_display)]` / `no_sql` / `no_cql` — opt out of an impl.
#[proc_macro_derive(Identifier, attributes(identifier))]
pub fn derive_identifier(input: TokenStream) -> TokenStream {
    derives::identifier::derive(input)
}

/// Derive every codec of a one-spelling text type through one parse function.
///
/// The type implements [`CanonicalText`](fundamental_core::model::CanonicalText). Emitted:
/// `Display`, `Serialize`, `RowBound`, `FromStr`, `Deserialize` and the detected sqlx and CQL
/// codecs, each decoder bounded by `MAX_LEN`. No `From<Inner>` is emitted.
///
/// # Attributes
/// - `#[canonical_text(parse = Self::parse_stored)]` — the function that reads it; may be
///   private.
/// - `admit` — withhold `FromStr` and `Deserialize` for an
///   [`Admit`](fundamental_core::model::Admit) type, read as `Unadmitted<T>`.
///
/// ```
/// # use fundamental::CanonicalText;
/// # use fundamental::core::model::{Admit, Unadmitted};
/// #[derive(CanonicalText)]
/// #[canonical_text(parse = Self::read)]
/// struct Word(String);
/// impl Word {
///     fn read(text: &str) -> Result<Self, &'static str> {
///         if text.bytes().all(|b| b.is_ascii_lowercase()) { Ok(Self(text.into())) } else { Err("not a word") }
///     }
/// }
/// impl fundamental::core::model::CanonicalText for Word {
///     const MAX_LEN: usize = 8;
///     fn as_canonical(&self) -> &str { &self.0 }
/// }
/// #[derive(CanonicalText)]
/// #[canonical_text(parse = Self::read, admit)]
/// struct Local(String);
/// impl Local {
///     fn read(text: &str) -> Result<Self, &'static str> { Word::read(text).map(|w| Self(w.0)) }
/// }
/// impl fundamental::core::model::CanonicalText for Local {
///     const MAX_LEN: usize = 8;
///     fn as_canonical(&self) -> &str { &self.0 }
/// }
/// impl Admit for Local {
///     type Context = str;
///     type Refusal = &'static str;
///     fn admit(text: &str, here: &str) -> Result<Self, &'static str> {
///         Self::read(text).and_then(|l| if l.0 == here { Ok(l) } else { Err("elsewhere") })
///     }
/// }
/// fn deserializable<T: fundamental::core::__serde::de::DeserializeOwned>() {}
/// assert!("core".parse::<Word>().is_ok());
/// deserializable::<Word>();
/// deserializable::<Unadmitted<Local>>();
/// ```
///
/// There is no `From<String>`:
///
/// ```compile_fail
/// # use fundamental::CanonicalText;
/// # use fundamental::core::model::{Admit, Unadmitted};
/// #[derive(CanonicalText)]
/// #[canonical_text(parse = Self::read)]
/// struct Word(String);
/// impl Word {
///     fn read(text: &str) -> Result<Self, &'static str> {
///         if text.bytes().all(|b| b.is_ascii_lowercase()) { Ok(Self(text.into())) } else { Err("not a word") }
///     }
/// }
/// impl fundamental::core::model::CanonicalText for Word {
///     const MAX_LEN: usize = 8;
///     fn as_canonical(&self) -> &str { &self.0 }
/// }
/// #[derive(CanonicalText)]
/// #[canonical_text(parse = Self::read, admit)]
/// struct Local(String);
/// impl Local {
///     fn read(text: &str) -> Result<Self, &'static str> { Word::read(text).map(|w| Self(w.0)) }
/// }
/// impl fundamental::core::model::CanonicalText for Local {
///     const MAX_LEN: usize = 8;
///     fn as_canonical(&self) -> &str { &self.0 }
/// }
/// impl Admit for Local {
///     type Context = str;
///     type Refusal = &'static str;
///     fn admit(text: &str, here: &str) -> Result<Self, &'static str> {
///         Self::read(text).and_then(|l| if l.0 == here { Ok(l) } else { Err("elsewhere") })
///     }
/// }
/// fn deserializable<T: fundamental::core::__serde::de::DeserializeOwned>() {}
/// let _ = Word::from(String::from("core"));
/// ```
///
/// And an admitted type has no `Deserialize` of its own:
///
/// ```compile_fail
/// # use fundamental::CanonicalText;
/// # use fundamental::core::model::{Admit, Unadmitted};
/// #[derive(CanonicalText)]
/// #[canonical_text(parse = Self::read)]
/// struct Word(String);
/// impl Word {
///     fn read(text: &str) -> Result<Self, &'static str> {
///         if text.bytes().all(|b| b.is_ascii_lowercase()) { Ok(Self(text.into())) } else { Err("not a word") }
///     }
/// }
/// impl fundamental::core::model::CanonicalText for Word {
///     const MAX_LEN: usize = 8;
///     fn as_canonical(&self) -> &str { &self.0 }
/// }
/// #[derive(CanonicalText)]
/// #[canonical_text(parse = Self::read, admit)]
/// struct Local(String);
/// impl Local {
///     fn read(text: &str) -> Result<Self, &'static str> { Word::read(text).map(|w| Self(w.0)) }
/// }
/// impl fundamental::core::model::CanonicalText for Local {
///     const MAX_LEN: usize = 8;
///     fn as_canonical(&self) -> &str { &self.0 }
/// }
/// impl Admit for Local {
///     type Context = str;
///     type Refusal = &'static str;
///     fn admit(text: &str, here: &str) -> Result<Self, &'static str> {
///         Self::read(text).and_then(|l| if l.0 == here { Ok(l) } else { Err("elsewhere") })
///     }
/// }
/// fn deserializable<T: fundamental::core::__serde::de::DeserializeOwned>() {}
/// deserializable::<Local>();
/// ```
#[proc_macro_derive(CanonicalText, attributes(canonical_text))]
pub fn derive_canonical_text(input: TokenStream) -> TokenStream {
    derives::canonical_text::derive(input)
}

/// Derive `message(&self)` and `Display` for a fieldless enum, each variant said by its crate's
/// message of the same name.
///
/// `#[messages(path::to::module)]` names the generated module with one function per variant,
/// in `snake_case`; a missing one is a compile error.
///
/// # Example
/// ```ignore
/// #[derive(Variant, Messages)]
/// #[messages(crate::m::article::kind)]
/// enum ArticleKind { Milestone, ImportantChange }
/// assert_eq!(ArticleKind::Milestone.to_string(), "Milestone");
/// ```
#[proc_macro_derive(Messages, attributes(messages))]
pub fn derive_messages(input: TokenStream) -> TokenStream {
    derives::messages::derive(input)
}

/// Derive [`Variant`](fundamental_core::Variant) for a fieldless enum: a stable `KEY`, its
/// `OPTIONS`, `value(&self)` and `from_value`, all in `snake_case`.
///
/// # Attributes
/// - `#[variant(key = "…")]` on the enum — override the axis key.
/// - `#[variant(value = "…")]` on a variant — override that option's string value.
///
/// # Example
/// ```
/// use fundamental::Variant;
///
/// #[derive(Clone, Copy, PartialEq, Eq, Debug, Variant)]
/// #[variant(key = "loc")]
/// enum Locale {
///     EnUs,
///     #[variant(value = "nl")]
///     NlNl,
/// }
///
/// assert_eq!(Locale::KEY, "loc");
/// assert_eq!(Locale::OPTIONS.len(), 2);
/// assert_eq!(Locale::EnUs.value(), "en_us");
/// assert_eq!(Locale::from_value("nl"), Some(Locale::NlNl));
/// assert_eq!(Locale::from_value("xx"), None);
/// ```
#[proc_macro_derive(Variant, attributes(variant))]
pub fn derive_variant(input: TokenStream) -> TokenStream {
    derives::variant::derive(input)
}

/// Derive [`Connection`](fundamental_core::model::Connection) for a unit struct: the logical
/// name of the database a model routes to with `#[model(connection = …)]`.
///
/// # Attributes
/// - `#[connection(name = "…")]` — override the routing name (defaults to the
///   type's name in `snake_case`).
///
/// # Example
/// ```
/// use fundamental::Connection;
///
/// #[derive(Connection)]
/// struct Reports;
///
/// #[derive(Connection)]
/// #[connection(name = "analytics_v2")]
/// struct Analytics;
///
/// assert_eq!(Reports::NAME, "reports");
/// assert_eq!(Analytics::NAME, "analytics_v2");
/// ```
#[proc_macro_derive(Connection, attributes(connection))]
pub fn derive_connection(input: TokenStream) -> TokenStream {
    derives::connection::derive(input)
}

/// Derive `PublicSegment` for a fieldless enum: a path segment from a fixed set of words,
/// which a page policy may declare public.
///
/// Each word is the variant's name in kebab-case, or `#[segment("…")]`. A shape that could
/// carry a value is a compile error, so a public segment never carries a record's id.
///
/// # Example
/// ```ignore
/// #[derive(PublicSegment)]
/// enum Step {
///     Company,
///     #[segment("betalen")]
///     Payment,
/// }
/// ```
#[proc_macro_derive(PublicSegment, attributes(segment))]
pub fn derive_public_segment(input: TokenStream) -> TokenStream {
    derives::public_segment::derive(input)
}

/// Derive `fundamental_nats::Message`, binding a type to a NATS subject.
///
/// # Attributes
/// Exactly one of:
/// - `#[subject = "events.user.created"]` — a static subject, validated at compile time.
/// - `#[subject_fn = "method_name"]` — a `fn(&self) -> impl Into<String>` computing the
///   subject per value; `SUBJECT` is then `"__dynamic__"`.
///
/// # Example
/// ```ignore
/// #[derive(Serialize, Deserialize, Message)]
/// #[subject = "events.user.created"]
/// struct UserCreated { user_id: String }
/// ```
#[proc_macro_derive(Message, attributes(subject, subject_fn, event_id))]
pub fn derive_message(input: TokenStream) -> TokenStream {
    derives::message::derive(input)
}

/// Derive `fundamental_nats::MessageSet` and `Publish` for an enum of an app's messages,
/// one `Variant(Payload)` per message.
///
/// A variant's subject is `#[subject = "…"]` or the enum and variant names dotted together;
/// the payload alone goes on the wire. Duplicate subjects are refused.
///
/// # Example
/// ```ignore
/// #[derive(MessageSet)]
/// enum Events {
///     UserCreated(UserCreated),          // subject "events.user.created"
///     #[subject = "shop.orders.placed"]  // explicit override replaces the default
///     OrderPlaced(OrderPlaced),
/// }
/// ```
#[proc_macro_derive(MessageSet, attributes(subject))]
pub fn derive_message_set(input: TokenStream) -> TokenStream {
    derives::message_set::derive(input)
}

/// Derive `fundamental_nats::RpcSet` for an enum of an app's request/reply RPCs, one
/// `Variant(Request, Reply)` per RPC.
///
/// The request subject is `#[subject = "…"]` or the enum and variant names dotted together;
/// the reply uses it with a `.reply` suffix. Every payload gets its `Message` impl.
/// Duplicate request subjects are refused.
///
/// # Example
/// ```ignore
/// #[derive(RpcSet)]
/// enum RpcLayer1 {
///     Erase(EraseRpcRequest, EraseRpcReply),  // request "rpc.layer1.erase", reply ".reply"
/// }
/// ```
#[proc_macro_derive(RpcSet, attributes(subject))]
pub fn derive_rpc_set(input: TokenStream) -> TokenStream {
    derives::rpc_set::derive(input)
}

/// Derive `fundamental_schema::Schema`: a type's shape as JSON Schema 2020-12.
///
/// A struct with named fields, a newtype or a fieldless enum; any other shape is a compile
/// error. Serde's `rename`, `rename_all`, `skip` and `skip_serializing` are honoured.
///
/// # Example
/// ```
/// use fundamental::Schema;
///
/// #[derive(Schema)]
/// struct CreateUser {
///     name: String,
///     nickname: Option<String>,
/// }
/// ```
#[proc_macro_derive(Schema, attributes(schema, serde))]
pub fn derive_schema(input: TokenStream) -> TokenStream {
    derives::schema::derive(input)
}

/// Derive `fundamental_nats::Queue` for a unit struct: a typed NATS queue group whose `NAME`
/// is its name in `snake_case`, or `#[queue(name = "…")]`.
///
/// # Example
/// ```ignore
/// #[derive(Queue)]
/// struct LogWriters;               // NAME = "log_writers"
///
/// #[respond(queue = LogWriters)]
/// async fn audit_rpc(log: &Log, req: Req) -> Reply { /* … */ }
/// ```
#[proc_macro_derive(Queue, attributes(queue))]
pub fn derive_queue(input: TokenStream) -> TokenStream {
    derives::queue::derive(input)
}

/// Derive `fundamental_nats::Stream` for a unit struct: a typed JetStream stream whose `NAME`
/// is its name in `snake_case`, or `#[stream(name = "…")]`.
#[proc_macro_derive(Stream, attributes(stream))]
pub fn derive_stream(input: TokenStream) -> TokenStream {
    derives::stream::derive(input)
}

/// Derive `fundamental_nats::Consumer` for a unit struct: a typed JetStream consumer whose
/// `NAME` is its name in `snake_case`, or `#[consumer(name = "…")]`.
#[proc_macro_derive(Consumer, attributes(consumer))]
pub fn derive_consumer(input: TokenStream) -> TokenStream {
    derives::consumer::derive(input)
}

/// Attach an `async` handler to a transport, emitting a `{fn}_listener` that runs it and
/// returns a `fundamental_nats::Listener`.
///
/// # Attributes
/// One transport, by the keys present:
/// - core NATS: `#[subscribe]` (subject from `<T as Message>::SUBJECT`) or
///   `#[subscribe(subject = "events.user.>")]`, with an optional `queue = "…"`;
/// - JetStream: `#[subscribe(stream = "ORDERS", consumer = "proc", ..)]`, with one of:
///   - `inbox = "billing.book-deposit", isolation = Serializable` — the handler takes
///     `(tx: &mut EffectTransaction<'_>, event: &Inbound<M>)` and its effect commits once
///     per event (`fundamental_inbox`);
///   - `without_inbox = "<why a redelivery is harmless>"` — takes `ReceivedMessage<T>`;
///     `auto_ack` acks on `Ok` and nacks on `Err`;
/// - WebSocket messages: `#[subscribe(socket)]` — takes `(WsContext, P)` for a mounted
///   `Message` (the `ws` feature);
/// - WebSocket raw: `#[subscribe(socket = "chat")]` — any `Deserialize` payload, ungated.
///
/// Except the raw socket, the message must be listed in `mount_messages!`.
///
/// # Example
/// ```ignore
/// #[subscribe(stream = Deposits, consumer = Booker, inbox = "billing.book-deposit", isolation = Serializable)]
/// async fn book(tx: &mut EffectTransaction<'_>, event: &Inbound<DepositPaid>) -> Result<(), BillingError> {
///     tx.insert(&Booking::for_deposit(event.event_id(), event.payload())).await?;
///     Ok(())
/// }
/// // generated: book_listener(&jetstream, &inbox).await? -> Listener
///
/// #[subscribe(stream = "ORDERS", consumer = "order-processor", auto_ack, without_inbox = "keyed by order id")]
/// async fn on_order(msg: ReceivedMessage<Order>) -> NatsResult<()> {
///     process(msg.payload).await
/// }
/// // generated: on_order_listener(&jetstream).await? -> Listener
/// ```
#[proc_macro_attribute]
pub fn subscribe(attr: TokenStream, item: TokenStream) -> TokenStream {
    attr::expand_subscribe(attr, item)
}

/// Attach an `async` request-reply handler to a NATS subject; its return value is published
/// to the caller's reply inbox.
///
/// The subject defaults to the request type's `#[subject]`; `queue = "…"` shares the RPC
/// across a queue group.
///
/// ```ignore
/// #[respond(subject = "audit.rpc", queue = "log-writers")]
/// async fn audit_rpc(log: &WitnessedLog, req: AuditRpcRequest) -> AuditRpcReply {
///     match log.apply(req).await {
///         Ok(reply) => reply,
///         Err(e) => AuditRpcReply::Failure { code: e.error_code().into(), message: e.to_string() },
///     }
/// }
/// // generated: audit_rpc_responder(Arc<NatsClient>, Arc<WitnessedLog>).await? -> Listener
/// ```
///
/// An optional third parameter, `meta: &RespondMeta`, gives the request's headers and
/// subject; a bearer token travels there, never in the payload.
///
/// ```ignore
/// #[respond(queue = Erasers)]
/// async fn erase_rpc(state: &State, req: EraseRequest, meta: &RespondMeta) -> EraseReply {
///     let Some(token) = meta.bearer_token() else {
///         return EraseReply::Failure { /* unauthenticated */ };
///     };
///     // …
/// }
/// ```
#[proc_macro_attribute]
pub fn respond(attr: TokenStream, item: TokenStream) -> TokenStream {
    attr::expand_respond(attr, item)
}

/// Bind an `async fn (ctx: HttpContext<S>, input: I) -> Result<O, AnyError>` to a method and
/// path, emitting a `{fn}_route` that returns its `fundamental_server::Route`.
///
/// In a web crate (one depending on `leptos`) it also emits a same-named browser stub,
/// `async fn {fn}(input: I) -> Result<O, AnyError>`, that calls the route.
///
/// # Attributes
/// - `METHOD` — `GET`, `POST`, `PUT`, `PATCH` or `DELETE` (any case).
/// - `content_type = "…"` — return the `String` output verbatim under that media type.
/// - `bytes` — with `content_type`, return a `Vec<u8>` body raw.
/// - `reactive`, `reactive(query)`, `reactive(mutation)` — the role of its `use_*` handle;
///   `GET` defaults to query, other methods to mutation.
/// - any other name after the path — a [`middleware`](macro@middleware), run in order.
///
/// A handler may be generic over its state, bounded by what it reads; the state is then its
/// only parameter, and its input and output may not name it.
///
/// ```ignore
/// pub trait NewsroomReader: Clone + Send + Sync + 'static {
///     fn reader(&self) -> &SqlPool;
/// }
///
/// #[action(GET, "/newsroom/articles")]
/// pub async fn browse<S: NewsroomReader>(
///     ctx: HttpContext<S>,
///     query: ArticleQuery,
/// ) -> Result<ArticlePage, AnyError> {
///     ArticlePage::read(ctx.state().reader(), query).await
/// }
/// // generated: impl<S: NewsroomReader> ServerAction<S> for Browse,
/// //   and browse_route::<S>() -> Route<S>.
///
/// // in the application:
/// impl NewsroomReader for WwwState {
///     fn reader(&self) -> &SqlPool { &self.reader }
/// }
/// mount_actions! { newsroom::Browse }
/// ```
///
/// # Example
/// ```ignore
/// #[action(POST, "/users", auth, rate_limit, audit_log)]
/// async fn create_user(ctx: HttpContext<AppState>, input: NewUser) -> Result<User, AnyError> {
///     ctx.state().db.insert(input).await
/// }
/// // generated: create_user_route() -> Route<AppState>
/// //   runs auth, then rate_limit, then audit_log, then the handler.
/// // in a web crate, also: async fn create_user(NewUser) -> Result<User, AnyError>
/// ```
#[proc_macro_attribute]
pub fn action(attr: TokenStream, item: TokenStream) -> TokenStream {
    attr::expand_action(attr, item)
}

/// Bind a server resolver to a syncable model and a slice name, emitting a marker type that
/// implements `fundamental::sync::Slice` and, off wasm, `ServerSlice`.
///
/// Without a params argument the slice's `Params` is `fundamental::sync::NoParams`. List the
/// marker in `mount_slices!` to serve it.
///
/// # Example
/// ```ignore
/// #[slice(Animal, "mine")]
/// async fn mine(ctx: SliceContext, _params: NoParams) -> Result<Filter, SliceError> {
///     Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
/// }
/// // generated: struct Mine; impl Slice for Mine; impl ServerSlice for Mine (off wasm).
/// ```
#[proc_macro_attribute]
pub fn slice(attr: TokenStream, item: TokenStream) -> TokenStream {
    attr::expand_slice(attr, item)
}

/// Mount a surface's actions: `register_actions(router)` and `describe_actions()` off wasm,
/// and in a web crate a `pub mod actions` with a stub per action.
///
/// An action not listed has no stub. Every listed action shares the router's state type.
///
/// # Example
/// ```
/// use fundamental::{action, mount_actions, AnyError};
/// use fundamental::server::HttpContext;
///
/// #[action(POST, "/double")]
/// async fn double(ctx: HttpContext<()>, input: u64) -> Result<u64, AnyError> {
///     Ok(input * 2)
/// }
///
/// mount_actions! { Double }
///
/// let actions = describe_actions();
/// assert_eq!(actions[0].method, "POST");
/// assert_eq!(actions[0].path, "/double");
/// ```
#[proc_macro]
pub fn mount_actions(input: TokenStream) -> TokenStream {
    attr::expand_mount_actions(input)
}

/// Declare the messages an app handles: a crate-local `MountedMessage` trait that
/// [`#[subscribe]`](macro@subscribe) requires, and `describe_messages()` off wasm.
///
/// Invoke it once at the crate root.
///
/// # Example
/// ```ignore
/// mount_messages! { UserCreated, OrderPlaced }
/// // generated: trait MountedMessage + impls; fn describe_messages() -> Vec<MessageDescriptor>
/// ```
#[proc_macro]
pub fn mount_messages(input: TokenStream) -> TokenStream {
    attr::expand_mount_messages(input)
}

/// Declare the [`#[respond]`](macro@respond) handlers an app serves, by path, emitting
/// `describe_responders()` off wasm.
///
/// # Example
/// ```ignore
/// mount_responders! { audit_rpc }
/// // generated: fn describe_responders() -> Vec<MessageDescriptor>
/// ```
#[proc_macro]
pub fn mount_responders(input: TokenStream) -> TokenStream {
    attr::expand_mount_responders(input)
}

/// Declare the [`#[subscribe]`](macro@subscribe) handlers an app's worker runs, by path,
/// emitting `register_workers(&WorkerContext)` and `describe_workers()` off wasm.
///
/// A path that names no `#[subscribe]` handler does not compile. See also
/// [`mount_responders!`] and [`mount_messages!`].
///
/// # Example
/// ```ignore
/// mount_workers! { on_hello, billing::book }
/// // generated: fn register_workers(&WorkerContext) -> WorkersFuture; fn describe_workers()
/// ```
#[proc_macro]
pub fn mount_workers(input: TokenStream) -> TokenStream {
    attr::expand_mount_workers(input)
}

/// Mount a surface's slices: `register_slices(registry)` off wasm, and in a web crate a
/// `pub mod slices` with a binder per slice.
///
/// # Example
/// ```ignore
/// mount_slices! { Mine, Channel }
/// // generated (server): fn register_slices(SliceRegistry) -> SliceRegistry
/// // generated (web):    mod slices { fn mine(..); fn channel(..); }
/// ```
#[proc_macro]
pub fn mount_slices(input: TokenStream) -> TokenStream {
    attr::expand_mount_slices(input)
}

/// Mount a native host's device slices, emitting `device_slices()` off wasm; a slice not
/// listed answers `NATIVE_016`.
///
/// # Example
/// ```ignore
/// mount_device_slices! { Mine, Channel }
/// // generated: fn device_slices() -> fundamental::native::DeviceSliceRegistry
/// ```
#[proc_macro]
pub fn mount_device_slices(input: TokenStream) -> TokenStream {
    attr::expand_mount_device_slices(input)
}

/// Bind a recompute handler to a projection model, its source tables and a name, emitting a
/// marker type (the handler's name in PascalCase) that implements `ServerProjection` off wasm.
///
/// List the marker in `mount_projections!`.
///
/// # Example
/// ```ignore
/// #[projection(ChannelSummary, sources(Message), "channel_summary")]
/// async fn summarize_channel(ctx: ProjectionContext, change: SyncChange)
///     -> Result<Vec<ProjectionUpdate>, ProjectionError> { Ok(Vec::new()) }
/// // generated: struct SummarizeChannel; impl ServerProjection for SummarizeChannel (off wasm).
/// ```
#[proc_macro_attribute]
pub fn projection(attr: TokenStream, item: TokenStream) -> TokenStream {
    attr::expand_projection(attr, item)
}

/// Mount a surface's projections, emitting `register_projections(registry)` off wasm.
///
/// # Example
/// ```ignore
/// mount_projections! { SummarizeChannel }
/// // generated (server): fn register_projections(ProjectionRegistry) -> ProjectionRegistry
/// ```
#[proc_macro]
pub fn mount_projections(input: TokenStream) -> TokenStream {
    attr::expand_mount_projections(input)
}

/// Turn `async fn name(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult`
/// into a unit `struct name` implementing `fundamental_server::Middleware`.
///
/// Call `next.run(ctx, body)` to continue the chain, or return early.
///
/// # Example
/// ```ignore
/// #[middleware]
/// async fn auth<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
///     if ctx.principal_opt().is_none() {
///         return Err(AnyError::new(ServerError::unauthenticated()));
///     }
///     next.run(ctx, body).await
/// }
/// // generated: struct auth; impl Middleware<S> for auth { … }
/// ```
///
/// # Attributes
/// - `on = Type` — implement `Middleware` for the caller's `Type`, taking `&self`; `&mut self`
///   is refused.
///
/// ```ignore
/// pub struct HttpMetrics { requests: IntCounterVec }
///
/// #[middleware(on = HttpMetrics)]
/// async fn record<S>(&self, ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
///     let route = ctx.route_template().unwrap_or("unmatched").to_owned();
///     let result = next.run(ctx, body).await;
///     self.requests.with_label_values(&[&route]).inc();
///     result
/// }
///
/// // registered once, as a value: `router.middleware(metrics.clone())`
/// ```
#[proc_macro_attribute]
pub fn middleware(attr: TokenStream, item: TokenStream) -> TokenStream {
    attr::expand_middleware(attr, item)
}

/// Mark `async fn name(ctx: &Context) -> Result<(), SeedError>` as a database seed, emitting
/// a `name_seed()` accessor for `seed_main!`.
///
/// # Attributes
/// - `name = "…"` — the seed's name (defaults to the function name); also the
///   target another seed references with `after`.
/// - `order = N` — run order among independent seeds, lower first (default `100`).
/// - `after = "…"` — names a seed that must run before this one.
///
/// # Example
/// ```ignore
/// #[seed(order = 10)]
/// async fn bootstrap(ctx: &AppContext) -> Result<(), SeedError> {
///     ctx.db.repository::<User>().upsert(&admin).await?;
///     Ok(())
/// }
/// // generated: bootstrap_seed() -> Seed<AppContext>
/// ```
#[proc_macro_attribute]
pub fn seed(attr: TokenStream, item: TokenStream) -> TokenStream {
    attr::expand_seed(attr, item)
}

/// Generate a seeder `fn main` for `src/bin/seed.rs` that loads `app`, applies the
/// environment gate, builds `context` and runs the listed `#[seed]` functions.
///
/// `context` runs in an `async` block and may borrow `app`. Requires a multi-thread `tokio`.
///
/// # Example
/// ```ignore
/// // src/bin/seed.rs
/// fundamental::seed_main! {
///     app = fundamental::config::AppSettings::load("main")?,
///     context = AppContext::connect(app).await?,
///     seeds = [bootstrap, demo],
/// }
/// ```
#[proc_macro]
pub fn seed_main(input: TokenStream) -> TokenStream {
    attr::expand_seed_main(input)
}

/// Mark `async fn name(ctx: &Context[, args: CommandArgs]) -> Result<(), CommandError>` as a
/// developer command, emitting a `name_command()` accessor for `commands_main!`.
///
/// # Attributes
/// - `name = "…"` — the command's name for `fun run <name>` (defaults to the function name).
/// - `description = "…"` — the one-line help shown by `fun run --list`.
///
/// # Example
/// ```ignore
/// #[command(description = "Recompute the search index")]
/// async fn reindex(ctx: &AppContext, args: CommandArgs) -> Result<(), CommandError> {
///     ctx.search().rebuild(args.first()).await?;
///     Ok(())
/// }
/// // generated: reindex_command() -> Command<AppContext>
/// ```
#[proc_macro_attribute]
pub fn command(attr: TokenStream, item: TokenStream) -> TokenStream {
    attr::expand_command(attr, item)
}

/// Generate a commands runner `fn main` for `src/bin/commands.rs` that lists the `#[command]`
/// functions or runs the named one.
///
/// Listing returns before `context` is built, so it needs no database. `context` runs in an
/// `async` block. Requires a multi-thread `tokio`.
///
/// # Example
/// ```ignore
/// // src/bin/commands.rs
/// fundamental::commands_main! {
///     context = AppContext::connect().await?,
///     commands = [reindex, backfill],
/// }
/// ```
#[proc_macro]
pub fn commands_main(input: TokenStream) -> TokenStream {
    attr::expand_command_main(input)
}

/// Generate a migration runner `fn main` for `src/bin/migrate.rs` with the `apply`, `status`,
/// `check` and `verify <base>` subcommands.
///
/// Each backend's connection expression runs in an `async` block, may read `app`, and is
/// evaluated only for the subcommand run. A failure exits `1` with its code; an unknown or
/// missing subcommand exits `2`. Requires `tokio` and `fundamental`'s `observability`
/// feature.
///
/// # Attributes
/// - `app = "<name>"` — required; its configuration and telemetry are loaded first.
/// - `telemetry = |builder| …` — adjust the `ObservabilityBuilder`.
/// - `migrations_dir = <expr>`.
/// - `guard = <MigrationGuard>` — required; its lint levels come from `lints!()?`.
/// - `embedded = [&A, &B]` or a `&'static [&'static EmbeddedMigrations]` — libraries'
///   migrations, applied first, in order.
/// - `sql = (<conn>, [Model, …])`, `cql = …`, `offline = …` — at least one backend;
///   `verify` needs `sql`.
///
/// # Example
/// ```ignore
/// // src/bin/migrate.rs
/// fundamental::migrator::migrate_main! {
///     app = "main",
///     migrations_dir = "apps/main/migrations",
///     guard = MigrationGuard::new(1_000, lints!()?, &[]),
///     sql = (connect_pool(app).await?, [User, Post]),
///     offline = (open_offline().await?, [CachedDoc]),
/// }
/// ```
#[proc_macro]
pub fn migrate_main(input: TokenStream) -> TokenStream {
    attr::expand_migrate_main(input)
}
