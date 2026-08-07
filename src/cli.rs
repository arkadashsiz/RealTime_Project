// src/cli.rs

use crate::core::generator::DEFAULT_TASKS_PER_ROUND;
use crate::core::scheduler::SchedulerKind;
use crate::core::simulator::SimConfig;
use crate::core::task::Weather;

#[derive(Debug, Clone)]
pub struct RunParams {
    pub scheduler: SchedulerKind,
    pub num_cores: usize,
    pub weather: Weather,
    pub tightness: f64,
    pub target_utilization: f64,
    pub context_switch_cost: f64,
    pub critical_coefficient: f64,
    pub task_count: usize,
}

impl Default for RunParams {
    fn default() -> Self {
        RunParams {
            scheduler: SchedulerKind::default(),
            num_cores: 2,
            weather: Weather::Sunny,
            tightness: 1.0,
            target_utilization: 1.0,
            context_switch_cost: 1.0,
            critical_coefficient: 5.0,
            task_count: DEFAULT_TASKS_PER_ROUND,
        }
    }
}

impl RunParams {
    pub fn to_sim_config(&self) -> SimConfig {
        SimConfig {
            num_cores: self.num_cores,
            weather: self.weather,
            tightness: self.tightness,
            context_switch_cost: self.context_switch_cost,
            critical_coefficient: self.critical_coefficient,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SweepParams {
    pub scheduler: SchedulerKind,
    pub runs_per_config: usize,
}

impl Default for SweepParams {
    fn default() -> Self {
        SweepParams {
            scheduler: SchedulerKind::default(),
            runs_per_config: 20,
        }
    }
}

pub enum Command {
    Help,
    Demo(RunParams),
    Sweep(SweepParams),
    Both(RunParams, SweepParams),
}

pub fn parse_args() -> Result<Command, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        return Ok(Command::Both(RunParams::default(), SweepParams::default()));
    }

    match args[0].as_str() {
        "-h" | "--help" => Ok(Command::Help),
        "demo" => Ok(Command::Demo(parse_run_params(&args[1..])?)),
        "sweep" => Ok(Command::Sweep(parse_sweep_params(&args[1..])?)),
        other => Err(format!(
            "unknown subcommand '{other}' (expected 'demo', 'sweep', or no subcommand to run both; try --help)"
        )),
    }
}

fn next_flag_value<'a>(args: &'a [String], i: usize) -> Result<&'a str, String> {
    args.get(i + 1).map(String::as_str).ok_or_else(|| format!("flag '{}' expects a value", args[i]))
}

fn parse_run_params(args: &[String]) -> Result<RunParams, String> {
    let mut p = RunParams::default();
    let mut i = 0;
    while i < args.len() {
        let value = next_flag_value(args, i)?;
        match args[i].as_str() {
            "--scheduler" => p.scheduler = SchedulerKind::parse(value)?,
            "--cores" => p.num_cores = parse_num(value, "--cores")?,
            "--weather" => p.weather = parse_weather(value)?,
            "--tightness" => p.tightness = parse_num(value, "--tightness")?,
            "--utilization" => p.target_utilization = parse_num(value, "--utilization")?,
            "--context-switch-cost" => p.context_switch_cost = parse_num(value, "--context-switch-cost")?,
            "--critical-coefficient" => p.critical_coefficient = parse_num(value, "--critical-coefficient")?,
            "--tasks" => p.task_count = parse_num(value, "--tasks")?,
            other => return Err(format!("unknown flag '{other}' for 'demo' (try --help)")),
        }
        i += 2;
    }
    Ok(p)
}

fn parse_sweep_params(args: &[String]) -> Result<SweepParams, String> {
    let mut p = SweepParams::default();
    let mut i = 0;
    while i < args.len() {
        let value = next_flag_value(args, i)?;
        match args[i].as_str() {
            "--scheduler" => p.scheduler = SchedulerKind::parse(value)?,
            "--runs" => p.runs_per_config = parse_num(value, "--runs")?,
            other => return Err(format!("unknown flag '{other}' for 'sweep' (try --help)")),
        }
        i += 2;
    }
    Ok(p)
}

fn parse_num<T: std::str::FromStr>(value: &str, flag: &str) -> Result<T, String> {
    value.parse::<T>().map_err(|_| format!("invalid value '{value}' for {flag}"))
}

fn parse_weather(s: &str) -> Result<Weather, String> {
    match s.to_ascii_lowercase().as_str() {
        "sunny" => Ok(Weather::Sunny),
        "rainy" => Ok(Weather::Rainy),
        "snowy" => Ok(Weather::Snowy),
        other => Err(format!("unknown weather '{other}' (expected sunny|rainy|snowy)")),
    }
}

pub fn print_usage() {
    println!(
        "ADAS Hybrid Scheduler — Phase 1

USAGE:
    cargo run --release                         Run the demo episode, then the Phase 1 sweep (all defaults)
    cargo run --release -- demo  [flags]        Run only the single demo episode
    cargo run --release -- sweep [flags]        Run only the Phase 1 sweep
    cargo run --release -- --help               Show this message

DEMO flags:
    --scheduler <global-edf|partitioned-edf|mllf|proposed|env-proposed> (default: global-edf)
    --cores <N>                                 (default: 2)
    --weather <sunny|rainy|snowy>               (default: sunny)
    --tightness <F>                             (default: 1.0, spec range [0.5, 2.46])
    --utilization <F>                           (default: 1.0)
    --context-switch-cost <F>                   (default: 1.0)
    --critical-coefficient <F>                  (default: 5.0)
    --seed <N>                                  (default: 42)
    --tasks <N>                                 (default: {DEFAULT_TASKS_PER_ROUND})

SWEEP flags:
    --scheduler <global-edf|partitioned-edf|mllf|proposed|env-proposed> (default: global-edf; applied to every swept configuration)
    --runs <N>                                  (episodes averaged per configuration; default: 20)

EXAMPLES:
    cargo run --release -- demo --scheduler env-proposed --cores 4 --weather rainy --tightness 1.5
    cargo run --release -- sweep --scheduler env-proposed --runs 20
"
    );
}