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
cargo test                                  # runs unit tests in generator.rs and simulator.rs
cargo run --release                         # demo episode + Phase 1 sweep, all defaults (unchanged from before)
cargo run --release -- --help               # full flag reference
cargo run --release -- demo  [flags]        # just the demo episode, with your chosen algorithm/parameters
cargo run --release -- sweep [flags]        # just the Phase 1 sweep, with your chosen algorithm
```

Output: a per-task table printed to stdout for one demo episode, plus
`output/phase1_sweep.csv` containing aggregated metrics across a
sweep of {2,4} cores × {sunny,rainy,snowy} × 11 tightness points × 20
runs each (Phase 2 should bump this to 100 runs per the spec).

### Choosing an algorithm and parameters at the command line

Everything that used to be hardcoded in `main.rs`/`experiment.rs`
(which scheduler, how many cores, which weather, tightness,
utilization, context-switch cost, critical coefficient, RNG seed, task
count) is now a CLI flag with the old hardcoded value as its default —
so `cargo run --release` with no arguments still does exactly what it
did before this change.

```bash
# Demo episode: partitioned EDF, 4 cores, rainy weather, tighter deadlines
cargo run --release -- demo --scheduler partitioned-edf --cores 4 --weather rainy --tightness 1.5

# Same demo, heavier load and a different seed for a different random task mix
cargo run --release -- demo --utilization 3.0 --seed 7

# Sweep using partitioned EDF instead of the default global EDF, fewer runs for a quick check
cargo run --release -- sweep --scheduler partitioned-edf --runs 5
```

Full flag list is in `cargo run --release -- --help` (also reproduced
in `src/cli.rs`'s module doc comment).

**What changed to support this:** `Scheduler` implementations
(`GlobalEdf`, `PartitionedEdf`) are picked at compile time by default
in Rust generics, which doesn't work once "which scheduler" is a
string typed at the command line. `simulator::run_simulation` was
changed from generic (`<S: Scheduler>(..., scheduler: &mut S)`) to a
trait object (`scheduler: &mut dyn Scheduler`), and a
`SchedulerKind` enum (`core::scheduler::SchedulerKind`) maps
CLI-supplied names to a `Box<dyn Scheduler>` via `SchedulerKind::build()`.
Adding a new scheduler (e.g. the still-missing Relaxation-based one —
see "Flagged, not changed" below) means implementing `Scheduler` for
it and adding one match arm each to `SchedulerKind::parse`/`build`/`as_str`.

## Refactoring notes (this pass)

This pass removed redundancy/dead code without changing runtime
behavior, and separately flagged a couple of things that look like
real issues but are functional/experimental-design decisions, not
"redundancy" — so they were left in place with a comment rather than
silently changed:

**Removed / consolidated:**
- **`simulator.rs`**: dropped unused imports (`normalize_laxities`,
  `relaxation`, `theta` were imported but never referenced, now that
  scheduling is delegated to the `Scheduler` trait rather than done
  inline). `should_preempt` is dead code today — nothing calls it —
  but it encodes the spec's documented, unresolved preemption
  inequality (assumption 2 below). Kept and marked
  `#[allow(dead_code)]` with an explanatory comment rather than
  deleted, so that unresolved ambiguity isn't silently lost.
- **`SimConfig::for_sweep_point(num_cores, weather, tightness)`**
  added as a constructor. `main.rs` and `experiment.rs` were each
  building the full `SimConfig` struct literal by hand, repeating
  `context_switch_cost: 1.0, critical_coefficient: 5.0` verbatim in
  three separate places; all three now go through this one
  constructor, so there's a single place to change those defaults.
- **`experiment.rs`**: `run_config` was rebuilding an identical
  `SimConfig` on every one of the `runs_per_config` loop iterations
  (only the RNG seed and task set change per run) — it's now built
  once outside the loop.
- **`scheduler.rs`**: `PartitionedEdf`'s hand-written `impl Default`
  was equivalent to `#[derive(Default)]` (both its fields already
  implement `Default`) — replaced with the derive.
- **`main.rs`**: removed an unused `use crate::core::scheduler;` import
  (the `scheduler::*` glob import already covers it).
- **`visualize.py`**: the three near-identical `plot_metric(...)`
  calls in `main()` are now driven by one `METRICS` list instead of
  three copy-pasted call sites.

**Flagged, not changed:**
- `task.rs`'s `theta` / `normalize_laxities` / `relaxation` functions
  are never called by any `Scheduler` impl — only `GlobalEdf` and
  `PartitionedEdf` exist, and neither uses the Relaxation (R) metric.
  There is no `RelaxationScheduler` wired up yet, even though the
  Relaxation-based algorithm is what this project is named after and
  what this README describes as "the proposed" Phase 1 algorithm.
  That reads as a functionality gap, not something a redundancy pass
  should paper over by deleting the "unused" helper functions — they
  look like the building blocks for the scheduler that's still
  missing.
- `experiment.rs`'s Phase-1 sweep loops over 4 `target_utilization`
  values (`UTILIZATION_SWEEP_POINTS`) that aren't mentioned in this
  README's stated sweep shape (2 cores × 3 weathers × 11 tightness
  points × N runs) and aren't faceted on by `visualize.py`. Left in
  place — just pulled out of an inline literal into a named constant
  with a comment — but worth confirming with your TA whether that
  extra dimension belongs in Phase 1 output at all, since it silently
  quadruples the sweep's row count and run time.

As before: no Rust toolchain was available in the environment that
produced this refactor, so none of the above has actually been
compiled — please run `cargo build && cargo test` yourself first and
report back any errors.

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
   inequality direction is actually intended. (Note: as of this
   refactor pass, this function isn't called by the simulation loop —
   see "Refactoring notes" above.)

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
