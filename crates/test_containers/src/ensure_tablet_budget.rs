use bollard::Docker;
use bollard::container::LogOutput;
use bollard::exec::StartExecResults;
use bollard::models::ExecConfig;
use futures::StreamExt;
use tracing::{info, warn};

use crate::container_name::ENGINE;

/// How many full suites a machine runs against the shared engine at once.
const CONCURRENT_SUITES: u64 = 3;

/// The most tablets one full framework suite holds at once, above what is resident.
const PEAK_PER_SUITE: u64 = 574;

/// The tablets that stay on the engine with no suite running, the pooled schemas included.
const RESIDENT: u64 = 2297;

/// The tablets the engine must admit for the suites a machine runs at once.
const TABLET_BUDGET: u64 = RESIDENT + CONCURRENT_SUITES * PEAK_PER_SUITE;

/// What one tablet replica costs the tserver, in bytes.
const BYTES_PER_TABLET: u64 = 390 * 1024;

/// The margin on [`BYTES_PER_TABLET`] before a ceiling is called affordable: one and a half.
const MARGIN_NUMERATOR: u64 = 3;
const MARGIN_DENOMINATOR: u64 = 2;

const GIB: u64 = 1024 * 1024 * 1024;

/// Prints the tserver's root memory limit, its tablet-overhead share and the master's per-GiB
/// tablet limit, one per line, read inside the container.
const READ_SCRIPT: &str = r#"H=$(hostname -i)
curl -sf "http://$H:9000/api/v1/mem-trackers" | python3 -c 'import json,sys; print(json.load(sys.stdin)["limit_bytes"])'
curl -sf "http://$H:9000/varz?raw" | sed -n 's/^--tablet_overhead_size_percentage=//p'
curl -sf "http://$H:7000/varz?raw" | sed -n 's/^--tablet_replicas_per_gib_limit=//p'"#;

/// What the engine reports about its own tablet ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EngineMemory {
    /// The tserver's root memory limit, in bytes.
    root_limit_bytes: u64,
    /// The share of it the engine sizes its tablet ceiling from, in percent.
    overhead_percent: u64,
    /// The master's current `tablet_replicas_per_gib_limit`.
    per_gib: u64,
}

/// What [`ensure_tablet_budget`] decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Raise {
    /// The ceiling the engine has now, in tablets.
    ceiling: u64,
    /// The per-GiB limit to set, or `None` when the ceiling already admits the budget or is as
    /// high as memory allows.
    per_gib: Option<u64>,
    /// The ceiling once `per_gib` is set.
    raised_ceiling: u64,
    /// How many suites the raised ceiling admits beside what is resident.
    suites_admitted: u64,
}

impl EngineMemory {
    /// The tablet-overhead budget the ceiling is sized from, in bytes.
    fn overhead_bytes(self) -> u64 {
        self.root_limit_bytes / 100 * self.overhead_percent
    }

    /// The engine's ceiling at `per_gib`, as the master computes it.
    fn ceiling_at(self, per_gib: u64) -> u64 {
        // `u128`, because the product overflows `u64`.
        let ceiling = u128::from(per_gib) * u128::from(self.overhead_bytes()) / u128::from(GIB);
        u64::try_from(ceiling).unwrap_or(u64::MAX)
    }

    /// The most tablets the tserver's memory holds at the measured cost with the margin.
    fn affordable(self) -> u64 {
        self.overhead_bytes() * MARGIN_DENOMINATOR / (BYTES_PER_TABLET * MARGIN_NUMERATOR)
    }

    /// Decides whether, and how far, to raise the per-GiB limit so the ceiling admits `budget`.
    ///
    /// Only ever raises, since another checkout may have raised it further, and never past what
    /// memory holds.
    fn raise_for(self, budget: u64) -> Raise {
        let ceiling = self.ceiling_at(self.per_gib);
        let target = budget.min(self.affordable());
        let overhead = self.overhead_bytes().max(1);
        let wanted = (u128::from(target) * u128::from(GIB)).div_ceil(u128::from(overhead));
        let wanted = u64::try_from(wanted).unwrap_or(u64::MAX);
        let per_gib = (wanted > self.per_gib).then_some(wanted);
        let raised_ceiling = per_gib.map_or(ceiling, |per_gib| self.ceiling_at(per_gib));
        let suites_admitted = raised_ceiling.saturating_sub(RESIDENT) / PEAK_PER_SUITE;
        Raise {
            ceiling,
            per_gib,
            raised_ceiling,
            suites_admitted,
        }
    }

    /// Reads the three numbers [`READ_SCRIPT`] prints.
    fn parse(output: &str) -> Option<Self> {
        let mut lines = output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty());
        Some(Self {
            root_limit_bytes: lines.next()?.parse().ok()?,
            overhead_percent: lines.next()?.parse().ok()?,
            per_gib: lines.next()?.parse().ok()?,
        })
    }
}

