# Changelog

Every user-visible change, in the [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
format. Until 1.0 a minor version may break; a breaking change says what to do instead.

## [Unreleased]

The first public release.

### Added

- `fun bench`: the workspace's benchmarks, one layer at a time, each a bench target of the
  layer's name in any member (so an app's own benches run the same way): `fun bench` counts
  instructions (`instructions`) under callgrind and checks them against
  `benches/baseline/<arch>.json`, naming every bench beyond its threshold (`CLI_084`), and
  `fun bench --save` rewrites that file with the toolchain it was measured with (refused before
  another layer, `fun bench --save wall`, rather than ignored); `fun bench wall`
  runs criterion; `fun bench load --each 20s --concurrency 1,8,32,128`, `fun bench soak
  --duration 10m` and `fun bench fault --writes 300` build with the `bench-live` profile and
  write their reports under `target/bench`. The counts are taken in a container (docker or
  podman) whenever one is here, even beside a host valgrind, since callgrind counts libc too
  and only the baseline's libc compares; the host's valgrind is used only without an engine.
  The container is built from the project's own toolchain, valgrind and the
  `gungraun-runner` the lockfile names, with the workspace mounted at its own path and built
  into `target/bench-container`. The committed `benches/baseline/aarch64.json` is the first
  baseline; `x86_64.json` is written by the `bench` workflow's `save-baseline` run.
- CI: a `bench` job on every pull request runs `fun bench` on an arm64 runner against the
  committed `aarch64.json`, in the same pinned container a developer machine uses, so libc, valgrind and toolchain match the baseline (installing valgrind on the runner
  would count a different libc). `.github/workflows/bench.yml`, run by hand, runs `wall`,
  `load`, `soak` and `fault` with the step time and soak duration as inputs and uploads the
  reports, and with `save-baseline` counts on x86_64 and arm64 and uploads each baseline.
- `BENCHMARKS.md`: the method of every layer, the machine, the command behind each number, and
  one measured run of each: the instruction counts, criterion's means, load at 1, 8, 32 and 128
  in flight, a ten-minute soak (memory flat at 46 to 47 MiB after the first minute, no failed
  call) and the three faults (no write lost or duplicated, every job fired). Linked from the
  README and CONTRIBUTING; AGENTS.md now asks a hot-path change to run `fun bench` and to commit
  a moved baseline with its reason.
- `fundamental_test_containers::DedicatedEngineNode` and `nats::DedicatedNatsServer`: a
  YugabyteDB node and a NATS server a fault run owns (`fundamental-test-fault-engine`,
  `fundamental-test-nats-08-fault`), configured as the shared ones are, so `restart`, `stop` and
  `start_again` disturb no suite. Each start reads the ports again and waits until the engine
  answers.
- `fundamental_test_containers::TcpProxy::retarget`: every new connection goes to another
  upstream, so a client keeps one address across a container restart, which Docker publishes on
  new host ports.
- `fundamental_bench`, a dev-only crate of benchmarks and stability runs (`publish = false`):
  `Baseline` and `Measured`, the committed instruction counts per architecture
  (`benches/baseline/<arch>.json`) and the check that names every bench that grew beyond its
  threshold (2% unless the file or the bench says otherwise), is no longer measured or is not in
  the baseline, all at once (`BENCH_001`); `Rss`, the process's resident memory read from
  `VmRSS` in `/proc/self/status` (kilobytes, so no page size and no `unsafe`).
- `fundamental_bench::Measured::read_summaries`: every callgrind instruction count gungraun wrote
  with `--save-summary=json` under its output directory, each bench named by its summary's
  directory (`<package>/<bench file>/<group>/<function>.<id>`).
- `fundamental_bench::Load`: a call driven in a closed loop at rising concurrency (1, 8, 32
  and 128 by default, each step warmed for a second and measured for 20), every latency in an
  HDR histogram, into a `Report` of `Step`s (requests, errors, throughput, p50, p95, p99 and max)
  written as JSON (`Report::write`) or a Markdown table (`Report::table`) and held to a bound with
  `Report::p99_below` (`BENCH_007`, naming each step that missed) and `Report::error_free`
  (`BENCH_006`).
- `fundamental_bench::Soak`: a call held at one concurrency for a duration (32 for ten minutes
  by default) while resident memory is sampled every ten seconds, into a `SoakReport` whose
  `judge` fails on any failed call (`BENCH_006`) and on the last third's average memory growing
  more than 10% over the first third's (`BENCH_009`), so the warm-up is judged against the
  steady state rather than an empty process.
- `fundamental_bench::Fault`: numbered writes made while a fault is injected, each retried until
  acknowledged, then read back into a `FaultReport` whose `judge` fails (`BENCH_011`) on an
  acknowledged write that is missing, a write found twice, or one the writer never got through,
  with the outage as the writer saw it (first refusal to next acknowledgement).
- `fundamental_bench::workloads` (feature `workloads`): the hot paths both bench harnesses
  measure, each as a setup and a call so they measure identical code: a page of plain Leptos
  parts rendered as a served page renders it (`render`), an `#[action]` mounted by
  `mount_actions!` and dispatched through the router with a JSON body (`action`), YSQL and YCQL
  statements built with ten filters (`query`), a 50-table schema diffed and its next migration
  generated (`migration`), a 100-row pulled page, a pushed row, its signed bytes and the
  WebSocket envelope encoded and decoded (`sync`), `fun.toml` with `fun.production.toml` loaded by
  the config loader and the manifest (`config`), and MessageFormat 2 messages with numbers,
  plurals and selections said in English and Dutch (`mf2`).
- `crates/bench/benches/instructions.rs` and `wall.rs`: each workload counted under callgrind by
  gungraun 0.20 (setup and one warm call before counting starts, the fixture dropped after it
  stops) and timed by criterion, the same call in both.
- `fundamental_bench::live` (feature `live`, built with debug assertions): `BenchApp`, an app
  served in process over a real YugabyteDB and NATS: one page, an `#[action]` that stores a row
  and schedules a job in one transaction and publishes an event under the row's message id,
  a `fundamental_jobs` worker and a sync slice over `SqlSliceStore` and WebSockets; and
  `LiveArgs`, the settings after `--` (`--each 20s`, `--concurrency 1,8,32,128`,
  `--duration 10m`, `--writes 300`). `benches/load.rs`, `soak.rs` and `fault.rs` drive it: each
  part at rising concurrency, the mix for a duration, and writes through a database restart, a
  NATS outage and a network partition, each acknowledged write checked as a row and an event
  exactly once and each job fired. The `bench-live` profile is `release` with debug assertions
  on, which the test containers' fixtures and a plain-text engine connection need.
- `ToRow::clustering_key_orders()`: the order each clustering column is stored in, written by
  `#[derive(Model)]` from `clustering_key = "desc"`. A page read bounds its translations by it;
  a hand-written `ToRow` that leaves the default (empty) reads them across the partition, with
  the same result.
- `fundamental_core::model::sql::{Lint, Level}`: the migration lints, each with a stable name
  (`hot_shard`), a stable code (`MIG_L06`), a default level and the fix its finding says, and
  the three levels a project sets them to: `refuse` blocks with no way out, `warn` blocks until
  the table's model accepts the lint with a reason, `allow` turns it off. Refused by default:
  `table_rewrite`, `key_split_changed`, `not_null_without_default`, `narrowed_type`, because
  each fails on a table with rows or cannot be undone. Warned by default: `destructive`,
  `blocking_index`, `ddl_beside_data`, `hot_shard`, `unindexed_reference`,
  `tenant_not_leading`, `backfill`, `large_colocated`. The words are `refuse | warn | allow`,
  not messages_build's `allow | warn | error`, because this `warn` blocks; aligning the two is
  a separate decision.
- `not_null_without_default` no longer fires on `ADD COLUMN … NOT NULL` to a table its own file
  creates, which holds no row yet, as `SET NOT NULL` there already did not.
- `narrowed_type` judges a `NUMERIC` by its digits before and after the point, so
  `NUMERIC(10, 2)` to `NUMERIC(10, 4)` and `NUMERIC(10)` to `NUMERIC(10, 2)` fire, where only a
  smaller precision or scale did; `FLOAT8` to `FLOAT4` and `TEXT` to `CHAR` fire too, and
  `TEXT` to a `VARCHAR` with no length, which is as unbounded, no longer does.
- `key_split_changed` is judged from the text too: `ALTER TABLE … ADD [CONSTRAINT …] PRIMARY
  KEY (…)`, recognised as `DdlStatement::AddPrimaryKey`, fires on a table its file did not
  create, since YugabyteDB stores rows by their key and writes every row again. The key it adds
  declares its sharding as a `CREATE TABLE`'s must (`GUARD_001`), where the statement used to
  pass unjudged as `Other`.
