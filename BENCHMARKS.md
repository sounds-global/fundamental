# Benchmarks

What the public features cost, and how they hold up under load and failure. Five layers, each
one `fun bench` verb, each a bench target of the same name in `crates/bench`
(`fundamental_bench`). None of them runs in the development loop; the first runs on every pull
request.

| Layer | Command | What it proves | Where it runs |
|---|---|---|---|
| Instructions | `fun bench` | a hot path does no more work than the committed baseline allows | every pull request |
| Wall | `fun bench wall` | what each hot path costs in time on one machine | by hand, `bench` workflow |
| Load | `fun bench load` | throughput and latency of a page, an action, the worker and sync at rising concurrency | by hand, `bench` workflow |
| Soak | `fun bench soak --duration 10m` | memory stays flat and nothing fails over time | by hand, `bench` workflow |
| Fault | `fun bench fault` | no acknowledged write is lost or made twice through a database restart, a NATS outage and a partition | by hand, `bench` workflow |

Every number below was measured once, with the command beside it, at commit `538f7d8`.

## The machine

| | |
|---|---|
| CPU | Apple M1 Max: 8 performance (Firestorm) and 2 efficiency (Icestorm) cores, 10 in all |
| Memory | 62 GiB |
| OS | Fedora Linux Asahi Remix 44, kernel `7.1.13-402.asahi.fc44.aarch64+16k` (16 KiB pages) |
| Toolchain | `rustc 1.96.0 (ac68faa20 2026-05-25)` |
| Docker | 29.8.2 |
| Engines | `yugabytedb/yugabyte:2026.1.1.0-b91` (one node), `nats:2.10.29` with JetStream |
| Instruction counts | `fundamental-bench:1.96.0-gungraun-0.20.0`: `rust:1.96.0-trixie`, valgrind 3.24.0, `gungraun-runner` 0.20.0 |

The app, the load generator and both engines share the machine: the load numbers are what one
laptop sustains with everything on it, not a server's ceiling.

## The workloads

One module per hot path in `crates/bench/src/workloads`, each a setup (what an app does once)
and a call (what it does per request). `benches/instructions.rs` counts the call and
`benches/wall.rs` times it, so both measure identical code.

| Bench | The call |
|---|---|
| `render/page` | a page of plain Leptos parts (a header, a navigation, 50 rows with a link and a badge, a form) rendered to HTML under the islands' shared context a served page has |
| `action/dispatch` | an `#[action]` mounted by `mount_actions!`, dispatched through the router with a JSON body: the route matched, the body read, the handler run, the answer serialised |
| `query/select_sql`, `update_sql`, `select_cql` | a YSQL `SELECT` with ten filters, a bulk `UPDATE` of ten columns, a YCQL `SELECT` on ten clustering columns |
| `migration/diff`, `generate`, `differences` | a 50-table schema against a database one column and one index behind on every table: the differ, the next migration's statements, and the snapshot comparison |
| `sync/encode_pull`, `decode_pull` | a pulled page of 100 rows to and from JSON |
| `sync/push_round_trip`, `canonical`, `envelope_round_trip` | one pushed row to JSON and back, the bytes a device signs for it, and the WebSocket envelope it travels in |
| `config/load`, `config/manifest` | `fun.toml` with `fun.production.toml` over it, through the config loader and through the manifest |
| `mf2/format` | twelve MessageFormat 2 messages (plain, a number, two plurals, a selection, a variable) in English and Dutch |

## Instructions

