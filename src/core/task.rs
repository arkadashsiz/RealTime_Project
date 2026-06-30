
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


pub fn theta(lambda: f64) -> f64 {
    1.5 + lambda
}

pub fn normalize_laxities(laxities: &[f64]) -> Vec<f64> {
    if laxities.is_empty() {
        return vec![];
    }
    let min_l = laxities.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_l = laxities.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max_l - min_l;
    if range.abs() < 1e-9 {
        return vec![0.5; laxities.len()];
    }
    laxities.iter().map(|l| (l - min_l) / range).collect()
}


pub fn relaxation(theta_val: f64, l_norm: f64, priority: Priority) -> f64 {
    theta_val * l_norm + priority as f64
}
