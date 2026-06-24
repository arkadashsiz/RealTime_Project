//! simulator.rs
//!
//! Discrete-time, tick-based simulator implementing the proposed
//! "Relaxation" scheduling algorithm for ADAS aperiodic tasks on a
//! homogeneous multi-core processor.
//!
//! Design summary:
//!   - Time advances in fixed 1-unit ticks (1 tick = 10ms real time).
//!   - Each core can run at most one task per tick.
//!   - A task already running on a core stays there unless preempted
//!     (no task migrates between cores mid-execution).
//!   - At every tick we: (1) admit newly arrived tasks into the ready
//!     queue, (2) drop any task whose laxity has gone negative,
//!     (3) compute Relaxation R for everything in the ready queue and
//!     rank ascending, (4) decide preemptions per core, (5) assign
//!     waiting tasks to free cores, (6) execute one tick of work on
//!     every busy, non-switching core, (7) record completions.
//!
//! All task references inside this module use the task's **index**
//! into the caller-provided `tasks` slice (not `task.id`) as the
//! identity used for core occupancy bookkeeping. `task.id` is only
//! used for human-readable event logging.
//!
//! CORE ASSIGNMENT NOTE: the spec describes a single combined ready
//! queue sorted by R but does not specify how tasks are distributed
//! across multiple cores. We adopt a global-queue convention (the
//! same style as "EDF-Global"): at each tick, free cores pull the
//! most eligible (lowest-R) waiting tasks from the front of the
//! ranked queue, and a busy core's incumbent is only swapped out when
//! the preemption condition below is met for that specific core.

use crate::task::{normalize_laxities, relaxation, theta, Task, Weather};

/// A trace event emitted by the simulator for later analysis /
/// debugging. Full per-task results are read off the `Task` structs
/// themselves after the run completes; this trace is supplementary
/// (e.g. for building Gantt-style charts in Phase 2).
#[derive(Debug, Clone)]
pub enum SimEvent {
    Started { time: f64, task_id: usize, core: usize, preemptive: bool },
    Preempted { time: f64, task_id: usize, core: usize },
    Completed { time: f64, task_id: usize },
    Dropped { time: f64, task_id: usize, reason: DropReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    NegativeLaxity,
}

/// State of one processor core during simulation. `running` holds
/// the **index into the tasks slice** of the task currently occupying
/// this core, if any.
#[derive(Debug, Clone)]
struct Core {
    running: Option<usize>,
}

/// Configuration parameters for a single simulation run.
#[derive(Debug, Clone, Copy)]
pub struct SimConfig {
    pub num_cores: usize,
    pub weather: Weather,
    pub tightness: f64,
    pub context_switch_cost: f64,
    /// "Critical coefficient" C used in the preemption-severity test.
    /// The spec uses this symbol without specifying a concrete value;
    /// exposed here as a tunable constant. Documented assumption —
    /// see README / project plan, flagged ambiguity #2.
    pub critical_coefficient: f64,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            num_cores: 2,
            weather: Weather::Sunny,
            tightness: 1.0,
            context_switch_cost: 1.0,
            critical_coefficient: 5.0,
        }
    }
}

/// Aggregate, per-run statistics useful for plotting later (Phase 2
/// will consume these across 100 runs per configuration).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SimResult {
    pub total_tasks: usize,
    pub completed_on_time: usize,
    pub dropped_count: usize,
    pub deadline_miss_count: usize, // completed late OR dropped OR never finished
    pub total_context_switches: u32,
    pub makespan: f64,
    /// Per-priority-level (1..=5) miss counts: index 0 = priority 1.
    pub misses_by_priority: [usize; 5],
    pub tasks_by_priority: [usize; 5],
}