**Method.** [gungraun](https://github.com/gungraun/gungraun) (formerly iai-callgrind) runs each
bench once under valgrind's callgrind and reports the instructions executed (`Ir`). The setup
and one warm call run before counting starts, so a first-use cache is not counted; the fixture
is handed back and dropped after counting stops. Address-space randomisation is off.

A count repeats to within a fraction of a percent run to run (the migration benches move by up
to 0.2%: their maps are seeded per process), and to the instruction for the rest. It is stable
across machines **of one architecture and toolchain only**: an `aarch64` build executes
different instructions from an `x86_64` one, and libc's own functions are counted too. So the
baseline is one file per architecture, `benches/baseline/<arch>.json`, and a check compares
only against its own architecture's file and refuses when there is none.

`fun bench` counts in a pinned container whenever docker or podman is on PATH, even on a
machine that has valgrind: libc's instructions are counted too, so only counts taken on the
baseline's libc and valgrind compare with it. It builds the image from
`cli/src/commands/bench/Containerfile` (the project's toolchain, valgrind, and the
`gungraun-runner` the lockfile names) and counts there, with the workspace mounted at its own
path and built into `target/bench-container`. The pull-request check runs the same container
on an arm64 runner, so its toolchain, libc and valgrind are the baseline's. Only on a machine
with no container engine does it use the host's valgrind, and it says that those counts may
differ from the baseline's.

**The check.** Each bench may grow by 2% (a bench's own `"threshold"`, or the file's, overrides
it). Growth beyond that, a bench no longer measured, and a bench measured but not in the
baseline each fail the check, all named at once. A count more than 2% below its baseline passes
and is reported, so the gain can be kept with `fun bench --save`.

**Reproduce.** `fun bench` (check), `fun bench --save` (rewrite the baseline). Natively, with
valgrind and the runner installed: `cargo bench -p fundamental_bench --bench instructions --
--save-summary=json`.

| Bench | Instructions |
|---|---:|
| `render/page` | 188,098 |
| `action/dispatch` | 16,087 |
| `query/select_sql` | 28,145 |
| `query/update_sql` | 24,691 |
| `query/select_cql` | 31,066 |
| `migration/diff` | 1,867,628 |
| `migration/generate` | 1,286,351 |
| `migration/differences` | 1,102,042 |
| `sync/encode_pull` | 386,782 |
| `sync/decode_pull` | 1,049,920 |
| `sync/push_round_trip` | 18,158 |
| `sync/canonical` | 2,814 |
| `sync/envelope_round_trip` | 21,993 |
| `config/load` | 534,012 |
| `config/manifest` | 227,465 |
| `mf2/format` | 65,337 |

These are `benches/baseline/aarch64.json`. `x86_64.json` is not committed yet: the `bench`
workflow's `save-baseline` run counts on an x86_64 runner and uploads it, to be committed from
there.

## Wall

**Method.** criterion 0.8, default settings (3 s warm-up, 5 s measurement, 100 samples), the
`bench` profile (`release`). The mean of each bench, with its 95% confidence interval within
±0.3% for every one.

**Reproduce.** `fun bench wall` (or `cargo bench -p fundamental_bench --bench wall`).

| Bench | Mean |
|---|---:|
| `render/page` | 13.10 µs |
| `action/dispatch` | 1.49 µs |
| `query/select_sql` | 2.29 µs |
| `query/update_sql` | 1.75 µs |
| `query/select_cql` | 2.01 µs |
| `migration/diff` | 157.10 µs |
| `migration/generate` | 102.77 µs |
| `migration/differences` | 93.04 µs |
| `sync/encode_pull` | 17.83 µs |
| `sync/decode_pull` | 74.12 µs |
| `sync/push_round_trip` | 1.11 µs |
| `sync/canonical` | 176.63 ns |
| `sync/envelope_round_trip` | 1.50 µs |
| `config/load` | 59.44 µs |
| `config/manifest` | 24.34 µs |
| `mf2/format` | 5.24 µs |

## The live app

Load, soak and fault drive `fundamental_bench::live::BenchApp`, an app served in the bench
process over real engines from `fundamental_test_containers`:

- **a page** at `/`: a `PageRoute` rendering the `render/page` view, served over HTTP by
  `ApiServer`;
- **an action** `POST /api/notes`: in one `READ COMMITTED` transaction it inserts the note if
  absent and schedules its reminder job, then publishes an event to a JetStream stream under the
  note's message id. Every step is idempotent, so a call retried after a lost answer writes
  nothing twice;
- **a worker**: a `fundamental_jobs` drain that fires due jobs, each fire writing its mark in
  the fire's own transaction;
- **a sync slice**: `slice_sync_router` over `SqlSliceStore` on WebSockets, each of eight
  readers pulling its own 100 notes, read through an index on the owner and `updated_at`.

The pool holds at most 64 connections (16 in the fault runs). The live programs build with the
`bench-live` profile: `release` with debug assertions on, because the test containers' fixtures
and an engine's plain-text development connection exist only with them (a release build
requires TLS, which a test engine does not serve). The numbers include the cost of every
`debug_assert!`.

