// src/core/scheduler.rs
use crate::core::simulator::SimConfig;
use crate::core::task::Task;
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
        let mut ready_candidates = Vec::new();
        for &idx in ready_queue {
            let mut assigned = false;
            for core in cores {
                if core.is_switching && core.running_task == Some(idx) {
                    assigned = true;
                    break;
                }
            }
            if !assigned {
                ready_candidates.push(idx);
            }
        }

        let mut available_tasks = ready_candidates;
        for core in cores {
            if !core.is_switching {
                if let Some(idx) = core.running_task {
                    if !available_tasks.contains(&idx) {
                        available_tasks.push(idx);
                    }
                }
            }
        }

        // Sort by Earliest Deadline First, with task_idx as a deterministic tie-breaker
        available_tasks.sort_by(|&a, &b| {
            tasks[a].deadline.partial_cmp(&tasks[b].deadline).unwrap().then(a.cmp(&b))
        });

        let mut candidates = available_tasks;
        let mut desired = vec![None; cores.len()];

        // Pass 1: Handle context switching (locked)
        for (i, core) in cores.iter().enumerate() {
            if core.is_switching {
                desired[i] = core.running_task;
                if let Some(t) = core.running_task {
                    candidates.retain(|&x| x != t);
                }
            }
        }

        // Pass 2: Fill currently idle cores
        for (i, core) in cores.iter().enumerate() {
            if !core.is_switching && core.running_task.is_none() {
                if let Some(next) = candidates.first().copied() {
                    desired[i] = Some(next);
                    candidates.retain(|&x| x != next);
                }
            }
        }

        // Pass 3: Preemption check for busy cores
        for (i, core) in cores.iter().enumerate() {
            if desired[i].is_some() { continue; }
            if let Some(running) = core.running_task {
                if let Some(best) = candidates.first().copied() {
                    if best == running {
                        desired[i] = Some(running);
                        candidates.retain(|&x| x != running);
                    } else {
                        // Strict preemption condition for EDF
                        if tasks[best].deadline < tasks[running].deadline {
                            desired[i] = Some(best);
                            candidates.retain(|&x| x != best);
                        } else {
                            desired[i] = Some(running);
                            candidates.retain(|&x| x != running);
                        }
                    }
                } else {
                    desired[i] = Some(running);
                }
            }
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

            // 2. Sort by deadline with deterministic tie-breaker
            core_tasks.sort_by(|&a, &b| {
                tasks[a].deadline.partial_cmp(&tasks[b].deadline).unwrap().then(a.cmp(&b))
            });

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
        let mut ready_candidates = Vec::new();
        for &idx in ready_queue {
            let mut assigned = false;
            for core in cores {
                if core.is_switching && core.running_task == Some(idx) {
                    assigned = true;
                    break;
                }
            }
            if !assigned {
                ready_candidates.push(idx);
            }
        }

        let mut available_tasks = ready_candidates;
        for core in cores {
            if !core.is_switching {
                if let Some(idx) = core.running_task {
                    if !available_tasks.contains(&idx) {
                        available_tasks.push(idx);
                    }
                }
            }
        }

        // Sort by Laxity (Lowest First), task_idx as tie-breaker
        available_tasks.sort_by(|&a, &b| {
            let lax_a = tasks[a].laxity(current_time);
            let lax_b = tasks[b].laxity(current_time);
            lax_a.partial_cmp(&lax_b).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b))
        });

        let mut candidates = available_tasks;
        let mut desired = vec![None; cores.len()];

        for (i, core) in cores.iter().enumerate() {
            if core.is_switching {
                desired[i] = core.running_task;
                if let Some(t) = core.running_task {
                    candidates.retain(|&x| x != t);
                }
            }
        }

        for (i, core) in cores.iter().enumerate() {
            if !core.is_switching && core.running_task.is_none() {
                if let Some(next) = candidates.first().copied() {
                    desired[i] = Some(next);
                    candidates.retain(|&x| x != next);
                }
            }
        }

        for (i, core) in cores.iter().enumerate() {
            if desired[i].is_some() { continue; }
            if let Some(running) = core.running_task {
                if let Some(best) = candidates.first().copied() {
                    if best == running {
                        desired[i] = Some(running);
                        candidates.retain(|&x| x != running);
                    } else {
                        // Strict preemption condition for MLLF
                        if tasks[best].laxity(current_time) < tasks[running].laxity(current_time) {
                            desired[i] = Some(best);
                            candidates.retain(|&x| x != best);
                        } else {
                            desired[i] = Some(running);
                            candidates.retain(|&x| x != running);
                        }
                    }
                } else {
                    desired[i] = Some(running);
                }
            }
        }

        desired
    }
}