/// Runs one full simulation episode over `tasks` (already generated,
/// with deadlines already computed) using the proposed Relaxation
/// scheduler. Mutates `tasks` in place to record start/finish/drop
/// bookkeeping, and returns the event trace plus aggregate stats.
pub fn run_simulation(tasks: &mut [Task], config: &SimConfig) -> (Vec<SimEvent>, SimResult) {
    let mut events = Vec::new();
    let mut cores: Vec<Core> = (0..config.num_cores).map(|_| Core { running: None }).collect();

    let lambda = config.weather.lambda();
    let theta_val = theta(lambda);

    // Indices of tasks, sorted by arrival time for efficient
    // sequential admission into the ready queue.
    let mut arrival_order: Vec<usize> = (0..tasks.len()).collect();
    arrival_order.sort_by(|&a, &b| {
        tasks[a].arrival_time.partial_cmp(&tasks[b].arrival_time).unwrap()
    });
    let mut next_arrival_ptr = 0usize;

    // Ready queue: indices into `tasks` of admitted tasks that are
    // neither completed nor dropped. This includes tasks currently
    // running on a core (kept in the queue for ranking purposes, but
    // skipped when assigning *free* cores since they're already busy
    // elsewhere).
    let mut ready_queue: Vec<usize> = Vec::new();

    let mut t: f64 = 0.0;
    let max_deadline = tasks.iter().fold(0.0_f64, |acc, tk| acc.max(tk.deadline));
    // Safety horizon so the loop always terminates even under
    // pathological scheduling; +50 margin beyond the latest deadline.
    let horizon = max_deadline + 50.0;

    let mut total_context_switches: u32 = 0;
    let mut last_completion_time: f64 = 0.0;

    // Per-core countdown of context-switch overhead ticks remaining.
    // While > 0, that core executes no task work this tick.
    let mut switch_overhead_remaining: Vec<f64> = vec![0.0; config.num_cores];

    loop {
        if t >= horizon {
            break;
        }

        // ---- 1. Admit newly arrived tasks ----
        while next_arrival_ptr < arrival_order.len()
            && tasks[arrival_order[next_arrival_ptr]].arrival_time <= t
        {
            let idx = arrival_order[next_arrival_ptr];
            ready_queue.push(idx);
            next_arrival_ptr += 1;
        }

        // Termination check: nothing left to admit, ready queue
        // empty, and all cores idle => simulation is done.
        if next_arrival_ptr >= arrival_order.len()
            && ready_queue.is_empty()
            && cores.iter().all(|c| c.running.is_none())
        {
            break;
        }

        // ---- 2. Drop any task whose laxity has gone negative ----
        // Per spec: checked continuously, not just at arrival.
        let mut newly_dropped: Vec<usize> = Vec::new();
        ready_queue.retain(|&idx| {
            if tasks[idx].laxity(t) < 0.0 {
                newly_dropped.push(idx);
                false
            } else {
                true
            }
        });
        for idx in newly_dropped {
            tasks[idx].dropped = true;
            tasks[idx].dropped_at = Some(t);
            for (ci, core) in cores.iter_mut().enumerate() {
                if core.running == Some(idx) {
                    core.running = None;
                    switch_overhead_remaining[ci] = 0.0;
                }
            }
            events.push(SimEvent::Dropped {
                time: t,
                task_id: tasks[idx].id,
                reason: DropReason::NegativeLaxity,
            });
        }

        // ---- 3. Rank the ready queue by Relaxation R (ascending) ----
        let laxities: Vec<f64> = ready_queue.iter().map(|&idx| tasks[idx].laxity(t)).collect();
        let l_norms = normalize_laxities(&laxities);
        let mut ranked: Vec<(usize, f64)> = ready_queue
            .iter()
            .zip(l_norms.iter())
            .map(|(&idx, &ln)| (idx, relaxation(theta_val, ln, tasks[idx].priority)))
            .collect();
        ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        // Helper: is task index `idx` currently occupying *any* core?
        let is_running = |cores: &[Core], idx: usize| cores.iter().any(|c| c.running == Some(idx));

        // ---- 4. Preemption decisions, per busy core ----
        //
        // IMPLEMENTATION NOTE (flagged ambiguity #2 — preemption
        // inequality, see project plan): the spec states the
        // preemption condition as:
        //     Laxity_critical(new) > C + 2 * RemainingTime(running)
        // which reads unusually (a *large* candidate laxity triggering
        // preemption is counter-intuitive; normally urgency = *low*
        // laxity). We implement the inequality exactly as written in
        // `should_preempt` below, isolated so it can be flipped in one
        // place once confirmed with the TA.
        for core_idx in 0..cores.len() {
            if switch_overhead_remaining[core_idx] > 0.0 {
                continue; // mid-switch; not eligible for re-evaluation
            }
            let running_idx = match cores[core_idx].running {
                Some(idx) => idx,
                None => continue, // handled in the free-core pass below
            };
            // Best waiting (not currently running anywhere) candidate.
            let candidate = ranked.iter().find(|&&(idx, _)| !is_running(&cores, idx));
            if let Some(&(cand_idx, _)) = candidate {
                let cand_laxity = tasks[cand_idx].laxity(t);
                let running_remaining = tasks[running_idx].remaining_time;
                if should_preempt(cand_laxity, config.critical_coefficient, running_remaining) {
                    events.push(SimEvent::Preempted {
                        time: t,
                        task_id: tasks[running_idx].id,
                        core: core_idx,
                    });
                    cores[core_idx].running = Some(cand_idx);
                    if tasks[cand_idx].start_time.is_none() {
                        tasks[cand_idx].start_time = Some(t);
                    }
                    switch_overhead_remaining[core_idx] = config.context_switch_cost;
                    total_context_switches += 1;
                    tasks[cand_idx].context_switches_incurred += 1;
                    events.push(SimEvent::Started {
                        time: t,
                        task_id: tasks[cand_idx].id,
                        core: core_idx,
                        preemptive: true,
                    });
                }
            }
        }

        // ---- 5. Assign waiting tasks to any free cores ----
        for core_idx in 0..cores.len() {
            if switch_overhead_remaining[core_idx] > 0.0 || cores[core_idx].running.is_some() {
                continue;
            }
            if let Some(&(cand_idx, _)) = ranked.iter().find(|&&(idx, _)| !is_running(&cores, idx)) {
                cores[core_idx].running = Some(cand_idx);
                if tasks[cand_idx].start_time.is_none() {
                    tasks[cand_idx].start_time = Some(t);
                }
                // A fresh assignment to a previously-idle core still
                // incurs the context-switch cost per spec (loading
                // the task's context onto the core).
                switch_overhead_remaining[core_idx] = config.context_switch_cost;
                total_context_switches += 1;
                tasks[cand_idx].context_switches_incurred += 1;
                events.push(SimEvent::Started {
                    time: t,
                    task_id: tasks[cand_idx].id,
                    core: core_idx,
                    preemptive: false,
                });
            }
        }

        // ---- 6. Execute one tick of work on every busy, non-
        //         switching core ----
        for core_idx in 0..cores.len() {
            if switch_overhead_remaining[core_idx] > 0.0 {
                switch_overhead_remaining[core_idx] -= 1.0;
                continue;
            }
            if let Some(idx) = cores[core_idx].running {
                tasks[idx].remaining_time -= 1.0;
                if tasks[idx].remaining_time <= 1e-9 {
                    tasks[idx].completed = true;
                    tasks[idx].finish_time = Some(t + 1.0);
                    last_completion_time = (t + 1.0).max(last_completion_time);
                    events.push(SimEvent::Completed { time: t + 1.0, task_id: tasks[idx].id });
                    cores[core_idx].running = None;
                }
            }
        }

        // ---- 7. Drop completed/dropped tasks from the ready queue ----
        ready_queue.retain(|&idx| !tasks[idx].completed && !tasks[idx].dropped);

        t += 1.0;
    }

    let result = summarize(tasks, total_context_switches, last_completion_time);
    (events, result)
}

