use crate::core::task::{normalize_laxities, relaxation, theta, Task, Weather};
use crate::core::scheduler::{Scheduler, CoreView};
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

pub fn run_simulation<S: Scheduler>(
    tasks: &mut [Task], 
    config: &SimConfig, 
    scheduler: &mut S
) -> (Vec<SimEvent>, SimResult) {
    let mut events = Vec::new();
    let mut cores: Vec<Core> = (0..config.num_cores).map(|_| Core { running: None }).collect();
    
    // Setup logic remains identical...
    let mut arrival_order: Vec<usize> = (0..tasks.len()).collect();
    arrival_order.sort_by(|&a, &b| tasks[a].arrival_time.partial_cmp(&tasks[b].arrival_time).unwrap());
    let mut next_arrival_ptr = 0usize;
    let mut ready_queue: Vec<usize> = Vec::new();
    let mut t: f64 = 0.0;
    let max_deadline = tasks.iter().fold(0.0_f64, |acc, tk| acc.max(tk.deadline));
    let horizon = max_deadline + 50.0;
    let mut total_context_switches: u32 = 0;
    let mut last_completion_time: f64 = 0.0;
    let mut switch_overhead_remaining: Vec<f64> = vec![0.0; config.num_cores];

    loop {
        if t >= horizon { break; }

        // ---- 1. Admit newly arrived tasks ----
        while next_arrival_ptr < arrival_order.len()
            && tasks[arrival_order[next_arrival_ptr]].arrival_time <= t
        {
            let idx = arrival_order[next_arrival_ptr];
            ready_queue.push(idx);
            
            // NEW: Let the scheduler know a task arrived
            scheduler.on_task_arrival(idx, tasks, config);
            
            next_arrival_ptr += 1;
        }

        // Termination check
        if next_arrival_ptr >= arrival_order.len() && ready_queue.is_empty() && cores.iter().all(|c| c.running.is_none()) {
            break;
        }

        // ---- 2. Drop negative laxity (identical to before) ----
        let mut newly_dropped = Vec::new();
        ready_queue.retain(|&idx| {
            if tasks[idx].laxity(t) < 0.0 {
                newly_dropped.push(idx);
                false
            } else { true }
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
            events.push(SimEvent::Dropped { time: t, task_id: tasks[idx].id, reason: DropReason::NegativeLaxity });
        }

        // ---- 3. Ask Scheduler for Core Decisions ----
        let core_views: Vec<CoreView> = cores.iter().enumerate().map(|(i, c)| CoreView {
            running_task: c.running,
            is_switching: switch_overhead_remaining[i] > 0.0,
        }).collect();

        let desired_states = scheduler.schedule(&ready_queue, &core_views, tasks, t, config);

        // ---- 4. Apply Scheduler Decisions ----
        for core_idx in 0..cores.len() {
            if switch_overhead_remaining[core_idx] > 0.0 {
                continue; // Cannot alter a core mid-switch
            }

            let current = cores[core_idx].running;
            let desired = desired_states[core_idx];

            if current != desired {
                // A change was requested by the scheduler

                // 1. Preempt if something was already running
                if let Some(running_idx) = current {
                    events.push(SimEvent::Preempted {
                        time: t,
                        task_id: tasks[running_idx].id,
                        core: core_idx,
                    });
                }

                // 2. Set new state
                cores[core_idx].running = desired;

                // 3. Start new task and incur overhead
                if let Some(new_idx) = desired {
                    if tasks[new_idx].start_time.is_none() {
                        tasks[new_idx].start_time = Some(t);
                    }
                    switch_overhead_remaining[core_idx] = config.context_switch_cost;
                    total_context_switches += 1;
                    tasks[new_idx].context_switches_incurred += 1;
                    
                    events.push(SimEvent::Started {
                        time: t,
                        task_id: tasks[new_idx].id,
                        core: core_idx,
                        preemptive: current.is_some(), // It's a preemption if the core wasn't idle
                    });
                }
            }
        }

        // ---- 5. Execute one tick (identical to before) ----
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

        // ---- 6. Clean ready queue (identical to before) ----
        ready_queue.retain(|&idx| !tasks[idx].completed && !tasks[idx].dropped);
        
        t += 1.0;
    }

    let result = summarize(tasks, total_context_switches, last_completion_time);
    (events, result)
}

fn should_preempt(candidate_laxity: f64, critical_coefficient: f64, running_remaining: f64) -> bool {
    candidate_laxity < critical_coefficient + 2.0 * running_remaining
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
