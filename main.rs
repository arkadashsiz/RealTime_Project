//! main.rs
//!
//! Phase 1 entry point.
//!
//! Running `cargo run --release` will:
//!   1. Generate one batch of 30 tasks and run a single demo episode
//!      (2-core, sunny weather, tightness=1.0), printing a per-task
//!      summary table to stdout so you can eyeball correctness.
//!   2. Run the Phase 1 sanity sweep across {2,4} cores x 3 weather
//!      conditions x 11 tightness points, 20 episodes each (kept
//!      small for Phase 1; bump to 100 in Phase 2's full experiment),
//!      and write aggregate results to `output/phase1_sweep.csv`.
//!
//! Run `cargo test` to execute the unit tests in `generator.rs` and
//! `simulator.rs`.

mod experiment;
mod generator;
mod simulator;
mod task;

use generator::{generate_tasks, DEFAULT_TASKS_PER_ROUND};
use rand::rngs::StdRng;
use rand::SeedableRng;
use simulator::{run_simulation, SimConfig};
use std::path::Path;
use task::Weather;

fn main() {
    println!("=== ADAS Hybrid Scheduler — Phase 1 ===\n");

    run_single_demo_episode();
    println!();
    run_sanity_sweep();
}

fn run_single_demo_episode() {
    println!("--- Demo episode: 2 cores, sunny weather, tightness = 1.0 ---");

    let mut rng = StdRng::seed_from_u64(42);
    let weather = Weather::Sunny;
    let tightness = 1.0;
    let mut tasks = generate_tasks(&mut rng, DEFAULT_TASKS_PER_ROUND, weather, tightness);

    let config = SimConfig {
        num_cores: 2,
        weather,
        tightness,
        context_switch_cost: 1.0,
        critical_coefficient: 5.0,
    };

    let (_events, result) = run_simulation(&mut tasks, &config);

    println!(
        "{:<4} {:>8} {:>8} {:>4} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "id", "arrival", "exec", "pri", "obs_d(m)", "deadline", "start", "finish", "status"
    );
    let mut sorted_tasks = tasks.clone();
    sorted_tasks.sort_by(|a, b| a.id.cmp(&b.id));
    for tk in &sorted_tasks {
        let status = if tk.dropped {
            "DROPPED".to_string()
        } else if let Some(f) = tk.finish_time {
            if f <= tk.deadline + 1e-9 {
                "ON-TIME".to_string()
            } else {
                "LATE".to_string()
            }
        } else {
            "UNFINISHED".to_string()
        };
        println!(
            "{:<4} {:>8.2} {:>8.2} {:>4} {:>9.1} {:>9.2} {:>9} {:>9} {:>9}",
            tk.id,
            tk.arrival_time,
            tk.execution_time,
            tk.priority,
            tk.obstacle_distance,
            tk.deadline,
            tk.start_time.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "-".to_string()),
            tk.finish_time.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "-".to_string()),
            status
        );
    }

    println!("\nSummary:");
    println!("  total tasks:            {}", result.total_tasks);
    println!("  completed on time:      {}", result.completed_on_time);
    println!("  dropped:                {}", result.dropped_count);
    println!("  deadline misses (total):{}", result.deadline_miss_count);
    println!("  context switches:       {}", result.total_context_switches);
    println!("  makespan:               {:.2}", result.makespan);
    println!("  misses by priority (P1..P5): {:?}", result.misses_by_priority);
}

fn run_sanity_sweep() {
    println!("--- Phase 1 sanity sweep: 2/4 cores x {{sunny,rainy,snowy}} x 11 tightness points, 20 runs each ---");

    let output_dir = Path::new("output");
    if !output_dir.exists() {
        std::fs::create_dir_all(output_dir).expect("failed to create output directory");
    }
    let output_path = output_dir.join("phase1_sweep.csv");

    match experiment::run_phase1_sanity_sweep(&output_path, 20) {
        Ok(rows) => {
            println!("Wrote {} aggregate rows to {}", rows.len(), output_path.display());
            println!("\nSample rows:");
            for row in rows.iter().take(5) {
                println!(
                    "  cores={} weather={:<6} tightness={:.2} -> DMR={:.3} avg_cs/task={:.2} avg_makespan={:.2}",
                    row.num_cores,
                    row.weather,
                    row.tightness,
                    row.avg_deadline_miss_ratio,
                    row.avg_context_switches_per_task,
                    row.avg_makespan
                );
            }

            // Basic, eyeball-able sanity assertion: DMR should
            // generally trend upward as tightness increases (tighter
            // deadlines => more misses). We don't hard-fail on this in
            // Phase 1 since the Tightness-to-TTC mapping is a
            // documented assumption (see task.rs), but we surface a
            // warning if it clearly does NOT hold, since that would
            // suggest a bug rather than just sampling noise.
            sanity_check_monotonic_trend(&rows);
        }
        Err(e) => {
            eprintln!("Sweep failed: {}", e);
        }
    }
}

fn sanity_check_monotonic_trend(rows: &[experiment::AggregateRow]) {
    use std::collections::HashMap;
    let mut grouped: HashMap<(usize, String), Vec<(f64, f64)>> = HashMap::new();
    for row in rows {
        grouped
            .entry((row.num_cores, row.weather.clone()))
            .or_default()
            .push((row.tightness, row.avg_deadline_miss_ratio));
    }
    for ((cores, weather), mut series) in grouped {
        series.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let first_dmr = series.first().unwrap().1;
        let last_dmr = series.last().unwrap().1;
        if last_dmr + 1e-9 < first_dmr {
            println!(
                "  [warning] cores={} weather={}: DMR decreased from {:.3} (loosest) to {:.3} (tightest) — \
                 worth double-checking the tightness->deadline mapping in task.rs",
                cores, weather, first_dmr, last_dmr
            );
        }
    }
}
