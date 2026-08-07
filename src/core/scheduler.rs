// src/core/scheduler.rs
use crate::core::simulator::SimConfig;
use crate::core::task::{normalize_laxities, relaxation, theta, Task};
use std::collections::HashMap;

/// A snapshot of a core's current state, provided to the scheduler.
#[derive(Debug, Clone, Copy)]
pub struct CoreView {
    pub running_task: Option<usize>,
    pub is_switching: bool,
}

pub trait Scheduler {
    /// Called exactly once when a task arrives in the system.
    /// Partitioned schedulers use this to assign tasks to specific cores.
    fn on_task_arrival(&mut self, _task_idx: usize, _tasks: &[Task], _config: &SimConfig) {}

    /// Called every tick. Returns a vector of length `cores.len()`.
    /// Each element is the `task_idx` the core SHOULD run, or `None` if it should idle.
    fn schedule(
        &mut self,
        ready_queue: &[usize],
        cores: &[CoreView],
        tasks: &[Task],
        current_time: f64,
        config: &SimConfig,
    ) -> Vec<Option<usize>>;
}

// ==========================================
// 1. GLOBAL EDF SCHEDULER
// ==========================================
pub struct GlobalEdf;

impl Scheduler for GlobalEdf {
    fn schedule(
        &mut self,
        ready_queue: &[usize],
        cores: &[CoreView],
        tasks: &[Task],
        _current_time: f64,
        _config: &SimConfig,
    ) -> Vec<Option<usize>> {
        let mut desired = vec![None; cores.len()];
        let mut available_tasks = ready_queue.to_vec();

        // 1. Gather all tasks that aren't locked in a context switch
        for (i, core) in cores.iter().enumerate() {
            if core.is_switching {
                desired[i] = core.running_task;
                if let Some(t) = core.running_task {
                    available_tasks.retain(|&idx| idx != t);
                }
            }
        }

        // 2. Sort by Earliest Deadline First
        available_tasks.sort_by(|&a, &b| tasks[a].deadline.partial_cmp(&tasks[b].deadline).unwrap());

        // 3. Identify the highest priority tasks we have capacity for
        let unlocked_cores = cores.iter().filter(|c| !c.is_switching).count();
        let mut top_tasks: Vec<usize> = available_tasks.into_iter().take(unlocked_cores).collect();

        // 4. Pass 1 (Affinity): Keep tasks on their current cores to avoid context switches
        for i in 0..cores.len() {
            if cores[i].is_switching {
                continue;
            }
            if let Some(running) = cores[i].running_task {
                if let Some(pos) = top_tasks.iter().position(|&t| t == running) {
                    desired[i] = Some(running);
                    top_tasks.remove(pos); // Task is accounted for
                }
            }
        }

        // 5. Pass 2: Assign remaining high-priority tasks to remaining idle/preempted cores
        for i in 0..cores.len() {
            if cores[i].is_switching || desired[i].is_some() {
                continue;
            }
            desired[i] = top_tasks.pop();
        }

        desired
    }
}

// ==========================================
// 2. PARTITIONED EDF SCHEDULER
// ==========================================
#[derive(Default)]
pub struct PartitionedEdf {
    /// Maps task_id -> core_idx
    task_to_core: HashMap<usize, usize>,
    next_core_to_assign: usize,
}

impl Scheduler for PartitionedEdf {
    fn on_task_arrival(&mut self, task_idx: usize, _tasks: &[Task], config: &SimConfig) {
        // Simple Round-Robin bin-packing.
        self.task_to_core.insert(task_idx, self.next_core_to_assign);
        self.next_core_to_assign = (self.next_core_to_assign + 1) % config.num_cores;
    }

