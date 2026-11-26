use std::collections::HashMap;

use tracing::{Instrument, error, info, info_span, instrument, warn};

use crate::{Seed, SeedError};

/// Collects [`Seed`]s and runs them in a deterministic, dependency-aware order.
#[derive(Debug)]
#[must_use]
pub struct SeedRunner<C> {
    seeds: Vec<Seed<C>>,
}

impl<C> SeedRunner<C> {
    /// Creates an empty runner.
    pub fn new() -> Self {
        Self { seeds: Vec::new() }
    }

    /// Adds a single seed, returning the runner for chaining.
    pub fn register(mut self, seed: Seed<C>) -> Self {
        self.seeds.push(seed);
        self
    }

    /// Adds every seed from an iterator, returning the runner for chaining.
    pub fn extend(mut self, seeds: impl IntoIterator<Item = Seed<C>>) -> Self {
        self.seeds.extend(seeds);
        self
    }

    /// The number of registered seeds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.seeds.len()
    }

    /// Whether no seeds are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.seeds.is_empty()
    }

    /// Runs every registered seed, in order, against `ctx`, stopping at the first failure.
    ///
    /// # Errors
    ///
    /// Returns [`SeedError::CircularDependency`] if the `after` dependencies form a
    /// cycle, or whatever error a seed returns.
    #[instrument(skip_all, fields(seed_count = self.seeds.len()))]
    pub async fn run_all(&self, ctx: &C) -> Result<(), SeedError>
    where
        C: Sync,
    {
        if self.seeds.is_empty() {
            info!("no seeds registered; nothing to run");
            return Ok(());
        }

        let order = ordered_indices(&self.seeds)?;
        info!(count = order.len(), "running seeds");

        for &idx in &order {
            let seed = &self.seeds[idx];
            let span = info_span!("seed", name = seed.name(), order = seed.order());
            async {
                info!("running seed");
                if let Err(error) = seed.run(ctx).await {
                    error!(error = ?error, "seed failed");
                    return Err(error);
                }
                info!("seed complete");
                Ok(())
            }
            .instrument(span)
            .await?;
        }

        Ok(())
    }
}

impl<C> Default for SeedRunner<C> {
    fn default() -> Self {
        Self::new()
    }
}

