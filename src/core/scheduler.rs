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

/// Safety margin for TTC comparisons.
///
/// The paper introduces δ_TTC but does not give a concrete numerical
/// value. Therefore this must be supplied by the experiment/configuration.
const TTC_MARGIN: f64 = 0.1;

/// Relaxation tie-band ε_R.
///
/// The paper introduces ε_R but does not specify a numerical value.
/// This is therefore an implementation parameter.
const RELAXATION_TIE_BAND: f64 = 0.1;

impl ProposedAlgorithm {
    /// Calculate TTC from the task's obstacle distance and the current
    /// environmental conditions.
    fn task_ttc(
        task: &Task,
        config: &SimConfig,
    ) -> f64 {
        let d_eff =
            crate::core::task::effective_distance(
                task.obstacle_distance,
                config.tightness,
            );

        crate::core::task::time_to_collision(
            d_eff,
            config.weather.lambda(),
        )
        .unwrap_or(crate::core::task::FALLBACK_TTC)
    }

    /// Returns true when `a` should be preferred to `b` according to
    /// the paper's lexicographic comparison rules.
    fn preferred(
        a: &Task,
        b: &Task,
        a_relaxation: f64,
        b_relaxation: f64,
        current_time: f64,
        config: &SimConfig,
    ) -> bool {
        let la = a.laxity(current_time);
        let lb = b.laxity(current_time);

        // Rule 1:
        // A task with non-negative laxity is preferred to a task
        // with negative laxity.
        if la >= 0.0 && lb < 0.0 {
            return true;
        }

        if la < 0.0 && lb >= 0.0 {
            return false;
        }

        let ttc_a = Self::task_ttc(a, config);
        let ttc_b = Self::task_ttc(b, config);

        // Rule 2 from the paper:
        //
        // TTC_i + δ_TTC > TTC_j
        //
        // The paper defines this as a precedence rule independent
        // of R. We reproduce that rule literally.
        if ttc_a + TTC_MARGIN > ttc_b {
            return true;
        }

        if ttc_b + TTC_MARGIN > ttc_a {
            return false;
        }

        // Rule 3:
        // If relaxation values are sufficiently close, use TTC
        // as the deterministic tie-breaker.
        if (a_relaxation - b_relaxation).abs()
            <= RELAXATION_TIE_BAND
        {
            return ttc_a < ttc_b;
        }

        // Otherwise lower relaxation wins.
        a_relaxation < b_relaxation
    }
}

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

        /*
         * Tasks currently undergoing a context switch cannot be changed.
         * They are also not considered candidates for another core.
         */
        let mut available_tasks = Vec::new();

        for &idx in ready_queue {
            let mut assigned_to_switching_core = false;

            for core in cores {
                if core.is_switching
                    && core.running_task == Some(idx)
                {
                    assigned_to_switching_core = true;
                    break;
                }
            }

            if !assigned_to_switching_core {
                available_tasks.push(idx);
            }
        }

        /*
         * Negative-laxity tasks should not be selected.
         *
         * The simulator also drops these tasks from the ready queue,
         * but filtering here makes the scheduler safe independently.
         */
        available_tasks.retain(|&idx| {
            tasks[idx].laxity(current_time) >= 0.0
        });

        if available_tasks.is_empty() {
            for (i, core) in cores.iter().enumerate() {
                if core.is_switching {
                    desired[i] = core.running_task;
                } else {
                    desired[i] = core.running_task;
                }
            }

            return desired;
        }

        /*
         * ------------------------------------------------------------
         * 1. Calculate laxities
         * ------------------------------------------------------------
         */
        let laxities: Vec<f64> = available_tasks
            .iter()
            .map(|&idx| tasks[idx].laxity(current_time))
            .collect();

        /*
         * ------------------------------------------------------------
         * 2. Normalize laxities to the priority scale
         * ------------------------------------------------------------
         */
        let normalized = normalize_laxities(&laxities);

        let theta_value =
            theta(config.weather.lambda());

        /*
         * ------------------------------------------------------------
         * 3. Calculate relaxation
         * ------------------------------------------------------------
         */
        let mut ranked: Vec<(usize, f64)> = available_tasks
            .iter()
            .enumerate()
            .map(|(i, &idx)| {
                let r = relaxation(
                    theta_value,
                    normalized[i],
                    tasks[idx].priority,
                );

                (idx, r)
            })
            .collect();

        /*
         * ------------------------------------------------------------
         * 4. Sort using the paper's comparison rules
         * ------------------------------------------------------------
         */
        ranked.sort_by(|(idx_a, r_a), (idx_b, r_b)| {
            if *idx_a == *idx_b {
                return std::cmp::Ordering::Equal;
            }

            if Self::preferred(
                &tasks[*idx_a],
                &tasks[*idx_b],
                *r_a,
                *r_b,
                current_time,
                config,
            ) {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        });

        let mut candidates: Vec<usize> =
            ranked.into_iter().map(|(idx, _)| idx).collect();

        /*
         * ------------------------------------------------------------
         * 5. Preserve tasks already running when appropriate,
         *    otherwise apply the paper's preemption condition.
         * ------------------------------------------------------------
         */
        for core_idx in 0..cores.len() {
            if cores[core_idx].is_switching {
                desired[core_idx] =
                    cores[core_idx].running_task;
                continue;
            }

            let running_idx =
                cores[core_idx].running_task;

            match running_idx {
                None => {
                    /*
                     * Idle core:
                     *
                     * Algorithm 1 says to assign the task with minimum
                     * relaxation.
                     */
                    if let Some(next) = candidates.first().copied() {
                        desired[core_idx] = Some(next);

                        candidates.retain(|&x| x != next);
                    }
                }

                Some(running) => {
                    /*
                     * If the currently running task is not in the
                     * waiting candidates, simply keep it.
                     */
                    let best =
                        candidates.first().copied();

                    match best {
                        None => {
                            desired[core_idx] =
                                Some(running);
                        }

                        Some(candidate) if candidate == running => {
                            desired[core_idx] =
                                Some(running);

                            candidates.retain(
                                |&x| x != running
                            );
                        }

                        Some(candidate) => {
                            let candidate_laxity =
                                tasks[candidate]
                                    .laxity(current_time);

                            /*
                             * Equation (8):
                             *
                             * L(candidate) >=
                             * C_remaining(running) + 2a
                             *
                             * means DON'T preempt.
                             *
                             * Therefore preemption is allowed only when:
                             *
                             * L(candidate) <
                             * C_remaining(running) + 2a
                             */
                            let preemption_threshold =
                                tasks[running].remaining_time
                                    + 2.0
                                        * config
                                            .context_switch_cost;

                            /*
                             * TTC margin / relaxation tie-band have
                             * already been incorporated into the
                             * candidate ordering.
                             */
                            if candidate_laxity
                                < preemption_threshold
                            {
                                /*
                                 * Preempt running task.
                                 *
                                 * The simulator keeps the preempted
                                 * task in ready_queue, so it becomes
                                 * eligible again on the next decision.
                                 */
                                desired[core_idx] =
                                    Some(candidate);

                                candidates.retain(
                                    |&x| x != candidate
                                );
                            } else {
                                /*
                                 * Non-preemption condition satisfied.
                                 */
                                desired[core_idx] =
                                    Some(running);

                                candidates.retain(
                                    |&x| x != running
                                );
                            }
                        }
                    }
                }
            }
        }

        /*
         * ------------------------------------------------------------
         * 6. Fill any remaining idle cores
         * ------------------------------------------------------------
         */
        for core_idx in 0..cores.len() {
            if cores[core_idx].is_switching {
                continue;
            }

            if desired[core_idx].is_none() {
                if let Some(next) =
                    candidates.first().copied()
                {
                    desired[core_idx] = Some(next);

                    candidates.retain(
                        |&x| x != next
                    );
                }
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