// ==========================================
// 4. PROPOSED HYBRID ALGORITHM (RELAX)
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
        let mut ready_candidates = Vec::new();
        for &idx in ready_queue {
            let mut assigned = false;
            for core in cores {
                if core.is_switching && core.running_task == Some(idx) {
                    assigned = true;
                    break;
                }
            }
            if !assigned {
                ready_candidates.push(idx);
            }
        }

        // Proactive Drop: Filter out tasks locked in an unschedulable state
        ready_candidates.retain(|&idx| tasks[idx].laxity(current_time) >= 0.0);

        let mut available_tasks = ready_candidates;
        
        // Add existing running tasks to pool
        for core in cores {
            if !core.is_switching {
                if let Some(idx) = core.running_task {
                    if !available_tasks.contains(&idx) {
                        available_tasks.push(idx);
                    }
                }
            }
        }

        if available_tasks.is_empty() {
            let mut desired = vec![None; cores.len()];
            for (i, core) in cores.iter().enumerate() {
                desired[i] = core.running_task;
            }
            return desired;
        }

        // 1. Calculate & Normalize laxities over current pool dynamically
        let laxities: Vec<f64> = available_tasks.iter().map(|&idx| tasks[idx].laxity(current_time)).collect();
        let l_min = laxities.iter().copied().fold(f64::INFINITY, f64::min);
        let l_max = laxities.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mut span = l_max - l_min;
        if span < 1e-9 { span = 1e-9; }

        // 2. Base configuration constants
        let theta = 4.0;
        let mut rvals = HashMap::new();
        
        for (i, &idx) in available_tasks.iter().enumerate() {
            let l_norm = ((laxities[i] - l_min) / span) * 5.0;
            let r = theta * l_norm + (tasks[idx].priority as f64);
            rvals.insert(idx, r);
        }

        // 3. Sort using paper's lexicographic comparison rules
        available_tasks.sort_by(|&a_idx, &b_idx| {
            let r_a = rvals[&a_idx];
            let r_b = rvals[&b_idx];

            if (r_a - r_b).abs() <= 0.5 {
                let ttc_a = tasks[a_idx].deadline - tasks[a_idx].arrival_time;
                let ttc_b = tasks[b_idx].deadline - tasks[b_idx].arrival_time;
                
                if ttc_b - ttc_a > 0.5 { return std::cmp::Ordering::Less; }
                if ttc_a - ttc_b > 0.5 { return std::cmp::Ordering::Greater; }
                
                if r_a != r_b {
                    return if r_a < r_b { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
                }
                return a_idx.cmp(&b_idx);
            }
            
            if r_a < r_b { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }
        });

        let mut candidates = available_tasks;
        let mut desired = vec![None; cores.len()];

        // 4. Pass 1: Locked states (context switches)
        for (i, core) in cores.iter().enumerate() {
            if core.is_switching {
                desired[i] = core.running_task;
                if let Some(t) = core.running_task {
                    candidates.retain(|&x| x != t);
                }
            }
        }

        // 5. Pass 2: Assign idle cores
        for (i, core) in cores.iter().enumerate() {
            if !core.is_switching && core.running_task.is_none() {
                if let Some(next) = candidates.first().copied() {
                    desired[i] = Some(next);
                    candidates.retain(|&x| x != next);
                }
            }
        }

        // 6. Pass 3: Evaluate Preemptions (Gated)
        for (i, core) in cores.iter().enumerate() {
            if desired[i].is_some() { continue; }
            if let Some(running) = core.running_task {
                if let Some(best) = candidates.first().copied() {
                    if best == running {
                        desired[i] = Some(running);
                        candidates.retain(|&x| x != running);
                    } else {
                        let candidate_laxity = tasks[best].laxity(current_time);
                        let preemption_threshold = tasks[running].remaining_time + 2.0 * config.context_switch_cost;

                        if candidate_laxity < preemption_threshold {
                            desired[i] = Some(best);
                            candidates.retain(|&x| x != best);
                        } else {
                            desired[i] = Some(running);
                            candidates.retain(|&x| x != running);
                        }
                    }
                } else {
                    desired[i] = Some(running);
                }
            }
        }

        desired
    }
}


// ==========================================
// 5. PROPOSED HYBRID ALGORITHM (RELAX_ENV)
// ==========================================
pub struct EnvProposedAlgorithm;