/// Makes the shared engine admit the tablets this machine's concurrent suites need, once per run.
///
/// Set as a master runtime flag rather than in the recipe, so no fingerprint changes. Only ever
/// raises, capped by memory with a warning, and never fails the caller.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) async fn ensure_tablet_budget() {
    let docker = match Docker::connect_with_defaults() {
        Ok(docker) => docker,
        Err(error) => {
            warn!(%error, "test containers: no Docker client to size the engine's tablet ceiling");
            return;
        }
    };
    let Some(memory) = read_memory(&docker).await else {
        return;
    };
    let raise = memory.raise_for(TABLET_BUDGET);
    let Some(per_gib) = raise.per_gib else {
        info!(
            ceiling = raise.ceiling,
            budget = TABLET_BUDGET,
            per_gib = memory.per_gib,
            suites_admitted = raise.suites_admitted,
            "test containers: the engine's tablet ceiling already admits this machine's suites"
        );
        return;
    };
    let set = format!(
        "bin/yb-ts-cli --server_address=$(hostname -i):7100 set_flag tablet_replicas_per_gib_limit {per_gib}"
    );
    if let Err(error) = exec(&docker, &set).await {
        warn!(%error, per_gib, "test containers: could not raise the engine's tablet ceiling");
        return;
    }
    match read_memory(&docker).await {
        Some(after) if after.per_gib >= per_gib => {
            if raise.raised_ceiling < TABLET_BUDGET {
                warn!(
                    ceiling = raise.raised_ceiling,
                    budget = TABLET_BUDGET,
                    suites_admitted = raise.suites_admitted,
                    suites = CONCURRENT_SUITES,
                    "test containers: this machine's memory holds fewer tablets than its suites \
                     need; the ceiling is raised as far as memory allows"
                );
            } else {
                info!(
                    from = raise.ceiling,
                    to = raise.raised_ceiling,
                    per_gib,
                    budget = TABLET_BUDGET,
                    "test containers: raised the engine's tablet ceiling for this machine's suites"
                );
            }
        }
        Some(after) => warn!(
            wanted = per_gib,
            found = after.per_gib,
            "test containers: the engine's tablet ceiling did not take the raise"
        ),
        None => {}
    }
}

/// Reads what the engine reports about its memory and ceiling, or warns and answers `None`.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn read_memory(docker: &Docker) -> Option<EngineMemory> {
    match exec(docker, READ_SCRIPT).await {
        Ok(output) => {
            let memory = EngineMemory::parse(&output);
            if memory.is_none() {
                warn!(%output, "test containers: the engine's memory report did not parse");
            }
            memory
        }
        Err(error) => {
            warn!(%error, "test containers: could not read the engine's memory report");
            None
        }
    }
}

/// Runs `script` under `bash -c` in the engine container and returns its standard output.
async fn exec(docker: &Docker, script: &str) -> Result<String, String> {
    let config = ExecConfig {
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        cmd: Some(vec!["bash".to_owned(), "-c".to_owned(), script.to_owned()]),
        ..ExecConfig::default()
    };
    let created = docker
        .create_exec(ENGINE, config)
        .await
        .map_err(|error| error.to_string())?;
    let started = docker
        .start_exec(&created.id, None)
        .await
        .map_err(|error| error.to_string())?;
    let StartExecResults::Attached { mut output, .. } = started else {
        return Err("the exec detached".to_owned());
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    while let Some(chunk) = output.next().await {
        match chunk {
            Ok(LogOutput::StdOut { message }) => {
                stdout.push_str(&String::from_utf8_lossy(&message));
            }
            Ok(LogOutput::StdErr { message }) => {
                stderr.push_str(&String::from_utf8_lossy(&message));
            }
            Ok(LogOutput::StdIn { .. } | LogOutput::Console { .. }) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    let inspected = docker
        .inspect_exec(&created.id)
        .await
        .map_err(|error| error.to_string())?;
    match inspected.exit_code {
        Some(0) => Ok(stdout),
        code => Err(format!(
            "exited with {code:?}; stdout: {stdout}; stderr: {stderr}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LARGE: EngineMemory = EngineMemory {
        root_limit_bytes: 40_366_070_169,
        overhead_percent: 10,
        per_gib: 1462,
    };

    const SMALL: EngineMemory = EngineMemory {
        root_limit_bytes: 16 * GIB / 10 * 6,
        overhead_percent: 10,
        per_gib: 1462,
    };

    #[test]
    fn the_ceiling_is_computed_as_the_master_computes_it() {
        assert_eq!(LARGE.ceiling_at(1462), 5496);
    }

    #[test]
    fn a_machine_whose_ceiling_already_admits_the_suites_is_left_alone() {
        let raise = LARGE.raise_for(TABLET_BUDGET);
        assert_eq!(raise.per_gib, None, "{raise:?}");
        assert!(raise.suites_admitted >= CONCURRENT_SUITES, "{raise:?}");
    }

    #[test]
    fn a_small_machine_is_raised_but_never_past_what_its_memory_holds() {
        let raise = SMALL.raise_for(TABLET_BUDGET);
        let per_gib = raise
            .per_gib
            .expect("a 16 GB machine's default ceiling is below the budget");
        assert!(per_gib > SMALL.per_gib);
        assert!(raise.raised_ceiling <= SMALL.affordable(), "{raise:?}");
        assert!(raise.raised_ceiling > raise.ceiling, "{raise:?}");
    }

    #[test]
    fn a_budget_memory_can_hold_is_admitted_in_full() {
        let raise = SMALL.raise_for(1500);
        assert!(raise.raised_ceiling >= 1500, "{raise:?}");
    }

    #[test]
    fn a_ceiling_somebody_raised_further_is_never_lowered() {
        let raised = EngineMemory {
            per_gib: 100_000,
            ..SMALL
        };
        assert_eq!(raised.raise_for(TABLET_BUDGET).per_gib, None);
    }

    #[test]
    fn the_engines_report_is_read_line_by_line() {
        let memory = EngineMemory::parse("40366070169\n10\n1462\n").expect("three numbers");
        assert_eq!(memory, LARGE);
        assert!(EngineMemory::parse("40366070169\n10\n").is_none());
        assert!(EngineMemory::parse("x\n10\n1462\n").is_none());
    }
}
