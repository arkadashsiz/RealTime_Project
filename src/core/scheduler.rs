// src/core/scheduler.rs
use crate::core::task::{normalize_laxities, relaxation, theta, Task};
use crate::core::simulator::SimConfig;

pub trait Scheduler {
    /// Ranks the ready queue. Returns a vector of task indices sorted 
    /// from highest priority (execute first) to lowest.
    fn rank_tasks(&mut self, ready_queue: &[usize], tasks: &[Task], current_time: f64, config: &SimConfig) -> Vec<usize>;
}

pub struct LegacyScheduler;

impl Scheduler for LegacyScheduler {
    fn rank_tasks(&mut self, ready_queue: &[usize], tasks: &[Task], current_time: f64, config: &SimConfig) -> Vec<usize> {
        let lambda = config.weather.lambda();
        let theta_val = theta(lambda);

        let laxities: Vec<f64> = ready_queue.iter().map(|&idx| tasks[idx].laxity(current_time)).collect();
        let l_norms = normalize_laxities(&laxities);
        
        let mut ranked: Vec<(usize, f64)> = ready_queue
            .iter()
            .zip(l_norms.iter())
            .map(|(&idx, &ln)| (idx, relaxation(theta_val, ln, tasks[idx].priority)))
            .collect();
            
        // Sort ascending by relaxation value
        ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        
        // Strip the relaxation values and return just the ordered indices
        // (Here, .collect() can still infer Vec<usize> because of the return signature)
        ranked.into_iter().map(|(idx, _)| idx).collect()
    }
}