impl Scheduler for EnvProposedAlgorithm {
    fn schedule(
        &mut self,
        ready_queue: &[usize],
        cores: &[CoreView],
        tasks: &[Task],
        current_time: f64,
        config: &SimConfig,
    ) -> Vec<Option<usize>> {
        let mut ready_candidates = Vec::new();
        for &idx in ready_queue {
            let mut assigned = false;
            for core in cores {
                if core.is_switching && core.running_task == Some(idx) {
                    assigned = true;
                    break;
                }
            }
            if !assigned {
                ready_candidates.push(idx);
            }
        }

        // Proactive drop for Env-variant
        ready_candidates.retain(|&idx| tasks[idx].laxity(current_time) >= 0.0);

        let mut available_tasks = ready_candidates;
        for core in cores {
            if !core.is_switching {
                if let Some(idx) = core.running_task {
                    if !available_tasks.contains(&idx) {
                        available_tasks.push(idx);
                    }
                }
            }
        }

        if available_tasks.is_empty() {
            let mut desired = vec![None; cores.len()];
            for (i, core) in cores.iter().enumerate() {
                desired[i] = core.running_task;
            }
            return desired;
        }

        let laxities: Vec<f64> = available_tasks.iter().map(|&idx| tasks[idx].laxity(current_time)).collect();
        let l_min = laxities.iter().copied().fold(f64::INFINITY, f64::min);
        let l_max = laxities.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mut span = l_max - l_min;
        if span < 1e-9 { span = 1e-9; }

        let mut rvals = HashMap::new();
        let lambda = f64::from(config.weather.lambda());
        let theta = 1.5 + lambda;

        for (i, &idx) in available_tasks.iter().enumerate() {
            let l_norm = ((laxities[i] - l_min) / span) * 5.0;
            
            // Dynamic Priority adjustment (P_i) based on environment
            let mut p = tasks[idx].priority as f64;
            if lambda > 0.0 && tasks[idx].obstacle_distance < 100.0 {
                let shift = if (lambda - 0.3).abs() < 0.1 { 1.0 } else { 2.0 };
                p -= shift;
                if p < 1.0 { p = 1.0; }
            }

            let r = theta * l_norm + p;
            rvals.insert(idx, r);
        }

        available_tasks.sort_by(|&a_idx, &b_idx| {
            let r_a = rvals[&a_idx];
            let r_b = rvals[&b_idx];

            if (r_a - r_b).abs() <= 0.5 {
                let ttc_a = tasks[a_idx].deadline - tasks[a_idx].arrival_time;
                let ttc_b = tasks[b_idx].deadline - tasks[b_idx].arrival_time;
                
                if ttc_b - ttc_a > 0.5 { return std::cmp::Ordering::Less; }
                if ttc_a - ttc_b > 0.5 { return std::cmp::Ordering::Greater; }
                
                if r_a != r_b {
                    return if r_a < r_b { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
                }
                return a_idx.cmp(&b_idx);
            }
            if r_a < r_b { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }
        });

        let mut candidates = available_tasks;
        let mut desired = vec![None; cores.len()];

        for (i, core) in cores.iter().enumerate() {
            if core.is_switching {
                desired[i] = core.running_task;
                if let Some(t) = core.running_task {
                    candidates.retain(|&x| x != t);
                }
            }
        }

        for (i, core) in cores.iter().enumerate() {
            if !core.is_switching && core.running_task.is_none() {
                if let Some(next) = candidates.first().copied() {
                    desired[i] = Some(next);
                    candidates.retain(|&x| x != next);
                }
            }
        }

        for (i, core) in cores.iter().enumerate() {
            if desired[i].is_some() { continue; }
            if let Some(running) = core.running_task {
                if let Some(best) = candidates.first().copied() {
                    if best == running {
                        desired[i] = Some(running);
                        candidates.retain(|&x| x != running);
                    } else {
                        let candidate_laxity = tasks[best].laxity(current_time);
                        let preemption_threshold = tasks[running].remaining_time + 2.0 * config.context_switch_cost;

                        if candidate_laxity < preemption_threshold {
                            desired[i] = Some(best);
                            candidates.retain(|&x| x != best);
                        } else {
                            desired[i] = Some(running);
                            candidates.retain(|&x| x != running);
                        }
                    }
                } else {
                    desired[i] = Some(running);
                }
            }
        }

        desired
    }
}


// ==========================================
// 6. RUNTIME SCHEDULER SELECTION
// ==========================================
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerKind {
    GlobalEdf,
    PartitionedEdf,
    Mllf,
    Proposed,
    EnvProposed,
}

impl SchedulerKind {
    pub const ALL: [SchedulerKind; 5] = [
        SchedulerKind::GlobalEdf,
        SchedulerKind::PartitionedEdf,
        SchedulerKind::Mllf,
        SchedulerKind::Proposed,
        SchedulerKind::EnvProposed,
    ];

    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().replace(['_', ' '], "-").as_str() {
            "global-edf" | "globaledf" | "edf" | "edf-global" => Ok(SchedulerKind::GlobalEdf),
            "partitioned-edf" | "partitionededf" => Ok(SchedulerKind::PartitionedEdf),
            "mllf" => Ok(SchedulerKind::Mllf),
            "proposed" | "hybrid" => Ok(SchedulerKind::Proposed),
            "env-proposed" | "env" | "extended-relaxation" => Ok(SchedulerKind::EnvProposed),
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
            SchedulerKind::EnvProposed => Box::new(EnvProposedAlgorithm),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SchedulerKind::GlobalEdf => "global-edf",
            SchedulerKind::PartitionedEdf => "partitioned-edf",
            SchedulerKind::Mllf => "mllf",
            SchedulerKind::Proposed => "proposed",
            SchedulerKind::EnvProposed => "env-proposed",
        }
    }
}

impl Default for SchedulerKind {
    fn default() -> Self {
        SchedulerKind::GlobalEdf
    }
}