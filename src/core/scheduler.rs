// src/core/scheduler.rs
use crate::core::task::Task;
use crate::core::simulator::SimConfig;
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
        
        // 1. Gather all tasks that aren't locked in a context switch
        let mut available_tasks = ready_queue.to_vec();
        for (i, core) in cores.iter().enumerate() {
            if core.is_switching {
                desired[i] = core.running_task; // Lock this core's state
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
            if cores[i].is_switching { continue; }
            if let Some(running) = cores[i].running_task {
                if let Some(pos) = top_tasks.iter().position(|&t| t == running) {
                    desired[i] = Some(running);
                    top_tasks.remove(pos); // Task is accounted for
                }
            }
        }

        // 5. Pass 2: Assign remaining high-priority tasks to remaining idle/preempted cores
        for i in 0..cores.len() {
            if cores[i].is_switching || desired[i].is_some() { continue; }
            desired[i] = top_tasks.pop(); 
        }

        desired
    }
}

// ==========================================
// 2. PARTITIONED EDF SCHEDULER
// ==========================================
pub struct PartitionedEdf {
    /// Maps task_id -> core_idx
    task_to_core: HashMap<usize, usize>,
    next_core_to_assign: usize,
}

impl Default for PartitionedEdf {
    fn default() -> Self {
        Self {
            task_to_core: HashMap::new(),
            next_core_to_assign: 0,
        }
    }
}

impl Scheduler for PartitionedEdf {
    fn on_task_arrival(&mut self, task_idx: usize, _tasks: &[Task], config: &SimConfig) {
        // Simple Round-Robin bin-packing. (Can be upgraded to Worst-Fit/Best-Fit later).
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