- `fundamental_db::migration::lints!()` compiles a project's lint levels into its migrator: the
  nearest `fun.toml` at or above the crate is included when it builds (an edit rebuilds it) and
  read by `Lints::from_toml`, so `guard = MigrationGuard::new(1_000, lints!()?, &[])` holds the
  release to the levels the laptop had. `Lints::read(env!("CARGO_MANIFEST_DIR"))`, which the
  first draft named here, read nothing on a host, where the source directory is absent, and
  silently fell back to the defaults. `Lints::read` stays for what runs beside the source (an
  owner's migrations test, the generator), and refuses a `[migrations]` setting written in
  `fun.local.toml` or an environment's file (`LINT_005`), since the levels are the same in
  every environment and a migrator compiles in `fun.toml` alone.
- `fundamental_db::migration::Lints`, the level of every migration lint for one project:
  `Lints::default()` is each lint at its default, `Lints::read(root)` what the nearest
  `fun.toml` sets over it, through `fundamental_manifest`, the one settings loader:
  `[migrations] colocated = true|false` (whether the database was created `WITH COLOCATION =
  true`, default false), `[migrations] tenant = "organisation_id"` (turns on
  `tenant_not_leading`) and `[migrations.lints] <lint> = "refuse" | "warn" | "allow"`. A lint or
  level there is none of is refused at its file and line (`LINT_002`, `LINT_003`). Unlike
  messages_build's policy, the levels are the same in every environment, so a migration a
  laptop refuses is refused in the release. In code: `Lints::default().with(Lint::Backfill,
  Level::Allow).tenant("organisation_id").colocated(true)`.
- A table's placement and writers, generated: `TableSchema::{set_colocated, set_tablets,
  add_writer, accept}` and the snapshot's `colocated`, `tablets` and `writers` (each written to
  `schema.json` only when set; an acceptance is the model's and never written there). A table
  out of colocation is created `… ) WITH (COLOCATION = false)` only in a colocated database
  (`[migrations] colocated = true`), since a table of any other already has its own tablets, and
  one split is created `… ) SPLIT INTO <n> TABLETS`. A writer is made when absent and granted
  `SELECT, INSERT, UPDATE, DELETE` (`SELECT` because an `UPDATE` or a `DELETE` reads what it
  filters on), and revoked all four once its model no longer names it. A table's colocation or
  tablets changed after it exists is refused as `MIG_L02 table_rewrite`, saying to create a new
  table and move to it, since the engine cannot change either in place; a changed key is
  refused as `MIG_L03 key_split_changed`. `generate::next_statements` takes the project's
  `&Lints`; an owner's migrations test reads them from the nearest `fun.toml`
  (`GenerateError::Lints` when they cannot be read).
- `#[model(colocated = false)]`, `#[model(tablets = N)]`, `#[model(written_by(role = "importer"))]`
  and `#[model(accept(<lint> = "reason"))]` on a SQL model. `tablets` with `colocated` left off
  takes the table out of colocation; with `colocated = true` it is a build error, and so is a
  key that does not lead with a hashed part, since `SPLIT INTO` splits by hash only. A writer is
  a lowercase identifier said once, and no role both reads (`read_by`, which narrows its rows)
  and writes. An acceptance names a lint there is (the error lists them) and a reason that is
  one: empty, `why`, `todo`, `tbd`, `fixme`, `ok` and the like are refused.
- The DDL recogniser reads what the lints judge: a `CREATE TABLE`'s columns with their types and
  defaults (`DdlColumn`, an identity column's default read as `identity`), its `WITH
  (COLOCATION = …)` and `SPLIT INTO <n> TABLETS`; whether a `CREATE INDEX` is online (`false`
  only for `NONCONCURRENTLY`); an `ADD COLUMN`'s type; `ALTER COLUMN … TYPE` and `SET DATA TYPE`
  (`AlterColumnType`), `ALTER COLUMN … SET NOT NULL` (`SetNotNull`); and a statement that moves
  rows, `UPDATE`, `DELETE`, `INSERT … SELECT` and `COPY` (`Data`). An `INSERT … VALUES` is a
  seed and stays `Other`.
- The migration lints, judged by `MigrationGuard` over a set's text before any of it runs,
  every finding that blocks reported at once as `GuardError::Linted` (`GUARD_008`), each line
  its code, name, file, what was found and the fix (`GuardError::lints()` lists them). One judge
  a file under `migration::lint::judge`, each tested firing and not firing: `destructive`,
  `table_rewrite` (a column retyped on a table its file did not create),
  `blocking_index` (`CREATE INDEX NONCONCURRENTLY` on such a table), `ddl_beside_data`,
  `hot_shard` (a table or index key led by a time, a serial or a `now()`/`nextval`/identity
  default, split `ASC` or `DESC`), `unindexed_reference` (a `UUID` `<x>_id` that leads no key
  or index where `<x>s` or `<x>` is a table of the set: a heuristic, so it warns),
  `tenant_not_leading` (with `[migrations] tenant`), `not_null_without_default` (also `SET NOT
  NULL` on an existing table), `narrowed_type` (`INT8` to `INT4`, `TEXT` to `VARCHAR(n)`, a
  smaller `NUMERIC`, `TIMESTAMPTZ` to `DATE`), `backfill` (`UPDATE`, `DELETE`, `INSERT …
  SELECT`, `COPY` on a table its file did not create) and `large_colocated` (in a colocated
  database: an outbox, a key on a time or sequence, or one led by the tenant, left colocated).
  A warning passes when the table's model accepts it (`Retirements` carries each owner table's
  acceptances), in the app's own files by `Exemption::accept(Lint::HotShard, "events", "…")`,
  and a destructive statement as before by its owner's declarations or `Exemption::drop_of`. A
  CQL or SQLite set is judged for `destructive` and `not_null_without_default` only.
  `MigrationError::Lints` lets `migrate_main!`'s `guard = MigrationGuard::new(1_000,
  lints!()?, &[])` stop on unreadable settings.
- An owner's migrations test judges the lints too, without a database: the owner's files
  under the project's levels, a warning passing when the table's model accepts it, refused as
  `GenerateError::Linted` with every finding; and `fun migrate generate` writes no file the
  lints would refuse. An acceptance whose lint fires on its table in no file is refused as
  stale, as clippy refuses an unfulfilled `#[expect]`. The generator writes what a model changes
  once its lints pass: a retyped column as `ALTER COLUMN … TYPE` (`table_rewrite`, and
  `narrowed_type` when the new type holds less, both refused by default), a column made
  required as `ALTER COLUMN … SET NOT NULL` and one added `NOT NULL` with no default
  (`not_null_without_default`, refused by default); each refusal names its lints' codes
  (`MIG_L02 table_rewrite, MIG_L10 narrowed_type: column `total` changed its type …`). With
  `destructive = "allow"` the commit-time drop check is off, and with `"refuse"` every drop is
  refused.
- Drift and verify see a table's placement and writers. `SchemaItem::{Colocation, Tablets,
  Writer(role)}` in `Snapshot::differences`, compared where the expected side says them: a
  colocation or tablet count the model leaves to the database is not compared, and a writer is
  a difference when the model names it and the table lets it write nothing. A role the table
  lets write that no model names is not, since a deployment's default privileges grant its own
  login every table; that limit replaces the colocation line in `DRIFT_BLIND_SPOTS`.
  `SchemaDiffer` reports `SchemaChange::ChangeWriter` and `SchemaChange::ChangePlacement`, the
  tablets only when the table has fewer than declared, since the engine splits a growing table
  on its own. `TableCatalog` reads `tablets` (`yb_table_properties.num_tablets`) and `writers`
  (the roles holding `INSERT`, `UPDATE` or `DELETE`, the owner and the connected role
  excepted), and the introspector puts both, with `colocated`, on the `TableSchema`.
- In a colocated database the generator refuses a table that stays colocated with a hashed key
  part, naming the fix (split it `ASC`/`DESC`, or `colocated = false`): measured on YugabyteDB
  2026.1.1.0, the engine refuses one (`42P16 cannot colocate hash partitioned table`), and every
  key a model leaves undeclared leads with `HASH`. `hot_shard` does not judge such a table,
  which has one tablet whatever its key. A container test proves the generated `WITH
  (COLOCATION = false) SPLIT INTO 4 TABLETS` and the writer's grant on a database created `WITH
  COLOCATION = true`, read back by `TableCatalog` as apart, four tablets and that writer.
- `table_rewrite` fires on a column added with a volatile default (`random()`,
  `gen_random_uuid()`, `uuid_generate_v4()`, `clock_timestamp()`, `nextval(…)`) to a table its
  file did not create, which the engine fills by writing every row again; a constant or a
  `now()` default does not fire. The recogniser reads an `ADD COLUMN`'s default for it.
- The lints' table (name, code, default, what fires, how to fix) heads the
  `fundamental_db::migration::lint` docs; the migration module, `migrations!` and
  `migrate_main!` docs say how levels, acceptances, placement and writers are declared.
- `fun migrate rebase [<upstream>] [-p <owner>]`, named after `git rebase`: when main gained a
  migration after this branch wrote its own, each owner's branch files are discarded and
  written again from its models on top of main's, as one file numbered after main's last
  (`removed …/0005_article_standfirst.up.sql`, `wrote …/0006_article_standfirst.up.sql`).
  It works mid-merge and mid-rebase, with `schema.json` still conflicted. The upstream defaults
  to the tip of `origin/main`, not the merge base, because during a merge `HEAD` is still the
  fork point, which lacks main's new file; it is resolved to one commit before cargo runs,
  and one that names none is refused (`CLI_081`, `RebaseUpstreamUnresolved`), saying to fetch
  it or name one. It runs every owner's `migrations` test with `FUNDAMENTAL_MIGRATION_MODE=rebase`
  and `FUNDAMENTAL_MIGRATION_BASE=<commit>`, clearing `FUNDAMENTAL_MIGRATION`; `generate` and
  `regenerate` now clear `FUNDAMENTAL_MIGRATION_BASE` too. `fun migrate regenerate` keeps its
  current use; this is the branch case the plan gave it.
- An owner's migrations test (`generate::the_models_are_their_migrations`) rebases when
  `FUNDAMENTAL_MIGRATION_MODE=rebase` and `FUNDAMENTAL_MIGRATION_BASE=<commit>` are set: main's
  files and snapshot are read through git from the repository the owner's crate is in, the
  branch's files are written again on top of them, and each file removed and written is
  printed. `rebase` without a base is refused naming `FUNDAMENTAL_MIGRATION_BASE`
  (`generate::BASE_VARIABLE`); any other mode is refused as neither `regenerate` nor `rebase`.
- `fundamental_db::migration::generate::rebase(set, &main)` writes an owner's branch migrations
  again on top of main's (`Main { files, snapshot }`, read through git by `Main::read`): every
  file of the set main lacks is removed, and what the models declare beyond main's snapshot is
  written as one file numbered after main's last, named as the newest branch file is, with the
  snapshot after it; when main already makes it, nothing is written and the snapshot is
  main's. It answers `Rebased { removed, written }`. Several branch files become one, because
  the models hold only where they end, not the steps between. Refused, each naming the fix:
  main's snapshot not after main's files (`MainUnreadable`), a main file missing from the tree
  (`MainFileMissing`: merge or rebase main first) or changed in it (`MainFileChanged`), and a
  branch file without the generator's line (`WrittenByHand`), which would be lost. Unlike a
  renumbering rebase, the file is written from the models, so it is right when main touched
  the same table. An owner main holds nothing of yet is left as it is (an empty `Main`), so
  `fun migrate rebase` over every owner does not fail on one the branch added.
- `fun migrate resolve <name> (--retry | --applied) --reason <text> [--app <app>] [--bin
  <bin>]` runs the app's migration binary with `resolve`, exactly one act required and a
  reason with it; `fun migrate status` now prints the binary's ledger (where each migration
  stands, a failed one with its step, statement and the engine's answer). The binary's
  arguments are handed over one word each, never through a shell.
- A migrate binary takes `resolve <name> (--retry | --applied) --reason <text>`:
  `MigrateCommand` now carries its arguments (`Verify { base }`, `Resolve { name, act, reason
  }`) and `MigrateCommand::parse` reads every word after the binary's name, refusing an unknown
  word, an extra argument, both acts or neither, a missing or empty reason and any other flag
  (`--rolled-back`, `--skip`): `run_main(args, run)` prints the usage line and exits `2`. A
  binary built with `migrate_main!` reads who resolves from `SUDO_USER` or `USER` (never the
  command line), asks the SQL wire and then the CQL wire for the name, and prints
  `sql: <name> resolved (retry) by <who>: steps n-m applied`; `status` prints each server
  wire's ledger instead of a count. Every runner call in the expansion is awaited as a
  `dyn Future`, so a binary with every backend stays within the compiler's query depth; a
  binary with a `cql` backend states `#![recursion_limit = "256"]`, as any crate awaiting the
  YCQL driver's futures does. The expansion is now compiled in the workspace's tests for each
  backend set.