## Load

**Method.** A closed loop, as oha and goose drive one: at each concurrency, that many workers
call back to back for 20 s after a 1 s warm-up that is not measured, so the concurrency is exact
and a slow server lowers throughput rather than queueing calls nobody waits for. Every call's
latency goes into an HDR histogram (1 µs to an hour, three significant figures). A failed call
is counted and its latency kept; the run fails if any call failed.

- **pages**: `GET /` over HTTP, the body read whole and checked.
- **worker**: one replica's tick: schedule a job due now in its own transaction, then claim and
  fire one due job.
- **sync**: one `slice.pull` of a reader's 100 rows over an authenticated WebSocket, on 32
  connected clients in turn.
- **actions**: `POST /api/notes`, last, so the jobs it schedules are not the worker's backlog.

**Reproduce.** `fun bench load` (or `cargo bench -p fundamental_bench --profile bench-live
--features live --bench load -- --each 20s --concurrency 1,8,32,128`). Each run's JSON is
`target/bench/load/<run>.json`.

### pages

| concurrency | requests | errors | per second | p50 ms | p95 ms | p99 ms | max ms |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 88502 | 0 | 4425 | 0.21 | 0.37 | 0.67 | 6.33 |
| 8 | 724130 | 0 | 36205 | 0.19 | 0.40 | 0.58 | 3.94 |
| 32 | 965784 | 0 | 48286 | 0.64 | 0.97 | 1.25 | 7.21 |
| 128 | 985255 | 0 | 49253 | 2.50 | 4.34 | 5.29 | 14.37 |

### worker

| concurrency | requests | errors | per second | p50 ms | p95 ms | p99 ms | max ms |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 367 | 0 | 18 | 47.30 | 123.65 | 139.39 | 156.16 |
| 8 | 2757 | 0 | 138 | 56.67 | 71.30 | 93.25 | 161.02 |
| 32 | 2496 | 0 | 124 | 254.85 | 296.96 | 316.67 | 352.00 |
| 128 | 2304 | 0 | 112 | 1133.57 | 1203.20 | 1231.87 | 1356.80 |

### sync

| concurrency | requests | errors | per second | p50 ms | p95 ms | p99 ms | max ms |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 6893 | 0 | 345 | 2.59 | 4.29 | 6.88 | 56.58 |
| 8 | 131140 | 0 | 6557 | 1.14 | 1.82 | 2.30 | 8.15 |
| 32 | 172682 | 0 | 8616 | 3.81 | 5.27 | 6.13 | 42.21 |
| 128 | 169153 | 0 | 8434 | 12.37 | 34.75 | 52.51 | 93.50 |

### actions

| concurrency | requests | errors | per second | p50 ms | p95 ms | p99 ms | max ms |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 4303 | 0 | 215 | 4.57 | 8.89 | 13.20 | 27.14 |
| 8 | 50989 | 0 | 2549 | 3.02 | 4.09 | 4.99 | 39.07 |
| 32 | 69257 | 0 | 3462 | 9.00 | 12.40 | 14.61 | 64.35 |
| 128 | 71073 | 0 | 3548 | 35.01 | 44.96 | 56.09 | 117.89 |

**What the numbers say.**

- Pages, sync and actions scale until the machine is saturated (pages near 32 calls in
  flight, actions and sync between 8 and 32); beyond it throughput holds and latency grows
  with the queue, without errors.
- The worker is the bottleneck of this app: a tick that schedules and fires one job takes
  47 ms at one replica, and more replicas than 8 lower throughput. The run logged 14,477
  serialisation conflicts (`40001`, `SQL_003`), each retried by the pool and none surfacing
  as an error: concurrent ticks contend for the same job rows. An app that fires jobs at more
  than about 140 a second per process on one node should batch its drain (`drain(n)` with a
  larger `n`) rather than add replicas.

