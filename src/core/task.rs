use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Weather {
    Sunny,
    Rainy,
    Snowy,
}

impl Weather {
    /// lambda: models the effect of weather on braking power.
    pub fn lambda(&self) -> f64 {
        match self {
            Weather::Sunny => 0.0,
            Weather::Rainy => 0.3,
            Weather::Snowy => 0.6,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Weather::Sunny => "sunny",
            Weather::Rainy => "rainy",
            Weather::Snowy => "snowy",
        }
    }

    pub fn all() -> [Weather; 3] {
        [Weather::Sunny, Weather::Rainy, Weather::Snowy]
    }
}

/// Priority levels: 1 = highest priority, 5 = lowest, matching the spec.
pub type Priority = u8;

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: usize,
    pub arrival_time: f64,
    pub execution_time: f64,
    pub remaining_time: f64,
    pub priority: Priority,
    pub obstacle_distance: f64,
    pub deadline: f64,

    // --- Bookkeeping fields populated during simulation ---
    pub start_time: Option<f64>,
    pub finish_time: Option<f64>,
    pub dropped: bool,
    pub dropped_at: Option<f64>,
    pub context_switches_incurred: u32,
    pub completed: bool,
}

/// Physical constants from the spec.
pub const V0: f64 = 30.0; // initial vehicle speed, m/s
pub const A: f64 = 5.0; // braking deceleration, m/s^2

/// See README assumption (1): `d_effective = d / tightness` is our
/// convention for how Deadline Tightness enters the TTC formula.
/// Confirm with the TA before trusting downstream results.
pub fn effective_distance(obstacle_distance: f64, tightness: f64) -> f64 {
    obstacle_distance / tightness
}

pub fn time_to_collision(d_effective: f64, lambda: f64) -> Option<f64> {
    let denom = A * (1.0 - lambda);
    if denom <= 0.0 {
        // (1 - lambda) should always be > 0 given lambda in {0.0,0.3,0.6},
        // but guard defensively against degenerate inputs.
        return None;
    }
    let under_sqrt = V0 * V0 - 2.0 * denom * d_effective;
    if under_sqrt < 0.0 {
        return None;
    }
    let ttc = (V0 - under_sqrt.sqrt()) / denom;
    Some(ttc)
}

/// See README assumption (4): fallback TTC used when no real collision
/// solution exists under the given braking assumptions.
pub const FALLBACK_TTC: f64 = 50.0;

pub fn compute_deadline(
    arrival_time: f64,
    obstacle_distance: f64,
    weather: Weather,
    tightness: f64,
) -> f64 {
    let d_eff = effective_distance(obstacle_distance, tightness);
    let ttc = time_to_collision(d_eff, weather.lambda()).unwrap_or(FALLBACK_TTC);
    arrival_time + ttc
}

impl Task {
    /// Laxity at current time `t`:
    ///     Laxity = Deadline - t - RemainingExecutionTime
    pub fn laxity(&self, t: f64) -> f64 {
        self.deadline - t - self.remaining_time
    }
}

// ---------------------------------------------------------------------
// Relaxation (R) metric helpers.
//
// These implement the "Relaxation-based" ranking that the project spec
// (Project 6) names as the proposed Phase-1 algorithm: theta(lambda),
// per-tick laxity normalization, and R = theta * L_norm + priority.
//
// NOTE: no `Scheduler` impl in scheduler.rs currently calls these —
// only `GlobalEdf` and `PartitionedEdf` are wired up today. Keeping
// these functions is intentional (they are the building blocks for the
// actual Relaxation scheduler this project is named after), but a
// `RelaxationScheduler` implementing the `Scheduler` trait still needs
// to be added and used in `main.rs`/`experiment.rs` before Phase 1 is
// functionally complete. Flagging this rather than silently deleting
// "unused" code, since it looks like the real gap in this codebase.
// ---------------------------------------------------------------------

pub fn theta(lambda: f64) -> f64 {
    1.5 + lambda
}
pub const PRIORITY_LEVELS: f64 = 5.0;
pub fn normalize_laxities(laxities: &[f64]) -> Vec<f64> {
    if laxities.is_empty() {
        return vec![];
    }

    let min_l = laxities
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);

    let max_l = laxities
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);

    let range = max_l - min_l;

    if range.abs() < 1e-9 {
        return vec![PRIORITY_LEVELS / 2.0; laxities.len()];
    }

    laxities
        .iter()
        .map(|l| {
            ((l - min_l) / range) * PRIORITY_LEVELS
        })
        .collect()
}

pub fn relaxation(theta_val: f64, l_norm: f64, priority: f64) -> f64 {
    theta_val * l_norm + priority
}
