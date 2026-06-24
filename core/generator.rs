//! generator.rs
//!
//! Generates randomized batches of tasks following the uniform
//! distributions specified in the project document:
//!   - Arrival time:      U[0, 30]
//!   - Execution time:    U[1, 13]
//!   - Priority:          discrete uniform over {1,2,3,4,5}
//!   - Obstacle distance: U[50, 150] meters
//!
//! Deadlines are derived afterward via the physics-based TTC model
//! in `task.rs`, which depends on weather and the deadline-tightness
//! coefficient — both supplied by the caller (the experiment runner),
//! not generated here.

use crate::task::{compute_deadline, Task, Weather};
use rand::Rng;

pub const DEFAULT_TASKS_PER_ROUND: usize = 30;

pub const ARRIVAL_MIN: f64 = 0.0;
pub const ARRIVAL_MAX: f64 = 30.0;
pub const EXEC_MIN: f64 = 1.0;
pub const EXEC_MAX: f64 = 13.0;
pub const OBSTACLE_DIST_MIN: f64 = 50.0;
pub const OBSTACLE_DIST_MAX: f64 = 150.0;

/// Generates `n` tasks for one simulation round under the given
/// `weather` and `tightness` settings, using `rng` for all random
/// draws (caller controls seeding for reproducibility).
pub fn generate_tasks<R: Rng>(
    rng: &mut R,
    n: usize,
    weather: Weather,
    tightness: f64,
) -> Vec<Task> {
    let mut tasks = Vec::with_capacity(n);

    for id in 0..n {
        let arrival_time = rng.gen_range(ARRIVAL_MIN..=ARRIVAL_MAX);
        let execution_time = rng.gen_range(EXEC_MIN..=EXEC_MAX);
        let priority = rng.gen_range(1..=5u8);
        let obstacle_distance = rng.gen_range(OBSTACLE_DIST_MIN..=OBSTACLE_DIST_MAX);

        let deadline = compute_deadline(arrival_time, obstacle_distance, weather, tightness);

        tasks.push(Task {
            id,
            arrival_time,
            execution_time,
            remaining_time: execution_time,
            priority,
            obstacle_distance,
            deadline,
            start_time: None,
            finish_time: None,
            dropped: false,
            dropped_at: None,
            context_switches_incurred: 0,
            completed: false,
        });
    }

    tasks
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn generates_correct_count() {
        let mut rng = StdRng::seed_from_u64(42);
        let tasks = generate_tasks(&mut rng, DEFAULT_TASKS_PER_ROUND, Weather::Sunny, 1.0);
        assert_eq!(tasks.len(), DEFAULT_TASKS_PER_ROUND);
    }

    #[test]
    fn respects_parameter_bounds() {
        let mut rng = StdRng::seed_from_u64(7);
        let tasks = generate_tasks(&mut rng, 1000, Weather::Rainy, 1.2);
        for t in &tasks {
            assert!(t.arrival_time >= ARRIVAL_MIN && t.arrival_time <= ARRIVAL_MAX);
            assert!(t.execution_time >= EXEC_MIN && t.execution_time <= EXEC_MAX);
            assert!(t.priority >= 1 && t.priority <= 5);
            assert!(t.obstacle_distance >= OBSTACLE_DIST_MIN && t.obstacle_distance <= OBSTACLE_DIST_MAX);
            assert!(t.deadline >= t.arrival_time); // deadline must be in the future
        }
    }
}