## Soak

**Method.** The load mix (a page, a write, a worker tick, a pull, in turn) at 32 calls in flight
for ten minutes, the process's resident memory sampled every ten seconds from `VmRSS` in
`/proc/self/status`. The verdict compares the average of the last third of the samples with the
first third's, so the warm-up (pools filling, caches warming) is judged against the steady
state rather than an empty process: it fails above 10% growth, and on any failed call. The
process is the whole app and its clients, so its memory is the app's.

**Reproduce.** `fun bench soak --duration 10m` (or `cargo bench -p fundamental_bench --profile
bench-live --features live --bench soak -- --duration 10m`). The samples are in
`target/bench/soak/mix.json`.

| duration | concurrency | calls | failed | per second | p50 ms | p95 ms | p99 ms | resident at start | at 1 min | at end | growth (last third over first) |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 600 s | 32 | 189,014 | 0 | 315 | 17.41 | 454.14 | 518.91 | 32 MiB | 46 MiB | 47 MiB | 2.6% |

Memory rises in the first minute, as the pool and the servers fill, and is flat after it: 46 MiB
at one minute, 47 MiB at ten. The tail latency is the worker's ticks in the mix.

## Fault

**Method.** Jepsen's check, at the scale of one machine. 300 numbered writes, 10 ms apart, each
through the action (a row, a job, an event), each refused write retried every 100 ms with the
same number for up to two minutes. One second in, a fault is injected; once it is over, every
write is read back from the table and from the stream, and every due job is fired. The run fails
on an acknowledged write that is not both a row and an event, a write found twice in either, a
write the writer never got through, or a job that never fired.

The engines are a node and a server of the run's own (`DedicatedEngineNode`,
`DedicatedNatsServer`), so restarting them disturbs no test suite, each reached through a
`TcpProxy`: a partition cuts it, and after a restart it is retargeted, since Docker publishes a
restarted container on new ports. The NATS client reconnects as a deployed one does.

- **database restart**: `docker restart` of the YugabyteDB node, waiting until it answers again.
- **nats outage**: the NATS server stopped for 5 s, then started again.
- **network partition**: both proxies cut for 5 s, every connection dropped and every new one
  refused, then restored.

**Reproduce.** `fun bench fault` (or `cargo bench -p fundamental_bench --profile bench-live
--features live --bench fault -- --writes 300`). Each fault's JSON is
`target/bench/fault/<fault>.json`.

| fault | writes | acknowledged | refused attempts | outage as the writer saw it | lost | duplicated | jobs fired |
|---|---:|---:|---:|---:|---:|---:|---:|
| database restart | 300 | 300 | 46 | 4.8 s | 0 | 0 | 300 |
| nats outage | 300 | 300 | 1 | 4.5 s | 0 | 0 | 300 |
| network partition | 300 | 300 | 49 | 6.2 s | 0 | 0 | 300 |

During the NATS outage the action mostly waited rather than failed: the client held the
publish while it reconnected, so the writes in the outage took its length, and one attempt was
refused and retried.

**Limits.** One YugabyteDB node, restarted whole: a replicated cluster (RF 3) losing one node of
three, the failure a deployment is built for, is too heavy for a minutes-long run on one
machine and is not measured here. The partition cuts the app from both engines at once, at the
TCP level; it does not split a cluster.

## Adding a benchmark

- **A hot path**: a workload in `crates/bench/src/workloads` (its setup and its call), a
  `#[library_benchmark]` in `benches/instructions.rs` and a criterion bench in `benches/wall.rs`,
  then `fun bench --save` and a commit that says why the baseline grew.
- **An app's own**: a package with bench targets named `instructions`, `wall`, `load`, `soak` or
  `fault` (`harness = false`) is run by the same `fun bench` verbs. `Load`, `Soak`, `Fault`,
  `Report` and `Baseline` from `fundamental_bench` are the parts; the live layers need a
  `[profile.bench-live]` with `inherits = "release"` and `debug-assertions = true`.