/// Isolates the preemption-severity test described in the spec so it
/// can be revisited/flipped in one place once confirmed with the TA.
///
/// As written in the source document:
///     Laxity_critical(new_task) > C + 2 * RemainingTime(running_task)
fn should_preempt(candidate_laxity: f64, critical_coefficient: f64, running_remaining: f64) -> bool {
    candidate_laxity > critical_coefficient + 2.0 * running_remaining
}

fn summarize(tasks: &[Task], total_context_switches: u32, last_completion_time: f64) -> SimResult {
    let total_tasks = tasks.len();
    let mut completed_on_time = 0usize;
    let mut dropped_count = 0usize;
    let mut deadline_miss_count = 0usize;
    let mut misses_by_priority = [0usize; 5];
    let mut tasks_by_priority = [0usize; 5];

    for tk in tasks {
        let p_idx = (tk.priority as usize).saturating_sub(1).min(4);
        tasks_by_priority[p_idx] += 1;

        if tk.dropped {
            dropped_count += 1;
            deadline_miss_count += 1;
            misses_by_priority[p_idx] += 1;
            continue;
        }
        match tk.finish_time {
            Some(f) if f <= tk.deadline + 1e-9 => {
                completed_on_time += 1;
            }
            Some(_) => {
                deadline_miss_count += 1; // finished, but late
                misses_by_priority[p_idx] += 1;
            }
            None => {
                // Never finished and never dropped — should not
                // normally happen given the horizon margin, but
                // counted as a miss defensively if it does.
                deadline_miss_count += 1;
                misses_by_priority[p_idx] += 1;
            }
        }
    }

    SimResult {
        total_tasks,
        completed_on_time,
        dropped_count,
        deadline_miss_count,
        total_context_switches,
        makespan: last_completion_time,
        misses_by_priority,
        tasks_by_priority,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::generate_tasks;
    use crate::task::Weather;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn simulation_terminates_and_accounts_for_every_task() {
        let mut rng = StdRng::seed_from_u64(123);
        let mut tasks = generate_tasks(&mut rng, 30, Weather::Sunny, 1.0);
        let config = SimConfig { num_cores: 2, weather: Weather::Sunny, tightness: 1.0, ..Default::default() };
        let (_events, result) = run_simulation(&mut tasks, &config);

        assert_eq!(result.total_tasks, 30);
        for tk in &tasks {
            assert!(tk.completed || tk.dropped, "task {} neither completed nor dropped", tk.id);
        }
        assert_eq!(result.completed_on_time + result.deadline_miss_count, 30);
    }

    #[test]
    fn runs_under_contention_without_panicking() {
        let mut rng = StdRng::seed_from_u64(99);
        let mut tasks = generate_tasks(&mut rng, 30, Weather::Snowy, 2.0);
        let config = SimConfig { num_cores: 2, weather: Weather::Snowy, tightness: 2.0, ..Default::default() };
        let (_events, result) = run_simulation(&mut tasks, &config);
        assert_eq!(result.total_tasks, 30);
    }

    #[test]
    fn four_core_config_runs() {
        let mut rng = StdRng::seed_from_u64(55);
        let mut tasks = generate_tasks(&mut rng, 30, Weather::Rainy, 1.5);
        let config = SimConfig { num_cores: 4, weather: Weather::Rainy, tightness: 1.5, ..Default::default() };
        let (_events, result) = run_simulation(&mut tasks, &config);
        assert_eq!(result.total_tasks, 30);
    }
}