    fn schedule(
        &mut self,
        ready_queue: &[usize],
        cores: &[CoreView],
        tasks: &[Task],
        _current_time: f64,
        _config: &SimConfig,
    ) -> Vec<Option<usize>> {
        let mut desired = vec![None; cores.len()];

        for (core_idx, core) in cores.iter().enumerate() {
            if core.is_switching {
                desired[core_idx] = core.running_task;
                continue;
            }

            // 1. Filter ready queue to ONLY tasks partitioned to this specific core
            let mut core_tasks: Vec<usize> = ready_queue
                .iter()
                .copied()
                .filter(|t| self.task_to_core.get(t) == Some(&core_idx))
                .collect();

            // 2. Sort by deadline
            core_tasks.sort_by(|&a, &b| tasks[a].deadline.partial_cmp(&tasks[b].deadline).unwrap());

            // 3. Pick the single best task for this core
            desired[core_idx] = core_tasks.first().copied();
        }

        desired
    }
}

// ==========================================
// 3. MODIFIED LEAST LAXITY FIRST (MLLF)
// ==========================================
pub struct Mllf;

impl Scheduler for Mllf {
    fn schedule(
        &mut self,
        ready_queue: &[usize],
        cores: &[CoreView],
        tasks: &[Task],
        current_time: f64,
        _config: &SimConfig,
    ) -> Vec<Option<usize>> {
        let mut desired = vec![None; cores.len()];
        let mut available_tasks = ready_queue.to_vec();

        // 1. Filter out tasks locked in a context switch
        for (i, core) in cores.iter().enumerate() {
            if core.is_switching {
                desired[i] = core.running_task;
                if let Some(t) = core.running_task {
                    available_tasks.retain(|&idx| idx != t);
                }
            }
        }

        // 2. Sort by Laxity (Lowest First)
        available_tasks.sort_by(|&a, &b| {
            let laxity_a = tasks[a].laxity(current_time);
            let laxity_b = tasks[b].laxity(current_time);
            laxity_a.partial_cmp(&laxity_b).unwrap_or(std::cmp::Ordering::Equal)
        });

        // 3. Affinity Pass
        let unlocked_cores = cores.iter().filter(|c| !c.is_switching).count();
        let mut top_tasks: Vec<usize> = available_tasks.into_iter().take(unlocked_cores).collect();

        for i in 0..cores.len() {
            if cores[i].is_switching { continue; }
            if let Some(running) = cores[i].running_task {
                if let Some(pos) = top_tasks.iter().position(|&t| t == running) {
                    desired[i] = Some(running);
                    top_tasks.remove(pos);
                }
            }
        }

        // 4. Fill remaining idle cores
        for i in 0..cores.len() {
            if cores[i].is_switching || desired[i].is_some() { continue; }
            // Using remove(0) keeps assigning the lowest laxity task first
            if !top_tasks.is_empty() {
                desired[i] = Some(top_tasks.remove(0));
            }
        }

        desired
    }
}

// ==========================================
// 4. PROPOSED HYBRID ALGORITHM
// ==========================================
pub struct ProposedAlgorithm;