- `MigrateRunner::status_sql(pool)` and `status_cql(pool)` return the `MigrationLedger` of
  every migration, embedded and on disk. `MigrateRunner::resolve_sql(pool, name, act,
  &resolution)` and `resolve_cql` settle a failed migration: `Resolve::Retry` runs the failed
  step and those after it again, refused on YSQL while one of those steps builds an index the
  live catalog holds unfinished (`MIGRATE_024`, `UnfinishedIndex`), since `CREATE INDEX IF NOT
  EXISTS` would pass over it; `Resolve::Applied` replays the migrations before it into an
  empty schema (keyspace, on YCQL) beside the app's, made with the app pool's configuration and
  removed after, reads the catalog, applies it there and reads it again, and records it applied
  only when the live catalog holds every table, column, key, index and check that changed, an
  unfinished index counting as missing (`MIGRATE_019`, naming each); a migration with no
  catalog effect is refused for `--applied` (`MIGRATE_020`); a name no file has is
  `MIGRATE_003`. The migration itself is replayed one step at a time (`StepReplay`), and every
  step from the failed one on must change the catalog or leave nothing behind (`SELECT`,
  `ANALYZE`, `VACUUM`, `SET`, `RESET`, `SHOW`, `EXPLAIN`): a backfill `UPDATE` after an `ADD
  COLUMN` is refused for `--applied` naming the step (`MIGRATE_023`, `StepNotInCatalog`),
  since the column is in the catalog whether or not its rows were written. The failed step is
  read before the replay, so a migration with nothing to settle is refused (`MIGRATE_017`)
  without making a scratch schema. `apply_sql` and `apply_cql` return a ledger refusal as
  `MigrationError::Ledger`, so a caller can match `Unresolved`.
- `fundamental_cql::drop_keyspace_with_tables`, out of `test_support`: the one statement
  sequence that removes a keyspace on both CQL engines (YugabyteDB refuses `DROP KEYSPACE` on
  a keyspace with tables), for the migrator's scratch keyspace as well as the test teardown.
- `CqlMigrator` records each statement of a migration as a step in the keyspace's
  `_fundamental_migration_steps` under the migration lease, as it starts and as it ends, and a
  failure with the error's code, its message and whether the answer was lost (a timeout or a
  broken connection; an unavailable replica or a refusal is an answer). While a pending
  migration has a failed step, `apply_all` refuses before running anything (`MIGRATE_016`); a
  step done with the same statement is skipped. `CqlMigrator::ledger`, `retry` and
  `mark_applied` match `SqlMigrator`'s (`ledger` reads only the ledger tables the keyspace
  has, so `status` creates nothing), recording each act in
  `_fundamental_migration_resolutions` under the lease (YCQL has no transaction to share with
  the act's ledger change, and no `current_user`, so no login is recorded). The introspector
  leaves out all three ledger tables. `CqlError::MigrationFailed`'s position is now `step`,
  beside `total` and the `statement` text, and `CqlError::Ledger` carries a `LedgerError`.
  The offline (SQLite) runner keeps its single-ledger shape: an on-device store has no
  operator to resolve a failed step, and a failed migration there rolls back whole.
- `SqlMigrator` runs a migration one statement per step, each committed with the row that
  records it done in `_fundamental_migration_steps` (name, step, total, statement hash,
  `running`/`done`/`failed`, the SQLSTATE and the engine's message, whether the answer was
  lost, when), and records the migration in `_fundamental_migrations` (unchanged in shape)
  with its last step. A step is recorded as started before its statement is sent, so a
  migrator that stopped mid-step is recorded as failed with its answer lost by the next one;
  a step already done with the same statement is skipped, so a run carries on where the last
  stopped, except that a done `SET` or `RESET` runs again on the new connection, since the
  steps after it rely on the setting (a `SET statement_timeout` before an index build); a
  `CONCURRENTLY` statement runs outside the step's transaction. The migration lock
  is a session advisory lock held on one connection for the whole apply, which is closed
  rather than pooled afterwards, so two replicas never interleave steps. A store with nothing
  pending is read without the lock and without DDL, as before. While a pending migration has a
  failed step, `apply_all` refuses before running anything (`LedgerError::Unresolved`,
  `MIGRATE_016`): an unattended deploy that ran a timed-out statement again under the same
  load would repeat the outage. A conflict the 24-attempt budget did not outlast rolled back
  and is not recorded. Why: on YugabyteDB, DDL inside a transaction block is not atomic unless
  transactional DDL is on, so one transaction per migration could leave its first statements
  applied with nothing recorded (Prisma's P3009 state).
- `SqlMigrator::ledger(pool, migrations)` reads the `MigrationLedger` without the lock or any
  DDL; `SqlMigrator::retry(pool, (name, sql), &resolution)` runs the failed step and those
  after it again, refused (`MIGRATE_018`) when the failed step's answer was lost and its
  statement cannot run again; `SqlMigrator::mark_applied(pool, (name, sql), &resolution,
  &proof)` records it applied on a `CatalogProof`. Each records the act in
  `_fundamental_migration_resolutions` (act, who, why, the database login `current_user`, the
  step and SQLSTATE it settled) in one transaction with the act's own ledger change; a name
  with no failed step is `MIGRATE_017`.
- `SqlError::MigrationFailed` names `step`, `total` and the `statement`, as the step ledger
  does, and `SqlError::Ledger` carries a `LedgerError`. The introspector leaves out, and the
  pooled-schema wipe keeps, all three ledger tables.
- The migration ledger's vocabulary in `fundamental_db::migration`: `MigrationLedger` (every
  migration on disk with its `LedgerState`: applied with its time and the resolution that
  settled it, failed with its `FailedStep` — step n of m, the statement from the file, the
  engine's code and message, whether the answer was lost — running, pending, or blocked behind
  a failed one), whose `Display` is what `migrate status` prints; `StepRecord`/`StepState` for
  the step table; `Resolve` (`Retry` or `Applied`, and no third act), `Resolution` (who, read
  from `SUDO_USER` then `USER` and never given, and a non-empty reason) and `Resolved`;
  `CatalogProof::check`, which proves a migration's effect (the catalog before it and after it,
  replayed) is in the live catalog, an index whose build did not finish counting as missing;
  `LedgerError` (`MIGRATE_016`–`MIGRATE_022`), reached as `MigrationError::Ledger`; and the
  ledger's table names with `is_ledger_table`. It replaces `MigrationStatus`,
  `format_status` and `MigrationManager::status`, which read nothing from a database.
- `ddl::why_not_rerunnable(statement)`: whether running a migration statement a second time
  leaves what running it once left, judged by its shape. `CREATE … IF NOT EXISTS`,
  `CREATE OR REPLACE`, `DROP … IF EXISTS`, an `ALTER TABLE` whose every action is
  `ADD COLUMN IF NOT EXISTS`, `DROP … IF EXISTS` or an `ALTER COLUMN … SET/DROP/TYPE`, an
  `INSERT … ON CONFLICT`, `GRANT`, `REVOKE`, `COMMENT ON`, `SET` and `SELECT` run again; a
  `RENAME`, an `ADD CONSTRAINT`, a `CREATE` or `DROP` without its guard, an `UPDATE`, a
  `DELETE`, an `INSERT` without `ON CONFLICT`, a `DO` block and anything else are named with
  the reason. `migrate resolve --retry` asks it of the step that failed.
- `fun migrate verify [--app <app>] [--base <revision>]`: one step for CI and a developer's
  machine (after `fun local up`) that runs each app's migration binary with `verify <commit>`,
  replaying its SQL migrations from scratch and as the upgrade from the base, and failing on
  any difference from the committed snapshots and the models. The base defaults to
  `git merge-base HEAD origin/main` and is resolved to one commit before any binary runs; with
  no `--app`, every app with a migration binary is verified, each even when another fails.
  Exit codes match `check` and `apply`; a base that names no commit is `CLI_079`, and no app
  with a migration binary is `CLI_080`. `fun new` writes no CI file yet, so a generated
  project adds the step to its own CI.
