# ADAS Hybrid Scheduler — Hybrid Priority-Laxity Scheduling for Real-Time Aperiodic Tasks

> Implementation of the **relaxation-based hybrid scheduling algorithm** proposed in:
>
> Seyfipoor, M., Jaffry, S. M., & Mohammadi, S. (2025). *A Hybrid Priority-Laxity-Based Scheduling Algorithm for Real-Time Aperiodic Tasks Under Varying Environmental Conditions.* IEEE Access, 13, 173035–173051. [https://doi.org/10.1109/ACCESS.2025.3612340](https://doi.org/10.1109/ACCESS.2025.3612340)

**Course:** Real-Time Systems (Spring 2026), Sharif University of Technology — Ms. Sepideh Safari
---

## Table of Contents

- [Background](#background)
- [Algorithm Overview](#algorithm-overview)
- [Task Model](#task-model)
- [Project Structure](#project-structure)
- [Prerequisites](#prerequisites)
- [Building & Running](#building--running)
- [CLI Reference](#cli-reference)
- [Output](#output)
- [Known Assumptions & Open Questions](#known-assumptions--open-questions)
- [What Phase 1 Does Not Include](#what-phase-1-does-not-include)
- [Reference](#reference)

---

## Background

Advanced Driver Assistance Systems (ADAS) must process sensor data and react to road events with extremely low latency. Unlike periodic workloads, ADAS tasks — triggered by detected objects, weather changes, and proximity events — arrive aperiodically with unpredictable timing and execution requirements.

Classical schedulers like EDF prioritize purely by deadline and MLLF purely by laxity, neither of which accounts for the user-assigned importance of a task or the current environmental conditions (rain, snow, fog). This matters: pedestrian detection during a rainstorm should outrank lane-keeping, even if both have equal deadlines.

This project implements the **relaxation-based hybrid scheduler** from Seyfipoor et al. (2025), which combines three factors into a single prioritization metric:

- **Laxity** — how long a task can wait before missing its deadline (normalized and TTC-derived)
- **User-assigned priority** — the inherent importance of the task type
- **Environmental weight θ(λ)** — a weather-dependent coefficient that shifts the balance between urgency and criticality

The result is a scheduler that reduces the deadline miss rate by ~6% versus EDF and MLLF, while also lowering the proportion of *high-priority* missed deadlines.

---

## Algorithm Overview

The core metric is the **relaxation score** `R`:

```
R_i(λ) = θ(λ) · L_norm(i) + P_i(λ)
```

where:

| Symbol | Meaning |
|--------|---------|
| `L_norm(i)` | Laxity of task τᵢ, normalized to the priority scale: `((Lᵢ - L_min) / (L_max - L_min)) × Range(P)` |
| `P_i(λ)` | User-assigned priority, adjusted for current weather conditions λ |
| `θ(λ)` | Environment-dependent weighting coefficient (derived from rain/tire-slip/visibility sensors) |

Tasks are ranked by lowest `R` first (most urgent). Two safety margins prevent priority inversion from near-ties:

1. **`ε_R` tie-band** — tasks with very similar relaxation scores are treated as equivalent; TTC is used as the tie-breaker.
2. **`δ_TTC` margin** — only TTC differences large enough to be meaningful trigger a reorder.

**Preemption** only fires when the candidate task's laxity is less than the remaining execution time of the running task plus context-switch overhead — avoiding the excessive preemption characteristic of plain LLF.

**Deadlines** are derived from TTC (Time-to-Collision), incorporating weather-adjusted braking physics:

```
TTC = (v₀ ± √(v₀² − 2a(1−λ)d)) / (a(1−λ))
```

where `λ` increases with road wetness, reducing maximum deceleration and thus shortening TTC.

---

## Task Model

Each aperiodic task τᵢ is characterized by:

| Parameter | Meaning |
|-----------|---------|
| `a_i` | Arrival time |
| `c_i` | Execution time (capacity) |
| `d_i` | Deadline (derived from TTC) |
| `p_i` | User-assigned priority (P1 = highest, P5 = lowest) |

The scheduler targets four objectives simultaneously:
1. Minimize total deadline misses
2. Reduce the proportion of high-priority misses relative to total misses
3. Limit preemption overhead
4. Adapt priorities dynamically to environmental conditions

---

## Project Structure

```
RealTime_Project/
├── Cargo.toml
├── Cargo.lock
├── src/
│   ├── task.rs          # Task struct, TTC/deadline physics, laxity, θ, relaxation metric R
│   ├── generator.rs     # Random aperiodic task batch generation
│   ├── simulator.rs     # Discrete-time scheduling loop: admission, drop, ranking, preemption
│   ├── experiment.rs    # Sweep runner producing averaged CSV output
│   ├── cli.rs           # CLI flag definitions
│   └── main.rs          # Entry point: demo episode + Phase 1 sweep
├── visualization/       # Python plotting scripts for sweep output
├── report/              # Course report files
├── Project-phase1-report.pdf
└── Project-phase1-report.docx
```

---

## Prerequisites

- [Rust toolchain](https://rustup.rs/) (stable, 1.75+)
- Python 3 with `matplotlib` and `pandas` (for visualization only)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

---

## Building & Running

```bash
git clone https://github.com/arkadashsiz/RealTime_Project.git
cd RealTime_Project

cargo build --release
cargo test                   # unit tests in generator.rs and simulator.rs
cargo run --release          # demo episode + Phase 1 sweep (all defaults)
```

---

## CLI Reference

```
cargo run --release -- --help          # full flag list

# Subcommands
cargo run --release -- demo  [flags]   # single demo episode
cargo run --release -- sweep [flags]   # Phase 1 sweep → output/phase1_sweep.csv
```

### Common flags

| Flag | Default | Description |
|------|---------|-------------|
| `--scheduler` | `relaxation` | `relaxation`, `global-edf`, `partitioned-edf` |
| `--cores` | `2` | Number of processor cores |
| `--weather` | `sunny` | `sunny`, `rainy`, `snowy` |
| `--tightness` | `1.0` | Deadline tightness (range 0.5–2.46) |
| `--utilization` | `1.5` | Target task set utilization |
| `--seed` | `42` | RNG seed for reproducibility |
| `--runs` | `20` | Runs per sweep configuration |

### Examples

```bash
# Demo: 4-core rainy-weather run with tighter deadlines
cargo run --release -- demo --scheduler relaxation --cores 4 --weather rainy --tightness 1.5

# Quick sweep sanity check
cargo run --release -- sweep --runs 5

# Reproduce a specific random task mix
cargo run --release -- demo --utilization 3.0 --seed 7
```

---

## Output

**Demo episode** — per-task table printed to stdout showing arrival, deadline, finish time, and miss/hit status.

**Sweep** — `output/phase1_sweep.csv` with averaged metrics across:
- 2 core counts: {2, 4}
- 3 weather conditions: {sunny, rainy, snowy}
- 11 deadline tightness points in [0.5, 2.46]
- 20 runs per configuration

Metrics reported per configuration: deadline miss rate (DMR), average context-switch count per task, makespan, worst-case response time (WCRT), and E(Priority) of missed tasks.

### Visualization

```bash
cd visualization
pip install matplotlib pandas
python visualize.py   # reads output/phase1_sweep.csv
```

---

## Known Assumptions & Open Questions

Several details are left unspecified in the project document. Each is isolated in a clearly-commented section of the code so it can be corrected in one place. **Confirm these with your TA before relying on output for your report.**

1. **Deadline tightness → TTC mapping.** Implemented as `d_effective = d / tightness` in `task.rs::effective_distance()`. If your TA specifies a different mapping, change only this function.

2. **Preemption inequality direction.** The spec states `Laxity_critical(new) > C + 2 × RemainingTime(running)`. Implemented as written but isolated in `simulator.rs::should_preempt()` — the direction may be inverted from standard laxity semantics.

3. **Critical coefficient `C`.** Not given a concrete value in the spec. Currently defaults to `5.0` via `SimConfig::critical_coefficient`.

4. **TTC domain fallback.** If `v₀² − 2a(1−λ)d < 0` (no physically feasible collision), TTC falls back to `FALLBACK_TTC = 50.0` ticks.

5. **Multi-core assignment policy.** Uses a global-queue convention: free cores pull the lowest-R waiting task each tick; a task never migrates mid-execution.

6. **Context-switch cost on fresh assignment.** The 1-unit cost is charged on every core assignment, including idle→busy transitions — not only on preemptive swaps.

---


## Reference

```bibtex
@article{seyfipoor2025hybrid,
  title   = {A Hybrid Priority-Laxity-Based Scheduling Algorithm for Real-Time Aperiodic Tasks Under Varying Environmental Conditions},
  author  = {Seyfipoor, Mahdi and Jaffry, Sayyed Muhammad and Mohammadi, Siamak},
  journal = {IEEE Access},
  volume  = {13},
  pages   = {173035--173051},
  year    = {2025},
  doi     = {10.1109/ACCESS.2025.3612340}
}
```

The authors' original simulation code is available at: [github.com/1Jaffry1/Real-time-scheduling-for-aperiodic-tasks](https://github.com/1Jaffry1/Real-time-scheduling-for-aperiodic-tasks)