impl Scheduler for ProposedAlgorithm {
    fn schedule(
        &mut self,
        ready_queue: &[usize],
        cores: &[CoreView],
        tasks: &[Task],
        current_time: f64,
        config: &SimConfig,
    ) -> Vec<Option<usize>> {
        let mut desired = vec![None; cores.len()];
        let mut available_tasks = ready_queue.to_vec();

        for (i, core) in cores.iter().enumerate() {
            if core.is_switching {
                desired[i] = core.running_task;
                if let Some(t) = core.running_task {
                    available_tasks.retain(|&idx| idx != t);
                }
            }
        }

        if available_tasks.is_empty() {
            return desired;
        }

        // 1. Compute and normalize laxities
        let laxities: Vec<f64> = available_tasks.iter().map(|&idx| tasks[idx].laxity(current_time)).collect();
        let norm_laxities = normalize_laxities(&laxities);
        let theta_val = theta(config.weather.lambda());

        // 2. Calculate Relaxation Metric R_i for each task
        let mut metrics: Vec<(usize, f64)> = available_tasks
            .into_iter()
            .enumerate()
            .map(|(i, idx)| {
                let r_i = relaxation(theta_val, norm_laxities[i], tasks[idx].priority);
                (idx, r_i)
            })
            .collect();

        // 3. Sort ascending by R_i (lowest R_i is the most critical)
        metrics.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut top_tasks: Vec<usize> = metrics.into_iter().map(|(idx, _)| idx).collect();

        // 4. Assign to cores enforcing the Preemption Inequality
        for i in 0..cores.len() {
            if cores[i].is_switching { continue; }
            
            if let Some(running_idx) = cores[i].running_task {
                if let Some(&incoming_idx) = top_tasks.first() {
                    if incoming_idx == running_idx {
                        // The running task is still the absolute best task
                        desired[i] = Some(running_idx);
                        top_tasks.remove(0);
                    } else {
                        // The running task is NOT the best. Check if we should preempt it.
                        // Formula: L(new) < C_rem(running) + 2 * alpha
                        let incoming_laxity = tasks[incoming_idx].laxity(current_time);
                        let threshold = tasks[running_idx].remaining_time + (2.0 * config.context_switch_cost);

                        if incoming_laxity < threshold {
                            // Preemption strictly allowed
                            desired[i] = Some(incoming_idx);
                            top_tasks.remove(0);
                        } else {
                            // Preemption failed. Keep running the current task.
                            desired[i] = Some(running_idx);
                            if let Some(pos) = top_tasks.iter().position(|&t| t == running_idx) {
                                top_tasks.remove(pos); // Ensure it's not reassigned elsewhere
                            }
                        }
                    }
                } else {
                    desired[i] = Some(running_idx);
                }
            }
        }

        // 5. Fill idle cores with the best remaining top tasks
        for i in 0..cores.len() {
            if cores[i].is_switching || desired[i].is_some() { continue; }
            if !top_tasks.is_empty() {
                desired[i] = Some(top_tasks.remove(0));
            }
        }

        desired
    }
}

// ==========================================
// 5. RUNTIME SCHEDULER SELECTION
// ==========================================
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerKind {
    GlobalEdf,
    PartitionedEdf,
    Mllf,
    Proposed,
}

impl SchedulerKind {
    pub const ALL: [SchedulerKind; 4] = [
        SchedulerKind::GlobalEdf,
        SchedulerKind::PartitionedEdf,
        SchedulerKind::Mllf,
        SchedulerKind::Proposed,
    ];

    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().replace(['_', ' '], "-").as_str() {
            "global-edf" | "globaledf" | "edf" | "edf-global" => Ok(SchedulerKind::GlobalEdf),
            "partitioned-edf" | "partitionededf" => Ok(SchedulerKind::PartitionedEdf),
            "mllf" => Ok(SchedulerKind::Mllf),
            "proposed" | "hybrid" => Ok(SchedulerKind::Proposed),
            other => Err(format!(
                "unknown scheduler '{other}' (expected one of: {})",
                SchedulerKind::ALL.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")
            )),
        }
    }

    pub fn build(self) -> Box<dyn Scheduler> {
        match self {
            SchedulerKind::GlobalEdf => Box::new(GlobalEdf),
            SchedulerKind::PartitionedEdf => Box::new(PartitionedEdf::default()),
            SchedulerKind::Mllf => Box::new(Mllf),
            SchedulerKind::Proposed => Box::new(ProposedAlgorithm),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SchedulerKind::GlobalEdf => "global-edf",
            SchedulerKind::PartitionedEdf => "partitioned-edf",
            SchedulerKind::Mllf => "mllf",
            SchedulerKind::Proposed => "proposed",
        }
    }
}

impl Default for SchedulerKind {
    fn default() -> Self {
        SchedulerKind::GlobalEdf
    }
}