// src/core/generator.rs

use crate::core::task::{compute_deadline, Task, Weather};
use rand::Rng;

pub const DEFAULT_TASKS_PER_ROUND: usize = 30;

pub const ARRIVAL_MIN: f64 = 0.0;
pub const ARRIVAL_MAX: f64 = 30.0;
pub const EXEC_MIN: f64 = 1.0;
pub const EXEC_MAX: f64 = 13.0;
pub const OBSTACLE_DIST_MIN: f64 = 50.0;
pub const OBSTACLE_DIST_MAX: f64 = 150.0;

/// Generates a set of tasks where the total/average workload is controlled
/// by `target_utilization` using the UUnifast algorithm.
pub fn generate_tasks<R: Rng>(
    rng: &mut R,
    n: usize,
    weather: Weather,
    tightness: f64,
    target_utilization: f64,
) -> Vec<Task> {
    let mut tasks = Vec::with_capacity(n);

    // 1. Generate independent baseline parameters (arrival, obstacle distance, priority)
    let mut raw_data = Vec::with_capacity(n);
    for _ in 0..n {
        let arrival_time = rng.gen_range(ARRIVAL_MIN..=ARRIVAL_MAX);
        let priority = rng.gen_range(1..=5u8);
        let obstacle_distance = rng.gen_range(OBSTACLE_DIST_MIN..=OBSTACLE_DIST_MAX);
        let deadline = compute_deadline(arrival_time, obstacle_distance, weather, tightness);
        let window = (deadline - arrival_time).max(1.0); // Ensure window is positive

        raw_data.push((arrival_time, priority, obstacle_distance, deadline, window));
    }

    // 2. Apply UUnifast algorithm to distribute `target_utilization` across `n` tasks
    let mut sum_u = target_utilization;
    let mut utilizations = Vec::with_capacity(n);
    for i in 1..n {
        let next_sum_u = sum_u * rng.gen::<f64>().powf(1.0 / (n - i) as f64);
        utilizations.push(sum_u - next_sum_u);
        sum_u = next_sum_u;
    }
    utilizations.push(sum_u);

    // 3. Construct tasks using the generated utilizations and windows
    for (id, ((arrival_time, priority, obstacle_distance, deadline, window), u)) in
        raw_data.into_iter().zip(utilizations).enumerate()
    {
        // Derived execution time: C = utilization * window
        let mut execution_time = u * window;

        // Clamp execution time to reasonable execution bounds
        execution_time = execution_time.clamp(EXEC_MIN, EXEC_MAX);

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
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn generates_correct_count() {
        let mut rng = StdRng::seed_from_u64(42);
        let tasks = generate_tasks(&mut rng, DEFAULT_TASKS_PER_ROUND, Weather::Sunny, 1.0, 1.5);
        assert_eq!(tasks.len(), DEFAULT_TASKS_PER_ROUND);
    }

    #[test]
    fn respects_parameter_bounds() {
        let mut rng = StdRng::seed_from_u64(7);
        let tasks = generate_tasks(&mut rng, 1000, Weather::Rainy, 1.2, 2.0);
        for t in &tasks {
            assert!(t.arrival_time >= ARRIVAL_MIN && t.arrival_time <= ARRIVAL_MAX);
            assert!(t.execution_time >= EXEC_MIN && t.execution_time <= EXEC_MAX);
            assert!(t.priority >= 1 && t.priority <= 5);
            assert!(t.obstacle_distance >= OBSTACLE_DIST_MIN && t.obstacle_distance <= OBSTACLE_DIST_MAX);
            assert!(t.deadline >= t.arrival_time);
        }
    }
}
