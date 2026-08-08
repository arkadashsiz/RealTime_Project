mod cli;
mod core;

use cli::{Command, RunParams, SweepParams};
use core::generator::generate_tasks;
use core::simulator::run_simulation;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::path::Path;
use std::fs::File;
use std::io::Write;
fn main() {
    match cli::parse_args() {
        Ok(Command::Help) => cli::print_usage(),
        Ok(Command::Demo(params)) => {
            println!("=== ADAS Hybrid Scheduler — Phase 1 ===\n");
            run_single_demo_episode(&params);
        }
        Ok(Command::Sweep(params)) => {
            println!("=== ADAS Hybrid Scheduler — Phase 1 ===\n");
            run_sanity_sweep(&params);
        }
        Ok(Command::Generate(params)) => {
            println!("=== ADAS Hybrid Scheduler — Phase 1 ===\n");
            run_generate_taskset_csv(&params);
        }
        Ok(Command::Both(demo_params, sweep_params)) => {
            println!("=== ADAS Hybrid Scheduler — Phase 1 ===\n");
            run_single_demo_episode(&demo_params);
            println!();
            run_sanity_sweep(&sweep_params);
        }
        Err(e) => {
            eprintln!("error: {e}\n");
            cli::print_usage();
            std::process::exit(1);
        }
    }
}
fn run_single_demo_episode(params: &RunParams) {
    println!(
        "--- Demo episode: {} cores, {} weather, tightness = {:.2}, scheduler = {} ---",
        params.num_cores,
        params.weather.as_str(),
        params.tightness,
        params.scheduler.as_str()
    );

    let mut rng = StdRng::from_entropy();
    let mut tasks = generate_tasks(
        &mut rng,
        params.task_count,
        params.weather,
        params.tightness,
        params.target_utilization,
    );

    let config = params.to_sim_config();
    let mut scheduler = params.scheduler.build();
    let (_events, result) = run_simulation(&mut tasks, &config, scheduler.as_mut());

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

fn run_sanity_sweep(params: &SweepParams) {
    println!(
        "--- Phase 1 sanity sweep: 2/4 cores x {{sunny,rainy,snowy}} x 11 tightness points, {} runs each, scheduler = {} ---",
        params.runs_per_config,
        params.scheduler.as_str()
    );

    let output_dir = Path::new("output");
    if !output_dir.exists() {
        std::fs::create_dir_all(output_dir).expect("failed to create output directory");
    }
    
    // Dynamically inject the scheduler name into the CSV filename
    let file_name = format!("phase1_sweep_{}.csv", params.scheduler.as_str());
    let output_path = output_dir.join(file_name);

    match core::experiment::run_phase1_sanity_sweep(&output_path, params.runs_per_config, params.scheduler) {
        Ok(rows) => {
            println!("Wrote {} aggregate rows to {}", rows.len(), output_path.display());
            println!("\nSample rows:");
            for row in rows.iter().take(5) {
                println!(
                    "  cores={} weather={:<6} tightness={:.2} -> DMR={:.3} avg_cs/task={:.2} avg_makespan={:.2}",
                    row.num_cores, row.weather, row.tightness, row.avg_deadline_miss_ratio, row.avg_context_switches_per_task, row.avg_makespan
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
fn sanity_check_monotonic_trend(rows: &[core::experiment::AggregateRow]) {
    use std::collections::HashMap;
    let mut grouped: HashMap<(usize, String), Vec<(f64, f64)>> = HashMap::new();
    for row in rows {
        grouped.entry((row.num_cores, row.weather.clone())).or_default().push((row.tightness, row.avg_deadline_miss_ratio));
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



fn run_generate_taskset_csv(params: &RunParams) {
    let output_dir = Path::new("output");
    if !output_dir.exists() {
        std::fs::create_dir_all(output_dir).expect("failed to create output directory");
    }

    let file_name = format!(
        "taskset_{}_{}_t{:.2}.csv",
        params.task_count,
        params.weather.as_str(),
        params.tightness
    );
    let output_path = output_dir.join(file_name);

    println!(
        "--- Generating Task Set CSV: {} tasks, {} weather, tightness = {:.2} ---",
        params.task_count,
        params.weather.as_str(),
        params.tightness
    );

    let mut rng = StdRng::from_entropy();
    let tasks = generate_tasks(
        &mut rng,
        params.task_count,
        params.weather,
        params.tightness,
        params.target_utilization,
    );

    let mut file = File::create(&output_path).expect("Failed to create CSV file");
    
    // Write CSV header
    writeln!(file, "id,arrival_time,execution_time,priority,obstacle_distance,deadline")
        .expect("Failed to write CSV header");

    // Write task rows
    for tk in tasks {
        writeln!(
            file,
            "{},{:.3},{:.3},{},{:.3},{:.3}",
            tk.id,
            tk.arrival_time,
            tk.execution_time,
            tk.priority,
            tk.obstacle_distance,
            tk.deadline
        ).expect("Failed to write CSV row");
    }

    println!("Success! Wrote task set to {}\n", output_path.display());
}