/// The indices of `seeds` in run order; an `after` naming an unknown seed is logged and ignored.
#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
fn ordered_indices<C>(seeds: &[Seed<C>]) -> Result<Vec<usize>, SeedError> {
    let n = seeds.len();
    let name_to_idx: HashMap<&str, usize> = seeds
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name(), i))
        .collect();

    let mut in_degree = vec![0u32; n];
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, seed) in seeds.iter().enumerate() {
        let Some(dep_name) = seed.after() else {
            continue;
        };
        match name_to_idx.get(dep_name) {
            Some(&dep_idx) => {
                adjacency[dep_idx].push(i);
                in_degree[i] += 1;
            }
            None => {
                warn!(
                    seed = seed.name(),
                    missing = dep_name,
                    "seed declares `after` on an unknown seed; ignoring the dependency"
                );
            }
        }
    }

    // Within each layer, independent seeds come before dependent ones, then by `order`.
    let layer_sort = |batch: &mut Vec<usize>| {
        batch.sort_by(|&a, &b| {
            let a_dep = seeds[a].after().is_some();
            let b_dep = seeds[b].after().is_some();
            a_dep
                .cmp(&b_dep)
                .then(seeds[a].order().cmp(&seeds[b].order()))
                .then(seeds[a].name().cmp(seeds[b].name()))
        });
    };

    let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    layer_sort(&mut queue);

    let mut result = Vec::with_capacity(n);
    while !queue.is_empty() {
        let current = std::mem::take(&mut queue);
        let mut next: Vec<usize> = Vec::new();
        for idx in current {
            result.push(idx);
            for &neighbor in &adjacency[idx] {
                in_degree[neighbor] -= 1;
                if in_degree[neighbor] == 0 {
                    next.push(neighbor);
                }
            }
        }
        layer_sort(&mut next);
        queue = next;
    }

    if result.len() < n {
        let cycle: Vec<&str> = (0..n)
            .filter(|&i| in_degree[i] > 0)
            .map(|i| seeds[i].name())
            .collect();
        return Err(SeedError::CircularDependency {
            seeds: cycle.join(", "),
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(name: &'static str, order: u32, after: Option<&'static str>) -> Seed<()> {
        Seed::new(name, order, after, |_ctx| Box::pin(async { Ok(()) }))
    }

    fn names(seeds: &[Seed<()>]) -> Vec<&'static str> {
        ordered_indices(seeds)
            .expect("ordering should succeed")
            .into_iter()
            .map(|i| seeds[i].name())
            .collect()
    }

    #[test]
    fn independent_seeds_run_in_order_ascending() {
        let seeds = vec![
            seed("alpha", 200, None),
            seed("beta", 50, None),
            seed("gamma", 100, None),
        ];
        assert_eq!(names(&seeds), vec!["beta", "gamma", "alpha"]);
    }

    #[test]
    fn equal_order_breaks_ties_by_name_deterministically() {
        let seeds = vec![seed("zeta", 100, None), seed("alpha", 100, None)];
        assert_eq!(names(&seeds), vec!["alpha", "zeta"]);
    }

    #[test]
    fn after_dependency_runs_after_its_target() {
        let seeds = vec![
            seed("demo", 100, Some("bootstrap")),
            seed("bootstrap", 100, None),
            seed("extra", 50, None),
        ];
        assert_eq!(names(&seeds), vec!["extra", "bootstrap", "demo"]);
    }

    #[test]
    fn chain_of_dependencies_is_linearized() {
        let seeds = vec![
            seed("c", 100, Some("b")),
            seed("b", 100, Some("a")),
            seed("a", 100, None),
        ];
        assert_eq!(names(&seeds), vec!["a", "b", "c"]);
    }

    #[test]
    fn circular_dependency_is_rejected() {
        let seeds = vec![seed("a", 100, Some("b")), seed("b", 100, Some("a"))];
        let err = ordered_indices(&seeds).expect_err("a cycle has no valid order");
        match err {
            SeedError::CircularDependency { seeds } => {
                assert!(seeds.contains("a") && seeds.contains("b"), "got: {seeds}");
            }
            other => panic!("expected CircularDependency, got {other:?}"),
        }
    }

    #[test]
    fn missing_dependency_is_ignored_not_fatal() {
        let seeds = vec![seed("a", 100, Some("nonexistent")), seed("b", 50, None)];
        assert_eq!(names(&seeds), vec!["b", "a"]);
    }

    #[tokio::test]
    async fn empty_runner_is_a_noop() {
        let runner: SeedRunner<()> = SeedRunner::new();
        assert!(runner.is_empty());
        runner.run_all(&()).await.expect("empty run should succeed");
    }

    #[tokio::test]
    async fn run_all_executes_in_dependency_order() {
        use std::sync::Mutex;

        struct Ctx {
            log: Mutex<Vec<&'static str>>,
        }

        #[expect(clippy::excessive_nesting, reason = "readability baseline")]
        fn record(name: &'static str) -> Seed<Ctx> {
            match name {
                "first" => Seed::new("first", 10, None, |ctx: &Ctx| {
                    Box::pin(async move {
                        ctx.log.lock().unwrap().push("first");
                        Ok(())
                    })
                }),
                _ => Seed::new("second", 100, Some("first"), |ctx: &Ctx| {
                    Box::pin(async move {
                        ctx.log.lock().unwrap().push("second");
                        Ok(())
                    })
                }),
            }
        }

        let ctx = Ctx {
            log: Mutex::new(Vec::new()),
        };
        let runner = SeedRunner::new()
            .register(record("second"))
            .register(record("first"));
        runner.run_all(&ctx).await.expect("run should succeed");

        assert_eq!(*ctx.log.lock().unwrap(), vec!["first", "second"]);
    }

    #[tokio::test]
    async fn run_all_stops_at_the_first_failure() {
        #[expect(clippy::excessive_nesting, reason = "readability baseline")]
        fn boom() -> Seed<()> {
            Seed::new("boom", 10, None, |_ctx| {
                Box::pin(async { Err(SeedError::msg("kaboom")) })
            })
        }
        let runner = SeedRunner::new().register(boom());
        let err = runner.run_all(&()).await.expect_err("the seed fails");
        assert!(matches!(err, SeedError::Message { .. }));
    }
}
