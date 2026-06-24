# ADAS Hybrid Scheduler — Phase 1 (Rust)

Implementation of Phase 1 for Project 6 ("Hybrid Scheduling of Aperiodic
Tasks (ADAS) Under Variable Environmental Conditions"): the simulation
environment plus the proposed Relaxation-based scheduling algorithm.
Baselines (EDF-Global, MLLF) and the full 100-run statistical
comparison are Phase 2 work and are **not** included here.

## IMPORTANT — Not compiled or run in this environment

This code was written without access to a Rust toolchain or network
(the sandbox that produced it has no internet access and no
`cargo`/`rustc` installed, so `sh.rustup.rs` could not be reached to
install one). Every file was instead checked by hand: types, trait
bounds (`Copy`/`Clone`/`Serialize` derives), and borrow-checker shape
were traced through line by line. I'm confident in the logic, but you
should run `cargo build` and `cargo test` yourself as the first step
before relying on it — please report back any compiler errors and I
will fix them immediately.

## Build & run

```bash
cargo build --release
cargo test            # runs unit tests in generator.rs and simulator.rs
cargo run --release   # runs a demo episode + the Phase 1 sanity sweep
```

Output: a per-task table printed to stdout for one demo episode, plus
`output/phase1_sweep.csv` containing aggregated metrics across a
sweep of {2,4} cores × {sunny,rainy,snowy} × 11 tightness points × 20
runs each (Phase 2 should bump this to 100 runs per the spec).

## Project layout

| File | Purpose |
|---|---|
| `src/task.rs` | `Task` struct, TTC/deadline physics, Laxity, theta, Relaxation (R) metric |
| `src/generator.rs` | Random task batch generation per the spec's uniform distributions |
| `src/simulator.rs` | Discrete-time scheduling loop: admission, drop, ranking, preemption, execution |
| `src/experiment.rs` | Sweep runner producing averaged CSV output |
| `src/main.rs` | CLI entry point: demo episode + sanity sweep |

## Documented assumptions (the spec is ambiguous on these — confirm with your TA)

The source project document leaves a few details unspecified. Rather
than silently guessing, each is implemented behind an isolated,
clearly-commented piece of code so it can be changed in one place.
**I'd treat resolving these as the first task before you trust any
output for your report.**

1. **How `Deadline Tightness` (range [0.5, 2.46]) enters the TTC
   formula.** Not given explicitly in the spec. Implemented in
   `task.rs::effective_distance()` as `d_effective = d / tightness`
   (higher tightness → shorter effective distance → smaller TTC →
   tighter deadline). This is our convention, not the spec's — if your
   TA specifies a different mapping (e.g. scaling `a` or the final TTC
   directly), change only this one function.

2. **The preemption inequality.** The spec states:
   `Laxity_critical(new) > C + 2 * RemainingTime(running)`. This reads
   unusually — normally urgency is signaled by *low/negative* laxity,
   not a large value exceeding some threshold. Implemented exactly as
   written in `simulator.rs::should_preempt()`, isolated in one
   function so the direction/semantics can be corrected in one place
   once you confirm with the TA which task's laxity and which
   inequality direction is actually intended.

3. **The "critical coefficient" `C`.** Not given a concrete value
   anywhere in the spec. Currently a configurable field
   `SimConfig::critical_coefficient`, defaulted to `5.0` — arbitrary,
   needs a real value or a derivation rule from your TA.

4. **TTC domain-validity fallback.** If
   `v0² - 2·a·(1-λ)·d_effective < 0`, the square root is undefined
   (physically: no collision is possible under the given braking
   assumptions). We treat this as "no immediate collision risk" and
   fall back to a constant `FALLBACK_TTC = 50.0` ticks
   (`task.rs::FALLBACK_TTC`) rather than producing an unusable infinite
   deadline. This is a clamping choice, not a spec value.

5. **Multi-core task assignment.** The spec describes one ranked
   ready queue but not how multiple cores draw from it. We use a
   global-queue convention (same spirit as "EDF-Global"): free cores
   pull the lowest-R waiting task each tick; a busy core's incumbent is
   swapped only if the preemption condition fires for that specific
   core. No task migrates mid-execution between cores.

6. **Context-switch charging on fresh (non-preemptive) assignment.**
   The spec only explicitly calls out the 1-unit context-switch cost
   in connection with `Context Switch` overhead generally; we charge
   it on *every* assignment to a core (including the very first task
   placed on a previously-idle core), not just preemptive swaps. If
   your TA intends idle→busy transitions to be free, remove the charge
   in the "Assign waiting tasks to free cores" block of
   `simulator.rs`.

## What Phase 1 deliberately does *not* include

- EDF-Global and MLLF baseline algorithms (Phase 2).
- The full 100-run statistical comparison and the 5 final charts
  (Phase 2). The sweep machinery in `experiment.rs` is already
  generic enough that Phase 2 should only need to add baseline
  scheduler variants, not restructure the simulator.
- Combined/multi-weather scenario charts (Phase 2 — "weather impact"
  chart needs a fixed Tightness value across all three conditions,
  which `experiment.rs`'s sweep already produces; Phase 2 just needs
  to slice and plot it).