- `MigrateCommand::Verify`: a migrate binary takes `verify <base>`, and `run_main`'s usage
  line is `apply | status | check | verify <base>`. A binary built with `migrate_main!` runs
  `MigrateRunner::verify_sql` with its SQL connection for it, with no change to the app's
  code; an app with no `sql` backend is refused (`MIGRATE_011`).
- `MigrateRunner::verify_sql(pool, base)` replays every SQL migration into two empty schemas
  beside the app's, made with the app pool's own configuration and removed after: once from
  scratch, and once as the real upgrade, each file as the commit `base` holds it (read with
  `git`) and then the branch's new files, through the production runner and guard. Both
  catalogs are read back and compared with the union of the owners' committed
  `migrations/schema.json`, with the models and with each other, and every difference is named
  (`MigrationError::Unverified`); an index whose build did not finish is named too. Before
  anything runs, a file the base holds that the branch changed or removed, or a new file
  numbered before one at the base, is refused. A set outside the project's repository, or one
  not generated from its models, counts as already at the base, and a table such a set made is
  compared only with the models. Main's binary is not built: its files are applied by the
  branch's runner, one build instead of two, since an applied file never changes.
  `Verification` says what was replayed.
- `MigrationError` gains what `fun migrate verify` refuses, each with its code:
  `Unverified` (`MIGRATE_011`, every `SchemaDifference` a replay found), `AppliedFileChanged`
  (`MIGRATE_012`), `FileBeforeBase` (`MIGRATE_013`), `BaseFileGone` (`MIGRATE_014`) and
  `BaseUnreadable` (`MIGRATE_015`). Each file refusal names every file of its kind at once.
- `SqlIntrospector` reads a covering index's `INCLUDE` columns into `TableIndex::include`
  (`CatalogIndex::include` names them), so a schema read back compares them; before, they read
  as none. `SqlIntrospector::unusable_indexes(pool)` names each index whose build did not
  finish, which keeps its name and which the planner never uses.
- `Snapshot::differences(&found)` names every way a schema found differs from a snapshot as
  `SchemaDifference`s: a table either side lacks, a column's type, nullability or default, the
  key's split (`(organisation_id HASH, id ASC)` against `((organisation_id, id) HASH)`), an
  index's key, split, uniqueness, condition or `INCLUDE` list, a check's expression and a
  reader's rows, each printed with the expected and the found value. Values are compared as the
  engine means them (`INT8` is `BIGINT`, `kind IN ('a')` is `kind = ANY (ARRAY['a'::text])`,
  `char_length(s) BETWEEN 1 AND 9` is `char_length(s) >= 1 AND char_length(s) <= 9`), and
  column order is not compared, since a later column is always appended.
