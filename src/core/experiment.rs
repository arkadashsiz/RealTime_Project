
use crate::core::generator::{generate_tasks, DEFAULT_TASKS_PER_ROUND};
use crate::core::simulator::{run_simulation, SimConfig, SimResult};
use crate::core::task::Weather;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::error::Error;
use std::fs::File;
use std::path::Path;

/// One row of aggregate output: the configuration plus that
/// configuration's averaged metrics over `runs_per_config` episodes.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AggregateRow {
    pub num_cores: usize,
    pub weather: String,
    pub tightness: f64,
    pub runs: usize,
    pub avg_deadline_miss_ratio: f64,
    pub avg_context_switches_per_task: f64,
    pub avg_makespan: f64,
    pub avg_dropped_ratio: f64,
    pub misses_p1: f64,
    pub misses_p2: f64,
    pub misses_p3: f64,
    pub misses_p4: f64,
    pub misses_p5: f64,
}

/// Tightness sweep points spanning the spec's [0.5, 2.46] range.
/// 11 points gives smooth-enough curves for Phase 2 plotting while
/// keeping Phase 1 sanity runs fast.
pub fn tightness_sweep_points() -> Vec<f64> {
    let start = 0.5_f64;
    let end = 2.46_f64;
    let steps = 10;
    (0..=steps)
        .map(|i| start + (end - start) * (i as f64) / (steps as f64))
        .collect()
}

/// Runs `runs_per_config` independent simulation episodes for a given
/// (num_cores, weather, tightness) configuration and returns the
/// averaged `AggregateRow`.
pub fn run_config(
    num_cores: usize,
    weather: Weather,
    tightness: f64,
    runs_per_config: usize,
    base_seed: u64,
) -> AggregateRow {
    let mut total_dmr = 0.0;
    let mut total_cs_per_task = 0.0;
    let mut total_makespan = 0.0;
    let mut total_dropped_ratio = 0.0;
    let mut misses_by_priority_sum = [0.0_f64; 5];

    for run_idx in 0..runs_per_config {
        let seed = base_seed.wrapping_add(run_idx as u64);
        let mut rng = StdRng::seed_from_u64(seed);
        let mut tasks = generate_tasks(&mut rng, DEFAULT_TASKS_PER_ROUND, weather, tightness);

        let config = SimConfig {
            num_cores,
            weather,
            tightness,
            context_switch_cost: 1.0,
            critical_coefficient: 5.0,
        };

        let (_events, result) = run_simulation(&mut tasks, &config);
        accumulate(&result, &mut total_dmr, &mut total_cs_per_task, &mut total_makespan, &mut total_dropped_ratio, &mut misses_by_priority_sum);
    }

    let n = runs_per_config as f64;
    AggregateRow {
        num_cores,
        weather: weather.as_str().to_string(),
        tightness,
        runs: runs_per_config,
        avg_deadline_miss_ratio: total_dmr / n,
        avg_context_switches_per_task: total_cs_per_task / n,
        avg_makespan: total_makespan / n,
        avg_dropped_ratio: total_dropped_ratio / n,
        misses_p1: misses_by_priority_sum[0] / n,
        misses_p2: misses_by_priority_sum[1] / n,
        misses_p3: misses_by_priority_sum[2] / n,
        misses_p4: misses_by_priority_sum[3] / n,
        misses_p5: misses_by_priority_sum[4] / n,
    }
}

fn accumulate(
    result: &SimResult,
    total_dmr: &mut f64,
    total_cs_per_task: &mut f64,
    total_makespan: &mut f64,
    total_dropped_ratio: &mut f64,
    misses_by_priority_sum: &mut [f64; 5],
) {
    let n = result.total_tasks as f64;
    *total_dmr += result.deadline_miss_count as f64 / n;
    *total_cs_per_task += result.total_context_switches as f64 / n;
    *total_makespan += result.makespan;
    *total_dropped_ratio += result.dropped_count as f64 / n;
    for i in 0..5 {
        misses_by_priority_sum[i] += result.misses_by_priority[i] as f64;
    }
}

/// Phase-1 sanity sweep: for each of {2,4} cores, each weather
/// condition, and each tightness sweep point, run `runs_per_config`
/// episodes and collect aggregate rows. Writes the result to a CSV
/// at `output_path`.
pub fn run_phase1_sanity_sweep(
    output_path: &Path,
    runs_per_config: usize,
) -> Result<Vec<AggregateRow>, Box<dyn Error>> {
    let mut rows = Vec::new();
    let core_counts = [2usize, 4usize];
    let tightness_points = tightness_sweep_points();

    let mut seed_counter: u64 = 1000;
    for &cores in &core_counts {
        for weather in Weather::all() {
            for &tightness in &tightness_points {
                let row = run_config(cores, weather, tightness, runs_per_config, seed_counter);
                seed_counter = seed_counter.wrapping_add(10_000);
                rows.push(row);
            }
        }
    }

    write_csv(output_path, &rows)?;
    Ok(rows)
}

fn write_csv(path: &Path, rows: &[AggregateRow]) -> Result<(), Box<dyn Error>> {
    let file = File::create(path)?;
    let mut wtr = csv::Writer::from_writer(file);
    for row in rows {
        wtr.serialize(row)?;
    }
    wtr.flush()?;
    Ok(())
}
