use super::CircuitState;
use std::time::Instant;

pub(crate) struct CircuitInternals {
    pub state: CircuitState,
    pub opened_at: Option<Instant>,
    pub failure_times: Vec<Instant>,
    pub half_open_successes: u32,
}