- `App::sync(register_slices)` with `App::projections(register_projections)`: the app's live
  slice-sync server over its default SQL pool at `[apps.<name>.sync]`, built from the pools and
  the token verifier the app already has, so a generated `main` no longer wires a store, a
  verifier and `LiveSyncServer` by hand. Its tables are checked and its verifier loaded when
  the runtime starts it; a check resolves its address and pool and connects nothing. A slice
  that asks for change capture is still refused (`SYNC` `NoChangeCapture`), as before. Behind
  `fundamental_app`'s `sync` feature, which the umbrella's `sync_server` turns on beside `app`.
  With its `cql` feature (the umbrella's `cql`) an app that declares no SQL database syncs over
  its default CQL keyspace instead, its capture tables derived from the mounted slices and its
  outbox made when the server starts; the connections are kept until the server has stopped.
- `App::sync_store(SyncStore::Cql)` names the store an app's sync server uses when it
  declares both engines; without it the SQL pool wins. A store the manifest declares no
  database for is refused by a check (`AppError::SyncStoreUndeclared`, `SERVE_005`). `fun new`
  and `fun add app` with `--db sql,cql --sync-store cql` write the line, so the explicit choice
  is kept rather than syncing over SQL.
- `App::worker(register_workers)`: the app's `#[subscribe]` handlers, from `mount_workers!`,
  run as a worker under the app's one runtime. It connects to NATS from `[messaging.nats]`
  and the secret store when the runtime starts it, never when the app is built: a check
  resolves its messaging settings, refusing an app with none, and opens no connection.
  `App::messages(describe_messages)` hands the dev tools its messages. Behind
  `fundamental_app`'s `worker` feature, which the umbrella's `nats` turns on beside `app`.
  `App::provide_to_workers(|served| ..)` gives every handler a value built from what the app is
  served with, such as the `Inbox` an inbox handler needs, when the worker starts and never
  when it is checked.
- `App::socket(|router| router.route(on_ping_route()))`: the app's WebSocket server at
  `[apps.<name>.socket]` under its one runtime, with `App::socket_config(WsConfig::public)`
  for its settings beyond the address. Behind `fundamental_app`'s `socket` feature, which the
  umbrella's `ws` turns on beside `app`.
- `App::api(register_api)`: the app's HTTP API, its routes over the same state as its pages,
  served by its own listener at `[apps.<name>.api]` under the app's one runtime; the dev tools
  ride it when the app serves no pages. `App::api_router()` is what a test answers API
  requests with, as `router()` is for pages.
- An `App` runs the surfaces its settings enable and builds no other: `App::surfaces(&settings)`
  names them, each one the app declares a builder for that `FUNDAMENTAL_SURFACES` (or
  `enabled` in `[apps.<name>.<surface>]`) lets run, so a disabled one binds no port and opens no
  connection. What runs is chosen by configuration, never by code: every surface in one
  process in development, one unit per surface in production. `.pages(..)` is optional, and an
  app without it binds no web listener. The admin listener runs whenever
  `[apps.<name>.admin]` declares it (name `admin` in `FUNDAMENTAL_SURFACES` for the one unit
  that answers health), publishing the app's name when `.admin(..)` declares nothing more.
- `AppSettings::with_surfaces(["worker"])` runs only the surfaces named, as
  `FUNDAMENTAL_SURFACES` does, for a test or an embedder that should not set the process
  environment; `AppSettings::declares_surface("admin")` says whether the app declares one.
- `mount_workers! { on_hello, billing::book }` lists the `#[subscribe]` handlers an app's
  worker runs and generates `register_workers(&WorkerContext)`, which starts each one's
  listener with what its form takes, and `describe_workers()`. Every NATS form of
  `#[subscribe]` now emits `{fn}_worker(&WorkerContext)` beside `{fn}_listener`, so a core,
  set, JetStream and inbox handler start the same way; an inbox handler takes the `Inbox`
  provided to the context (`WorkerContext::provide`), and is refused with `NATS_026` when none
  was. A path that names no handler does not compile. Separate from `mount_messages!`
  because messages are types and workers are functions, as `mount_responders!` already lists
  responders; an explicit list, like `mount_actions!`, rather than a link-time registry.
- `ReceivedMessage::delivered(payload)`: a message as a subscriber receives it, on its
  subject with no reply and no headers, so a test calls a `#[subscribe]` handler directly,
  `on_hello(ReceivedMessage::delivered(Hello { .. })?).await`, with no NATS server.
  `Inbound::delivered(event, delivery)` is the same for an inbox-form handler.
- An `App` whose manifest declares no `[apps.<name>]` is refused when it is served or checked
  (`AppError::Undeclared`, `SERVE_003`), naming the key: a renamed crate or section fails at
  once rather than starting on defaults it was never given.
- An `App` told to run a surface it does not run (`FUNDAMENTAL_SURFACES=worker` for an app
  whose `app()` has no `.worker(..)`, or a misspelt name) is refused when it is served or
  checked (`AppError::SurfaceNotRun`, `SERVE_004`), naming the surface: a production unit
  would otherwise start, run nothing it was given and look healthy.
  `AppSettings::named_surfaces()` gives the names the process was told.
- `AppSettings::load("my_svc")` loads `[apps.my-svc]` when the manifest declares no
  `[apps.my_svc]`: a crate's name (`CARGO_PKG_NAME`) is its package's with each `-` read as
  `_`, as Cargo and `fun new` write it, so the name an app's own crate gives finds the section
  its app was declared under. The exact key still wins, and two keys that both spell the name
  find neither.
- `#[action]` over a generic state: `async fn browse<S: NewsroomReader>(ctx: HttpContext<S>,
  query: ArticleQuery)` implements `ServerAction<S>` for every `S` meeting the handler's
  bounds and builds `browse_route::<S>()`, so a domain publishes one action over a
  capability trait and each application serves it by implementing that trait for its state
  and listing the marker in `mount_actions!`, with no turbofish at the mount. The marker,
  its `Action`, `path()` and the browser stub stay free of the state. The handler is
  refused, with the reason, when it is generic over anything else, takes a lifetime or
  const parameter, or names the state in its input or output, which cross to the browser.
  Chosen over a runtime lookup of the state (Leptos `use_context`, Actix `web::Data`),
  which fails per request rather than when the application compiles.
- `Retirements` and `MigrationGuard::inspect_sql_passing`: a destructive statement in an
  owner's embedded file (`<owner>/<file>`) passes the guard when the owner declares it, with
  nothing new written for a generated file: a column, check or index retired on the model
  (`DROP COLUMN`, `DROP CONSTRAINT`, `DROP INDEX`), a table in `retired = […]` (`DROP TABLE`),
  a column whose `was` names the old one (`RENAME COLUMN`), a rename whose new name was later
  retired or renamed again, a reader's policy whose role the model no longer `read_by`
  (`DROP POLICY t_read_by_r ON t`, and `t_written` once no role reads `t`: the file the
  generator writes when `read_by` is taken off), and any statement of a file named in
  `drops_by_hand`. A `DROP POLICY p ON t` is named `t.p`, and a `DROP INDEX CONCURRENTLY i` is
  named `i`, not the keyword. One owner's
  declarations pass nothing of another's, and a file of the app's own directory still needs
  `Exemption::drop_of`. The model is where the acknowledgement lives, as Rails
  strong_migrations reads `ignored_columns`, rather than a comment in the SQL (squawk, Atlas)
  or a whole-run flag (Prisma's `--accept-data-loss`). `inspect_sql` passes no declarations.
  `SqlMigrator::passing(retirements)` judges a set through them, and `MigrateRunner` passes
  its registered sets' declarations in `apply_sql` and `apply_sql_through`, so an app's start
  refuses only a drop nothing declares.
- `generate::check`, each owner's fast migrations test, makes the guard's judgement without a
  database: a file that drops or renames what no model declares fails it
  (`GenerateError::UndeclaredDrop`, naming the fix: retire it, say `was`, or
  `drops_by_hand = ["<file>"]`), and so does a file named in `drops_by_hand` that is not one of
  the owner's or needs no naming (`DropsNothingByHand`). So an undeclared drop fails the owner's
  commit, never an app's start.
- `migrations!(…, drops_by_hand = ["0002_…"])` names an owner's files that drop or rename by
  hand what no model retires or says `was`, as `needs_createrole` names files: written before
  the generator, they are the one place the owner says so
  (`EmbeddedMigrations::dropping_by_hand`, `files_dropping_by_hand`).
- A YCQL or offline model with `Localized` text names its companion table:
  `ModelSchema<CqlDb>::translation_schema()` and `ModelSchema<Offline>::translation_schema()`
  are generated, as `ModelSchema<SqlDb>`'s already was. `migration::render::create_cql_table`
  and `create_sqlite_table` write a YCQL or SQLite table schema as its `CREATE TABLE`.
- The messages build and `fun messages check` refuse a call the runtime cannot make, naming the
  file and the message, in every language: a function outside `:string`, `:integer`, `:number`,
  `:percent`, `:currency` and `:brand` (MF2's own `:datetime` too, which the runtime does not
  register), and `:brand` with no operand, a variable, or a literal outside `name`, `legal-name`,
  `site-domain` and `platform-domain`. Such a message compiled before and said nothing when
  read. `fundamental_messages_build::FUNCTIONS` and `BRAND_TERMS` name what is accepted.
- A message says the brand with the MF2 function `:brand`: `{name :brand}`, `{legal-name :brand}`,
  `{site-domain :brand}`, `{platform-domain :brand}` (`fundamental_messages::BrandTerm`). The words
  come from the one `BrandTerms` the process installs with `install_brand_terms`; a second install
  is refused (`BrandTermsInstalled::Already`, `MESSAGES_001`), so no catalog can say another
  brand. With no brand installed the message says nothing and the caller's own words answer
  (warned once), never a sentence with a hole in it. This replaces Fluent's `-brand-name` term,
  which MF2 does not have: no message spells the brand and no call site passes it. A custom
  function over a closed vocabulary, rather than a general `:term` with app-named terms, so the
  messages build can refuse a misspelt one.
- `Router::live_reload()` (and `live_reload_when(bool)`, `serves_live_reload()`): in development
  every open page learns when the bundle it loads changes on disk. It mounts
  `GET /_fundamental/live`, a same-origin server-sent-events stream (`hello` with the process's
  boot id and `retry: 250`, so a page reconnects to a restarted server in a quarter second rather
  than a browser's default seconds, then `css <stamp>` when only stylesheets changed and `reload` for a script or wasm),
  and serves its client at `/_fundamental/live.js` from memory, so the CSP needs no exception. A
  stylesheet swaps in place; any other change, or a reconnect to a restarted server, reloads the
  page. Each connection polls the disk-served mounts every 200 ms on its own thread, holds a
  change 100 ms so both `wasm-bindgen` files land first, and ends with the page. Same-origin SSE
  rather than a dev server's websocket on its own port, so it works through the local HTTPS proxy
  and adds no dependency; a router with no disk-served mount mounts nothing. A handler reads
  `HttpContext::serves_live_reload()`, a page `PageContext::live_reload()`, and
  `PageContext::hydration_script` then adds `<script type="module" src="/_fundamental/live.js">`
  by itself, so every shell (Site's `Document`, an app's own) loads the client with no code
  change. The free `hydration_script(url)` has no router to ask and never adds it.
- `Router::end_open_streams()` ends every stream the router holds open, today each open page's
  live reload stream, and the server backend calls it as it begins to stop. A graceful stop waits
  for every response to finish, and a page left open in a browser never finishes its stream, so
  every `fun dev` restart sat out the dev loop's 10-second grace before being killed. The page
  reconnects to the next server and reloads on its new boot id.
- A `Site` in development serves its bundle from disk (`StaticSource::for_development`) and
  turns on `Router::live_reload`, so a stylesheet `fun dev` rebuilds is served at once (it was held
  in memory from startup, and a CSS-only edit restarts nothing) and swaps in the open page.
- `fun new` gives a web app beside another server surface (a worker, a socket, an API) the same
  live reload: its generated `main.rs` mounts the bundle with
  `static_dir_from(.., StaticSource::for_development(development))` and
  `.live_reload_when(development)`, and its `pages/document.rs` calls
  `ctx.hydration_script(..)`, which adds the client when the router serves it; `fun add surface web`
  prints the same wiring.
- An app's islands in a crate of their own, by convention: when `apps/<app>/islands/Cargo.toml`
  exists (package `<app>_islands`, `fundamental::build::IslandsCrate`), `fun build` and `fun dev`
  compile the browser bundle from it, still named `<app>.js`, so a page or view edit rebuilds the
  server alone. Without the directory the bundle is built from the app crate as before; a native
  webview bundle is always built from the app crate. In such an app `PagesBuild` hydrates no page
  and refuses an `#[island]` in a page, layout, 404 page or gate (`BUILD_040`), naming each file:
  nothing compiles it to wasm any more, so it would render on the server and never hydrate.
  `fun new` writes `apps/<app>/islands/` for a web app (not one that syncs or runs native, whose
  browser build is the app crate's) and `fun add surface web` writes it beside an existing app,
  each registering it as a workspace member: a `lib.rs` that links the domain and exports
  `hydrate`, and a `Cargo.toml` whose browser dependencies sit behind its own `hydrate` feature,
  which the wasm build turns on, so a workspace build never unifies Leptos's `hydrate` with the
  server's `ssr`.
- `fundamental_site` (`fundamental::Site`, umbrella feature `site`): one `Site` value per app,
  declared once and served by its binary's whole `main`, `fn main() -> ExitCode { app::site().run() }`.
  `Site::new(name)` (or `site!(name)`, which names the bundle beside the calling crate and its
  version) takes `.catalog(fn)`, `.languages(LocalePolicy)` (which installs
  `LocaleChoice::with_arrivals()` and refuses a policy that reads no cookie, `SERVE_001`),
  `.appearance(PreferencePolicy)` (written as `<html data-theme>`), `.addresses(fn)`,
  `.pages(register_pages)`, `.look(fn)`, `.icons(&[SiteIcon])`, `.document(Document)`,
  `.font_source` and `.image_source`, `.indexing(Indexing)`, `.unlisted(prefix)` (kept out of the
  sitemap and answered with `X-Robots-Tag: noindex`), `.state(|served| ..)`, `.admin(|app| ..)`
  (any `ServiceDeclaration`) and `.part(impl SitePart<S>)`. Building one does no I/O.
  `Site::router()` is the router the binary serves, short of its parts, for a test to answer
  requests with; `Site::listed_pages()` the route table's own fixed-path pages a sitemap lists.
  `Site::serve()` loads the configuration under the bootstrap subscriber and installs its
  telemetry (with the dev tools' drain under `introspect`), installs the catalog, connects the
  databases, serves the bundle from the release's package (`App::web_bundle_dir`), mounts the dev
  tools, builds the state, installs each part, and runs the admin listener (probing the default
  pool), the web listener and each part's surfaces under one `Runtime`. `Site::check(&app)` builds
  all of it over unconnected pools and starts nothing; `Site::run()` dispatches
  `<app> check-config [manifest]` to `check_release` and `check`, serves otherwise, and exits with
  the error's exit code (`1` when that code is `0` or out of range, so a failure never exits as a
  success). A site with no state is refused (`SERVE_002`). `SitePart` is the public
  seam a kit implements later (`install`, and `surfaces` and `check` with empty defaults): a
  published site's robots and sitemap documents and its record sources, a mail sender, audit
  sealers. Rendering `sitemap.xml` and `robots.txt` and the record sources stay in their kit; the
  public side lists the route table's pages (`Served::listed_pages`), so an app types no list.
  A separate `site` feature rather than part of `web`, because serving a site connects databases
  and installs telemetry, which a `web` build such as a native app's pages needs neither of.
- `LocalePolicy::with_region(Locale::new("en", Some("GB")))` and `regional(&locale)`: the region
  each offered language is written for, as a shared link's `og:locale` and a date say it.
  Negotiation and addresses are unchanged, so a page stays at `/` rather than moving to `/en-gb`.
- `Router::page_paths()`: every page the router serves at a fixed path (a `GET` route answering
  `text/html` whose template captures nothing), sorted: the route table's own pages, as a sitemap
  lists them, so no app types that list by hand.
- `fundamental_web::Document`: the HTML shell every page of a site wears, declared once and
  installed on the router (`Document::install`), so an app needs no `pages/document.rs`:
  `default_document` wraps each page in it. It writes the style and font links (`.head(fn)`), the
  icons (`.icons(&[SiteIcon])`, also served at their paths by `install`), `<html data-theme>` from
  a preference (`.theme(name)`; no attribute when the reader pinned none), the title of a page
  that names none (`.title(site_name)`; a page's own title wins), the page's own head and
  the hydration script (`.bundle(js_path)`). `SiteIcon` is one icon, served and linked from one
  `const` list. `ShareCard::new(site_name)` writes `og:type`, `og:site_name`, `og:title`,
  `og:description`, `og:url`, `og:locale` in the region the site writes the page's language in
  (`en_GB`) with each other language as `og:locale:alternate`, and a JSON-LD `WebPage` only when
  the page has a canonical URL. `Document::locales` holds each language's region, and
  `PageContext::regional_locale()` gives the page's language in it (`en-GB`), for a date written
  by region. `PageContext::with_document` and `document()` carry the installed shell.
- `Router::with_extension(value)` and `HttpContext::extension::<T>()`: a value the whole surface
  hands every handler, one per type, such as the document shell every page wears. Typed at both
  ends, as `http::Extensions` is, rather than a map of names two libraries could collide on.
  `HttpContext::with_extension` attaches one in a handler's own test.
- `.at(..)` on a generated action hook:
  `actions::use_sessions().at(move || Sessions::path(handle.get()))` calls an action whose path
  holds captures at the path its marker fills. In the browser the hook
  fetches that path (`ClientAction::call_at`); on the server the dispatcher runs the route at it,
  its segments resolved and its guards run. A query reads the path again when what it reads
  changes; a mutation reads it when it submits. It combines with `.source(..)`, `.local()` and
  `.blocking()` in any order. Captures stay in the path, never in the input, as Axum's `Path` and
  `Query` are apart; a capture action used without `.at(..)` is refused at run time, not at
  compile time, to keep the hook's surface small.
- `ActionDispatcher::dispatch_at::<M>(target, body)`: a page's server render calls a mounted
  action at its own path, so `ctx.param` and an object guard see the action's captures, not the
  page's, and a `GET` or `DELETE` action reads its input from the target's query. A path the
  action's route does not serve is refused with `ServerError::ActionPathMismatch` (`API_034`).
  The router's declared segments resolve on the dispatched call before the action's own chain,
  so its guards read the record's id, as over HTTP.
- `fundamental_web::ActionRequest`: one action call shaped for the wire. A `GET` or `DELETE`
  input goes in the query string, urlencoded as the server decodes it (`()` adds none); any other
  method's input is a JSON body. A path that still holds a `{capture}` is refused with
  `WebClientError::Transport`, naming each capture and `.at(..)`.
- `ClientAction::call_at(path, input)`: calls an action at its `ActionPath`, its path with each
  capture filled by the marker's `path(..)`; `call(input)` keeps the declared path.
- `fundamental_core::action::ChangeId`: the id of one change a write made, serialized as its
  UUID string, so a page can undo exactly the change its write answered.
- `fundamental_core::action::Outcome<T>`: what a write answers, `{changed, change, said}`, built
  with `Outcome::changed(value).change(id).said(words)` or `Outcome::nothing()`.
- `Error::field_errors`: the refusal of each field an error names. `ValidationErrors` answers
  every error it holds; `ErrorOverride::override_field_errors` passes them on through `AnyError`,
  `#[overrides]` and `#[error(transparent)]`.
- `#[derive(Error)]` takes `#[error(field = "email")]`: the variant answers one field error at
  that field, with its user-safe message, and keeps the field when converted to a
  `ValidationError`. An `#[overrides]` field that refuses fields of its own refines it; one
  that refuses none leaves the declared field in place.
- `ServerResponse::from_error` adds `"fields": [{field, code, message}]` to the error envelope
  when the error refuses fields, each with its user-safe message; an error that refuses none keeps
  the envelope as it was.
- `WebClientError::from_error_response` decodes the envelope's `fields` into `RemoteFault`
  (`RemoteFault::fields`, `with_fields`), so `field_errors()` answers them in the browser.
- `MutationScope::refusal(field)`: the words that refuse one field in the latest answer, to draw
  beside it; `MutationScope::outcome()`: the latest write's change id and words.
- A page reads again after a write: a `<Mutation>` whose write went through bumps a count of
  writes that `<AsyncDefaults>` and `AsyncDefaultsConfig::provide` hold, and every query built by
  `use_query!` or `use_query_prefetch!` (so every `<Query>` over an action hook) tracks it and
  refetches. No keys, as Remix revalidates its loaders after an action. An island that writes
  and queries installs its defaults with `config.provide()`.
- A streamed page waits for its blocking resources before it answers: `render_page_stream`
  takes the body's first chunk once every blocking resource has resolved, then reads the status,
  headers and title the page set, so what a blocking resource's content sets is in the response.
  A page with none sends its shell and fallbacks at once, as before. As Leptos's own integrations
  do before they read `ResponseOptions`.
- `.blocking()` on a server query hook (`actions::use_<name>().blocking()`, before or after
  `.source(..)`): the page's status, headers, `<title>` and first chunk wait for it, and its
  content is sent in place with no fallback, so a page's primary query names it. Built with
  Leptos's `Resource::new_blocking` (`use_query_prefetch!(@blocking ..)`); every other query keeps
  streaming. There is no `.local().blocking()`: a browser-only query cannot hold the server's
  answer, and the type does not offer it.
- A blocking query that fails answers the whole page as a loader's `PageRefusal` does: a `401` is the
  gate (the step-up gate for `PageError::StepUpRequired`), any other client status the not-found page (`404`), a server status the error page,
  logged once. A page refuses one way whether its loader or its primary query refused, as
  Next.js's `notFound()` and SvelteKit's `error(404)` do.

### Changed

- A YCQL page read (`CqlQuery::execute`) reads its records' translations in one companion
  query instead of one per record: two round trips for a page, where there were one plus one
  per row. The page comes from one partition, so the read is by that partition, bounded to the
  range the page's first clustering column covers, and each translation is matched to its
  record on the clustering values the database sent for both, grouped in a map; the
  `cql/page_translations` bench (`--features internal-benches --bench translations`) measures
  that matching linear: 2.9 µs, 31.7 µs and 328 µs for 10, 100 and 1,000 records. The agreed
  design had the derive generate a key comparison over the key fields' Rust types; that would
  have needed `PartialEq` on each of them, the very bound it was to avoid, so the wire values
  are compared instead and a model needs no new bound. A projection without the clustering
  columns still reads each row's translations by its key. Every scalar key kind is hashed by
  its value, a `timeuuid` time-series key included: hashed by its kind alone, a whole page
  would share one bucket and be matched in quadratic time.
- `OfflineStore::all`, `query_filtered` and `hydrate_translations` match translations to their
  records through a map keyed by the record's key, linear in rows plus translations, where a
  scan of every translation per record was quadratic. Measured by the `offline/all_translated`
  bench (`cargo bench -p fundamental_offline --bench store`, pinned to one performance core),
  each record with two translations: 10 rows 33.8 µs to 34.9 µs, 100 rows 346 µs to 320 µs,
  1,000 rows 8.87 ms to 3.14 ms, 5,000 rows 140 ms to 16.4 ms. The key is compared as stored,
  a `REAL` by its bits, so `db::Value` keeps no `Eq` that NaN would break. A record given twice
  in one slice is hydrated both times.
- `MigrationGuard::new(min_hash_fanout, lints, exempt)` takes the project's `Lints` where it
  took a `DestructivePolicy`: `DestructivePolicy::Refuse` is `Lints::default()`, and
  `DestructivePolicy::Allow` is `Lints::default().with(Lint::Destructive, Level::Allow)`.
  `MigrationGuard` is `Clone`, no longer `Copy`, since the levels are read at start. A
  destructive statement and a `NOT NULL` column added with no default are the lints
  `destructive` (warn) and `not_null_without_default` (refuse), refused as `GuardError::Linted`
  in place of `GuardError::Destructive` (`GUARD_005`) and `GuardError::UnbackfillableColumn`
  (`GUARD_006`), whose codes are left unused. An app's `migrate.rs` writes
  `MigrationGuard::new(1_000, lints!()?, &[])`.

- An owner's migrations test (and so the commit hook) reports a stale snapshot as `<owner>:
  migrations/schema.json is not the schema after its last migration; if main gained a
  migration after this branch wrote its own, run `fun migrate rebase -p <owner>``, instead of
  saying to regenerate the newest file by hand. A `schema.json` a merge left conflicted
  (`<<<<<<<`) and two files that share a number are reported the same way, for their owner,
  where a conflicted snapshot used to be an `Unreadable` error naming none, and two files of
  one number passed when the merge kept the snapshot of the one that sorts last.
- `fundamental_db::migration::Revision` is public: the repository at one commit, read through
  `git` with a hook's `GIT_*` variables cleared. It moved from `fundamental_db_migrator`, whose
  `migrate verify` uses it from there, so one reader of a commit serves `migrate verify` and
  `fun migrate rebase`; `Revision::snapshot(dir)` reads a crate's `migrations/schema.json` as
  the commit holds it.
- `DRIFT_BLIND_SPOTS`, which `fun migrate check` prints, names what the drift comparison truly
  cannot see: foreign keys and colocation and tablets, which a model does not declare yet, and
  functions and triggers, which no table's shape holds. It no longer says keys, defaults and
  indexes go uncompared, since they are compared.
- `fun new` writes every app, whatever its surfaces, as `pub fn app() -> fundamental::App<()>`
  in its library, one builder line per surface (`.pages`, `.api`, `.worker`, `.socket`,
  `.sync`) beside each surface's handlers and mount macro, and the same one-line
  `fn main() -> ExitCode { <crate>::app().run() }`. The hand-built `#[tokio::main]` that
  wired a `Runtime` and gated each surface on `surface_enabled` is gone, and with it a web
  app's `pages/document.rs` (its pages wear the app's document shell) and the per-app tracing
  setup (the `App` installs telemetry). An app crate asks for `fundamental`'s `app` feature
  and each surface's, so the workspace always carries the `sqlx-sqlite` patch.
- `fun add surface worker|socket|api|sync` adds the surface's one builder line to `app()` in
  the app's `lib.rs`, right after `fundamental::app!()`, with its handlers, its mount macro
  and its `fundamental` feature, and leaves `main.rs` alone: the line is generated, so the
  command writes it rather than printing five to fifteen lines of `Runtime` wiring. `sync`
  also asks the domain for `sync-server`, where `fun new` asks for it: in the `domain`
  dependency's features, or a web app's `ssr` list. A library
  with no `app()` built by `fundamental::app!()` is the developer's own and is left as it is;
  the exact line is printed instead. `web` and `native` are added as before.
- Breaking: the loaded settings of the active app, `fundamental_config::App` (re-exported as
  `fundamental::App`), are now `AppSettings`: write `AppSettings::load("www")` and
  `use fundamental::AppSettings`, and nothing else changes. `App` is the name of the app
  itself, the one value its binary runs, so the settings it is served with take the name that
  says what they are. `AdminSurface::from_app` follows as `AdminSurface::from_settings`.
- Breaking: `fundamental_site` is now `fundamental_app` (`crates/app`), because the value runs
  every surface of an app, not only its pages. Write `fundamental::App` for `fundamental::Site`,
  `fundamental::app!` for `site!`, `AppPart` for `SitePart`, `AppError` for `SiteError`, the
  umbrella feature `app` for `site`, `pub fn app()` for `pub fn site()` and `www::app().run()`
  for `www::site().run()`. `Served::app()` is `Served::settings()`, which returns the
  `AppSettings`, and an `AppPart::check` takes `&AppSettings`. Its logs say `app = <name>`.
- Breaking: `fundamental::app!()` takes no name: it is `App::new(env!("CARGO_PKG_NAME"))`, so
  the app's key in `[apps.<name>]` and its telemetry's service name come from its crate and are
  never typed. Write `fundamental::app!()` for `site!("www")`. A second deployment of the same
  code runs under `FUNDAMENTAL_INSTANCE`, and a name readers see (a paid tier's, say) belongs in
  the brand or the catalog. `App::new(name)` stays for a layout whose package and key differ.
- `fundamental_web::Refusal` is now `PageRefusal`: a loader returns
  `Result<Model, PageRefusal>` (`use fundamental::web::{LoadCx, PageRefusal}`), and nothing
  else changes. Two public `Refusal` types answered two questions: the page's answer
  (404, 401, 400, error page) and an error's refusal drawn at its field
  (`fundamental_core::error::Refusal`, which keeps its name, as `Error::field_errors` and
  `MutationScope::refusal` mean that one). The page's answer takes the page's name, beside
  `PageError`, `PagePolicy` and `PagePath`, as SvelteKit's `error()` in a load and Axum's
  `Rejection` name theirs. No alias is kept: one name per thing.
- The migration generator's `Refusal` is now `UndeclaredChange`, and
  `GenerateError::Refused` carries them as `undeclared`: a schema change the models do not
  say the meaning of. No third public type is named `Refusal`.
- OTLP exports over HTTP with protobuf bodies by default (`protocol = "http/protobuf"`, also
  `"http"`, at `http://localhost:4318`), and `tonic` is no longer linked: gRPC is the
  `fundamental_observability` feature `otlp-grpc` (`fundamental`'s `observability-otlp-grpc`),
  its default endpoint port 4317. `protocol = "grpc"` in a build without it is refused when
  the process starts and by its check (`OBS_017`), and a protocol no exporter speaks
  (`OBS_018`), so a process never starts exporting nothing. Over HTTP the endpoint is the
  collector's base address and each signal goes to `/v1/traces` or `/v1/logs` under it, as
  the specification resolves `OTEL_EXPORTER_OTLP_ENDPOINT`; before, an `http` endpoint was
  posted to as written. HTTP is the specification's SDK default, and the Go, JavaScript and
  Python SDKs keep gRPC in its own package; an app exporting to a gRPC-only collector turns
  on `observability-otlp-grpc` and sets `protocol = "grpc"`. An `https://` collector is
  spoken to over rustls on aws-lc-rs with the platform's verifier; before, the HTTP exporter
  had no TLS backend and refused that scheme on every export.
- One TLS provider, aws-lc-rs, is compiled into every build: `reqwest` is 0.13 with its
  `rustls` feature (aws-lc-rs and the platform's verifier, and one `reqwest` in the tree
  where there were two), `async-nats` without `ring`, `sqlx` with `tls-rustls-aws-lc-rs`,
  `rcgen`, `rustls-webpki`, `testcontainers` and `RemoteBridge`'s client identity on
  aws-lc-rs. Before, both providers were compiled, so a `rustls::ClientConfig::builder()`
  that named neither panicked. aws-lc-rs over ring because it is rustls' own default, keeps
  the post-quantum `X25519MLKEM768` key exchange ring's provider lacks, and is already the
  one `jsonwebtoken` needs for constant-time RSA (RUSTSEC-2023-0071); `ring` remains only as
  a primitives library. NATS and YCQL still install aws-lc-rs as the process default, so an
  app whose own dependencies compile `ring` as well keeps connecting.
- The OS keychain is the `fundamental_keys` feature `os-keychain` (`fundamental`'s
  `keys-os-keychain`, which `native` turns on): `KeyringSeedStore`, `KeyringSecretStore`
  and, with `keys-hd`, `KeychainSealer`. `StoredSigner`, `MemorySeedStore`, `SecretStore`
  and `open_device_key` need no keychain. Before, every build with `auth` linked `keyring`,
  and on Linux through it `libdbus`, a C library and a session bus no server uses. A
  desktop app built on `native` changes nothing; one that names `keys` alone for the
  keychain adds `keys-os-keychain`.

- `ServerAction<S>`: an action's server half is generic over the state of the router it is
  mounted on, and `type State` is gone. `#[action]` over `HttpContext<TeamApi>` implements
  `ServerAction<S>` for every `S` that `TeamApi` is read out of (`FromRef`), its route
  projected into `S`; `Router::action::<M>()` takes `M: ServerAction<S>` and mounts
  `M::route()` as built, and `mount_actions!` bounds each marker by `ServerAction<S>`. A
  state parameter rather than an associated type, because an associated type cannot be
  generic, and an action over a generic state is served over many. Mounts written with
  `#[action]` and `mount_actions!` are unchanged. A hand-written impl names its state:
  `impl ServerAction<AppState> for X { fn route() -> Route<AppState> { … } }` in place of
  `impl ServerAction for X { type State = AppState; fn route() -> Route<AppState> { … } }`,
  and `<X as ServerAction>::route().project::<S>()` becomes `<X as ServerAction<S>>::route()`.
- The migration guard reads an `ALTER TABLE` action of `DROP COLUMN`, `DROP CONSTRAINT` or
  `RENAME COLUMN` as destructive (`DdlStatement::Destructive`, object `<table>.<name>`): each
  removes data, or a name running code reads, which before passed unjudged as `Other`.
  `ddl::recognise_all` reads every action of a multi-action `ALTER TABLE`, so
  `ADD COLUMN a …, DROP COLUMN b` judges both; `recognise` answers the first. `GUARD_005`
  says how to declare one.
- `MigrateRunner::cql_model`, `offline_model` and `syncable_cql_model` register the model's
  `<table>_i18n` companion, as `sql_model` does, and the YCQL and SQLite introspectors read
  companions like any table rather than hiding them: `check_cql` and `check_offline` compare a
  localized model's companion with its definition, and report it missing until a migration
  creates it. Write that migration from `translation_table_sql()`.
- A YCQL model's `<table>_i18n` companion is keyed by the record's own key columns instead of
  a text `entity_id` joining them: it shares the record's partition key and clusters by its
  clustering columns, then the field and the language, so a record's translations live in its
  partition. `KeyedRepository::delete`, `delete_many`, `CqlBatch::delete_batch` and `purge` now
  remove them too (a key deleted and written again no longer picks up the old translations),
  and `insert_many`, `update_many` and `CqlBatch::insert_batch` write them rather than dropping
  them; soft delete keeps them. A batch writes its rows first and each record's translations
  after, cleared then written: YCQL refuses a batched delete of more than one row, and a clear
  in the rows' batch would share their write time and win. `insert_if_not_exists` and
  `update_if_exists` write the translations when they apply. `RepositoryTranslation<_, CqlDb>`
  returns the records' keys typed, `ToRow::KeyValues` (the repository's `Id`), not rendered
  strings: compare with `[(organisation, id)]` where you compared with `["org\u{1}id"]`.
  `translation_table_sql()` is written from `ModelSchema<CqlDb>::translation_schema()`; create
  the companion again (`DROP TABLE <table>_i18n` and run it) where one exists.
- An offline model's `<table>_i18n` companion is keyed by the record's own key column, with
  its SQLite type, instead of a text `entity_id`: `find_ids_by_translation` returns the key as
  the record holds it (`Value::Integer(7)`, not `Value::Text("7")`), a delete by key or by
  filter compares like with like, and a page of records reads only its own translations,
  rather than the whole companion. The companion's `CREATE TABLE` is written from
  `ModelSchema<Offline>::translation_schema()`. A cache built before is dropped and rebuilt
  once (its schema version changes); nothing in it was the only copy.
- `TableSchema::translations` takes the engine (`MigrationDatabase`) and builds the companion
  for YCQL and SQLite as well as YSQL, keyed by the record's own key columns: on YCQL it shares
  the record's partition key and clusters by its clustering columns, then the field and the
  language, so a record's translations live in its partition; on SQLite it carries the record's
  key column with its own type. Pass `MigrationDatabase::SqlDb` where `translations()` was called.
- `fundamental_i18n_keys::fluent_argument` is renamed `argument_name`, with the same behaviour
  (`{0}` is the argument `arg0`).
- `CatalogBuilder::build()` returns `Catalog`, not `Result<Catalog, CatalogError>`: compiled MF2
  catalogs cannot fail to parse at startup, so drop the `?` or `.expect(..)`.
- `FluentTranslator` is renamed `CatalogTranslator` (one name per thing: it reads a `Catalog`).
  Its `lookup`, `own_words` and `carries` read only the app's own and the crates' MF2 catalogs.
- `ErrorCodeIndex` and `ErrorCodeSite` moved from `fundamental_i18n_tooling` to
  `fundamental_messages_build`, the crate that already reads a crate's error codes for the
  messages check: import `fundamental_messages_build::ErrorCodeIndex`. Same API and behaviour.
- `fun dev` builds a web app's islands at the same time as its server: `DevLoop::alongside(step)`
  runs a step beside every cargo build (a rebuild succeeds only when both do; either failing
  restarts nothing), and the bundle's styles and assets stay a `prebuild` step, so a stylesheet
  lands in `dist/` before the wasm and the server finish compiling. `fun dev --help` says how a
  change travels: styles first, islands beside the server, the open page live-reloaded.
- `DevLoop::style_only(test)` names the changed paths the `prebuild` step alone answers. A change
  made only of them runs the prebuild step and skips cargo and the `alongside` step, since no binary
  can change; any other path in the same change runs the whole build. Changes that arrive while the
  loop waits for quiet now count toward the rebuild too.
- `fun dev` rebuilds only the styles when every changed file is a `.css` file in a `styles` tree
  (`styles/`, `apps/<app>/styles/`, `domain/<name>/styles/`): no cargo build, no islands build,
  no restart, and the open page swaps its stylesheet. A `.css` file elsewhere may be compiled into
  a binary, so it still runs the whole build.
- `WasmBuild` passes its target directory to cargo as `--target-dir`, and `fun build` and
  `fun dev` give it `<target>/wasm`: the browser bundle compiles under its own cargo lock, so it
  never waits on the server build, and both commands share one wasm cache.
- `fun new` with a web surface and no other server surface writes the binary as one line,
  `fn main() -> ExitCode { app::site().run() }`, and declares `pub fn site()` in `lib.rs` (its
  pages, its actions for the dev tools, no state), so the tests answer the router the binary
  serves; the app's `ssr` feature asks for `fundamental/site`. Such an app writes no
  `pages/document.rs`: its pages wear the site's shell, `.document(Document::new().title(app))`
  (with Tailwind, `.head(style_links)` linking `app.css` by version), so the icons, theme and share
  card it adds there reach every page. A web app beside a worker, socket,
  api or sync surface keeps its hand-composed `Runtime` until those surfaces are site parts.
- `ActionHook` takes a fourth type parameter, the path it calls its action at (`DeclaredPath` by
  default), so a type that named `ActionHook<M, O, S>` keeps compiling.
- A server-rendered query hook sends its input where the action's route reads it: a `GET`
  action's `.source(..)` input as the query string, through `ActionDispatcher::dispatch_at`. It
  was sent as a JSON body the route never read, so a `GET` filter failed on the server.
- A browser action call sends a `GET` or `DELETE` input as the query string, as the route reads
  it: a `GET` filter was dropped after hydration, and a `DELETE` input was sent as a JSON body
  the route never read. The `fetch` and the Tauri `invoke` transports shape their call with
  `ActionRequest`; `fundamental_invoke` gives the path's query to the bridged request.
- An `#[action]` marker's `path(..)` answers `fundamental_core::action::ActionPath<Marker>`, the
  filled path typed by its action, instead of `String`. It derefs to `str` and displays as the
  path, so `FormAction::parse(&X::path(h))` and links keep compiling; call `.into_string()` where
  a `String` is needed.
- `ActionTransport::call` takes the path as `Cow<'static, str>`, so a transport sends a filled
  path as well as a declared one. A transport of your own takes `path: Cow<'static, str>` and
  reads it with `&path`.
- `render_page_stream` and `render_page_stream_with` return `Result<StreamedResponse, AnyError>`:
  the error is the page's answer when a blocking query failed. Write
  `render_page_stream(&ctx, page, document).await?` where the result was used as the response;
  the generated page routes do.
- `<Mutation>` takes only a write that answers `Outcome<T>`: a mutation `#[action]` returns
  `Result<Outcome<T>, _>` and `MutationScope::value` gives `Option<Result<Outcome<T>, AnyError>>`.
  Return `Outcome::changed(value)` (or `Outcome::nothing()`) where the action returned `value`.
- The `mutation_success` default is drawn from the write's `Outcome<()>` (its words and change
  id), through the new `AsyncSuccessView`: `AsyncDefaultsConfig::mutation_success` and
  `<AsyncDefaults mutation_success>` take a closure `|outcome: Outcome<()>| view! { … }` where
  they took a view with no argument. The built-in default says the outcome's words in a
  `role="status"` line.

### Removed

- `fundamental_db::migration::DestructivePolicy`, replaced by the `destructive` lint's level.

- `fundamental_core::model::ColumnDef`: nothing built or read it. A table's columns are
  described by `table_schema()`, and its indexes, unique ones too, by `SqlModel::INDEXES`.
- The Fluent-era source tables: `I18N_ENTRIES` on every `#[derive(Error)]` and
  `#[derive(Model)]` type, `ReturnPathError::I18N_ENTRIES`, `validate::VALIDATION_I18N_ENTRIES`
  and the `i18n::SourceEntry` they held. Nothing read them; the words are each crate's MF2
  messages. `entity_label`, `entity_label_plural` and `field_label` stay.
- `CqlModel::translation_entity_id`: a YCQL record's translations are addressed by its key,
  `ToRow::to_key_values()`.
- `TranslationSettings`' Fluent-only fields `catalog_dir`, `loading` (and `LoadingStrategy`),
  `keys_out` and `keys_module`: only the Fluent tooling read them. `[i18n]` keeps `enabled`,
  `source_locale`, `default_locale`, `supported_locales` and `fallback_chain`, which say which
  languages an app serves; the `[messages]` table holds what a missing translation does. The
  fields go rather than the whole table because the languages served are still read, and an
  old key in `[i18n]` is ignored, not refused.
- `fundamental_i18n_keys::fluent_identifier`, `fluent_term` and `with_terms_unresolved`, and
  `scan_template`'s reading of `{ -term }`: nothing maps a dotted id to a Fluent identifier any
  more, and MF2 has no terms (say the brand with `{name :brand}`).
- The `t!` macro, `#[derive(Localize)]` (its `localized_name()`, `#[localize]`,
  `#[localize_args]`), `fundamental_core::i18n::TranslationKey` and the hidden
  `fundamental_core::i18n::kind` argument dispatch only `t!` called. Say a word with the typed
  function a crate's messages build generates (`m::panel::save()`), and a closed value's words
  with `#[derive(Messages)]` on it; ask by id only where the id is data, with
  `fundamental_core::i18n::translate(key, args, fallback)`. An error's template no longer reads
  a Fluent term (`{ -brand-name }`): say the brand in its MF2 message with `{name :brand}`.
- `fundamental_i18n` holds no Fluent catalogs: `CatalogBuilder::with_locale` and
  `with_defaults`, `Catalog::from_sources`, `load_dir`, `LoaderError`, `CatalogError`, the
  re-export of `fluent_identifier`, and the `fluent` dependency are gone. An app words its own
  text, and any crate's message, in its own MF2 messages: `.with_own_messages(&crate::m::CATALOGS)`
  in place of `with_locale`; a library registers its words with `.with_messages(&lib::m::CATALOGS)`
  in place of `with_defaults`, and the app's own messages rank above it as before.
- The `fundamental_i18n_tooling` crate and the umbrella's `fundamental::i18n::tooling` module:
  the Fluent catalog sync and check (`sync_catalogs`, `check_catalogs`, the `.ftl` writer and
  the typed `keys` module). A crate's words are MF2 messages now: `fundamental_messages_build`
  builds and checks them, and `fun messages check [--ci]` is the gate. `ErrorCodeIndex` lives
  on in `fundamental_messages_build`.

### Fixed

- Two processes fetching the pinned Tailwind at once on a new machine no longer fail: each
  downloads into its own file before the verified binary is moved into place.
- A YCQL page read with its translations is `Send` again: the companion query's bound values
  are `Sync`, so `fundamental_authz`'s YCQL tuple store, whose reads are awaited on a
  multi-threaded runtime, compiles with the workspace's features.
- `fundamental_test_containers`' `sql` fixtures are fenced by `debug_assertions`, as `cql`'s
  are: they are built on `fundamental_sql::test_support`, which exists only with them, so a
  `cargo bench` whose dev-dependencies take the crate with `sql` failed to compile.
- `cargo bench -p fundamental_cql --features internal-benches` compiles again:
  `fundamental_test_containers`' `cql` module, built on `fundamental_cql::test_support`, is
  fenced by `debug_assertions` as that module is, so a bench build's dev-dependencies no
  longer reach for it.
- The migration ledger's resolutions table is made by the first resolution (or, on YCQL, the
  first status read) rather than by every first apply, so a fresh schema or keyspace — every
  test fixture's — pays for one catalog write fewer.
- `SqlMigrator`'s ledger reads are awaited behind a `dyn Future`, so `check_sql` stays within
  the compiler's default query depth (128) in an app's migrate binary that sets no
  `recursion_limit`; the step ledger had made it 130 deep under some feature graphs.
- `fundamental_keys` builds for `wasm32` without `keys-hd`: `WebCryptoAead` and
  `AEAD_NONCE_LEN` are exported only with `keys-hd`, whose `KeyError::Seal` and `Unseal` they
  answer and without which they never compiled. A browser crate that seals bytes turns on
  `keys-hd`.
- `keys-hd` without `os-keychain` or `keys-hd-password` builds with no unused code: the
  native seed-at-rest helpers are compiled only with a sealer that uses them.
- `SqlModel::INDEXES` lists a unique index as unique and a partial one with its condition,
  as the table is made: `#[derive(Model)]` wrote every index as a plain one.
- Not changed, and now held by a conformance case: `SqlModel::into_stored` leaves a
  `Localized` field whole, because a translation's `Checked` is whole milliseconds, which
  the companion table writes (`to_timestamp(ms / 1000.0)`) and reads back
  (`round(extract(epoch …) * 1000)`) exactly; only moments are cut to the microsecond.
