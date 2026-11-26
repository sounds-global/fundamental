//! [`AuthzError`]: the authorization subsystem's typed error.

use fundamental_core::AnyError;
use fundamental_db::DatabaseError;
use fundamental_macros::Error;

/// Errors raised by the authorization subsystem.
///
/// A permission denial is not an `AuthzError`: a check returns `allowed = false`, and the
/// guard turns that into a `403`. Infrastructure and schema failures are never a silent deny.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AuthzError {
    /// A wrapped error that does not map onto a more specific variant.
    Other(AnyError),

    /// An identifier failed the safe-identifier rule and was refused before reaching storage.
    #[error(
        code = "AUTHZ_001",
        message = "Invalid {kind} name: {value}",
        status = 400
    )]
    InvalidName {
        /// What kind of name was rejected (`namespace`, `relation`, `caveat`).
        kind: &'static str,
        /// The rejected value, for diagnostics.
        value: String,
    },

    /// The schema references a namespace that is not defined.
    #[error(
        code = "AUTHZ_002",
        message = "Schema references undefined namespace: {namespace}",
        status = 500
    )]
    UndefinedNamespace {
        /// The undefined namespace name.
        namespace: String,
    },

    /// The schema references a relation that is not defined on its namespace.
    #[error(
        code = "AUTHZ_003",
        message = "Namespace `{namespace}` references undefined relation `{relation}`",
        status = 500
    )]
    UndefinedRelation {
        /// The namespace whose rewrite referenced the missing relation.
        namespace: String,
        /// The undefined relation name.
        relation: String,
    },

    /// A relation or tuple references a caveat that is not defined.
    #[error(
        code = "AUTHZ_004",
        message = "Namespace `{namespace}` references undefined caveat `{caveat}`",
        status = 500
    )]
    UndefinedCaveat {
        /// The namespace whose relation referenced the missing caveat.
        namespace: String,
        /// The undefined caveat name.
        caveat: String,
    },

    /// A schema loaded from config text could not be parsed.
    #[error(
        code = "AUTHZ_005",
        message = "Failed to parse authorization schema: {0}",
        status = 400
    )]
    SchemaParse(String),

    /// An object or subject identifier failed the canonical-id rule.
    #[error(
        code = "AUTHZ_006",
        message = "Invalid object/subject identifier: {value}",
        status = 400
    )]
    InvalidCanonicalId {
        /// The rejected identifier, truncated for diagnostics.
        value: String,
    },

    /// A caveat context could not be encoded to JSON for storage.
    #[error(
        code = "AUTHZ_007",
        message = "Failed to encode caveat context: {0}",
        status = 500
    )]
    CaveatEncoding(String),

    /// Creating the tuple table(s) or their indexes failed, with the backend's error text.
    #[error(
        code = "AUTHZ_008",
        message = "Failed to set up authorization storage schema: {0}",
        status = 500
    )]
    SchemaSetup(String),

    /// A stored row could not be decoded into a tuple.
    #[error(
        code = "AUTHZ_009",
        message = "Failed to decode stored tuple: {0}",
        status = 500
    )]
    StorageDecode(String),

    /// A check exceeded the maximum evaluation depth.
    ///
    /// An error, not a denial, so a deep or cyclic schema is fixed rather than masked.
    #[error(
        code = "AUTHZ_010",
        message = "Authorization check exceeded the maximum depth of {depth}",
        status = 500
    )]
    DepthExceeded {
        /// The depth limit that was hit.
        depth: usize,
    },

    /// A check exceeded its read budget; an error, not a denial.
    #[error(
        code = "AUTHZ_011",
        message = "Authorization check exceeded its read budget of {budget}",
        status = 500
    )]
    BudgetExceeded {
        /// The read-budget limit that was hit.
        budget: usize,
    },

    /// A caveat could not be evaluated (a missing parameter, or incomparable values).
    #[error(
        code = "AUTHZ_012",
        message = "Caveat evaluation failed: {0}",
        status = 500
    )]
    CaveatEval(String),

    /// A consistency token (zookie) was malformed and could not be decoded.
    #[error(
        code = "AUTHZ_013",
        message = "Invalid consistency token: {value}",
        status = 400
    )]
    InvalidZookie {
        /// The rejected token, truncated for diagnostics.
        value: String,
    },

    /// An authorization decision was asked inside a caller's transaction whose isolation
    /// permits write skew.
    ///
    /// Below `SERIALIZABLE`, two transactions can each decide on a snapshot the other then
    /// invalidates, so the decision is refused.
    #[error(
        code = "AUTHZ_017",
        message = "An authorization decision inside a transaction requires SERIALIZABLE isolation",
        developer_message = "The caller's transaction was begun at {found}; begin it with IsolationLevel::Serializable.",
        status = 500
    )]
    IsolationTooWeak {
        /// The isolation the transaction was begun at, or that none was stated.
        found: String,
    },

    /// A compiled schema could not be serialised to the bytes its fingerprint is taken over.
    ///
    /// Refused rather than fingerprinted as the empty schema, which would let a zookie be trusted
    /// under another schema.
    #[error(
        code = "AUTHZ_018",
        message = "The authorization schema could not be fingerprinted",
        developer_message = "SchemaIr fingerprint serialisation failed: {detail}",
        status = 500
    )]
    SchemaNotHashable {
        /// Why serialisation failed.
        detail: String,
    },

    /// A check's request context bound a caveat parameter.
    ///
    /// A caveated edge is judged against its stored arguments and the engine's own `now`; a
    /// caller-supplied context could widen a grant, so it must be empty.
    #[error(
        code = "AUTHZ_019",
        message = "A check may not bind the caveat parameter `{parameter}`",
        developer_message = "The request context bound {count} caveat parameter(s), the first `{parameter}`. Caveats read only their stored arguments and the engine's `now`; pass an empty CaveatContext.",
        status = 400
    )]
    RequestCaveatParameter {
        /// The first parameter the request context bound, in name order.
        parameter: String,
        /// How many parameters it bound.
        count: usize,
    },

    /// A read scope reported a `now` that cannot be the real time.
    ///
    /// A time-bounded grant judged at the epoch would admit forever.
    #[error(
        code = "AUTHZ_020",
        message = "The authorization clock is not plausible",
        developer_message = "The evaluation's read scope reported now = {unix_seconds} s since the Unix epoch, before the earliest plausible instant {floor}.",
        status = 500,
        retryable
    )]
    ImplausibleClock {
        /// The instant the scope reported, in seconds since the Unix epoch.
        unix_seconds: i64,
        /// The earliest instant accepted, in seconds since the Unix epoch.
        floor: i64,
    },

    /// A relation declared `#[locked]` reaches an `exclusion` through its rewrite.
    ///
    /// A locked check locks only the tuples its allow rests on, and an exclusion grants on
    /// absence, which cannot be locked.
    #[error(
        code = "AUTHZ_021",
        message = "The authorization schema declares a locked relation that reaches an exclusion",
        developer_message = "{namespace}#{relation} is declared #[locked] but reaches the exclusion on {exclusion} through {path}. Remove #[locked], or express the rule without exclusion.",
        status = 500
    )]
    LockedRelationReachesExclusion {
        /// The namespace of the locked relation.
        namespace: String,
        /// The locked relation.
        relation: String,
        /// The relation whose rewrite holds the exclusion, as `namespace#relation`.
        exclusion: String,
        /// Every relation walked from the locked one to the exclusion, joined by ` -> `.
        path: String,
    },

    /// A locked check was asked of a relation the schema did not declare `#[locked]`.
    ///
    /// Only the declaration guarantees the schema holds no exclusion under the relation.
    #[error(
        code = "AUTHZ_022",
        message = "This relation is not declared for locked checks",
        developer_message = "{namespace}#{relation} was checked with check_in_locked; declare the relation #[locked] (RelationIr::with_lock).",
        status = 500
    )]
    RelationNotLocked {
        /// The namespace of the relation.
        namespace: String,
        /// The relation.
        relation: String,
    },

    /// A locked check was asked inside a transaction that is not `READ COMMITTED`.
    ///
    /// The locked check is proved at `READ COMMITTED`.
    #[error(
        code = "AUTHZ_023",
        message = "A locked authorization check requires READ COMMITTED isolation",
        developer_message = "The caller's transaction was begun at {found}; begin it with IsolationLevel::ReadCommitted.",
        status = 500
    )]
    LockedCheckIsolation {
        /// The isolation the transaction was begun at, or that none was stated.
        found: String,
    },

    /// A tuple a locked check's allow rested on changed between the walk and its lock.
    ///
    /// The caller's whole transaction is run again.
    #[error(
        code = "AUTHZ_024",
        message = "Authorization changed while it was being checked",
        developer_message = "{moved} of {witness} witness tuple(s) moved before their lock was granted; the transaction is re-run.",
        status = 503,
        retryable
    )]
    LockedWitnessMoved {
        /// How many witness tuples moved.
        moved: usize,
        /// How many witness tuples were locked.
        witness: usize,
    },

    /// A locked walk reached an `exclusion` the schema's compile-time closure does not.
    ///
    /// A stored tuple outside its relation's declared subject shapes led the walk there, and an
    /// exclusion would let an absence grant.
    #[error(
        code = "AUTHZ_025",
        message = "A locked authorization check reached an exclusion",
        developer_message = "The locked walk reached the exclusion on {namespace}#{relation} through a stored tuple whose shape the schema does not declare.",
        status = 500
    )]
    ExclusionInLockedWalk {
        /// The namespace whose relation holds the exclusion.
        namespace: String,
        /// The relation holding the exclusion.
        relation: String,
    },

    /// A locked check's witness held more tuples than one act may lock.
    #[error(
        code = "AUTHZ_026",
        message = "A locked authorization check would lock too many tuples",
        developer_message = "The witness holds {tuples} tuples; a locked check locks at most {limit}.",
        status = 500
    )]
    LockBudgetExceeded {
        /// How many tuples the witness held.
        tuples: usize,
        /// The most a locked check locks.
        limit: usize,
    },

    /// A locked check was asked while the caller's transaction holds a savepoint open.
    ///
    /// A rollback to it would release the locks the decision rests on.
    #[error(
        code = "AUTHZ_027",
        message = "A locked authorization check cannot run inside a savepoint",
        developer_message = "The caller's transaction holds {open} savepoint(s) open; release or roll back to them before check_in_locked.",
        status = 500
    )]
    LockedCheckInSavepoint {
        /// How many savepoints are open.
        open: usize,
    },

    /// The relation-tuple table is not on the connection's search path: its migrations are not
    /// applied.
    #[error(
        code = "AUTHZ_028",
        message = "Authorization storage is not set up",
        developer_message = "authz_relation_tuple is missing; register fundamental_authz::SQL_TUPLE_MIGRATIONS with MigrateRunner::with_embedded (or migrate_main!'s `embedded`) and run the migrator.",
        status = 500
    )]
    TupleTableMissing,

    /// The relation-tuple table differs from the one `SQL_TUPLE_MIGRATIONS` creates.
    #[error(
        code = "AUTHZ_029",
        message = "Authorization storage does not match its schema",
        developer_message = "authz_relation_tuple differs from fundamental_authz::SQL_TUPLE_MIGRATIONS: {problems}",
        status = 500
    )]
    TupleTableMismatch {
        /// Every difference found, separated by `; `.
        problems: String,
    },

    /// The connected role can change the relation-tuple table's shape, not only its rows.
    #[error(
        code = "AUTHZ_030",
        message = "Authorization storage is reachable by a role that can alter it",
        developer_message = "The runtime role holds more than SELECT, INSERT, UPDATE, DELETE on authz_relation_tuple: {excess}. Run the migrator as a separate role and grant this one only those four.",
        status = 500
    )]
    TupleRoleTooBroad {
        /// Every privilege beyond the four, separated by `, `.
        excess: String,
    },

    /// A storage-backend operation failed, forwarded with its own code, message and status.
    #[error(transparent)]
    Storage(#[from] DatabaseError),
}
