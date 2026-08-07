from pathlib import Path
import matplotlib.pyplot as plt
import pandas as pd

OUTPUT_DIR = Path("output")
IMAGE_BASE_DIR = Path("image")

METRICS = [
    (
        "avg_deadline_miss_ratio",
        "Deadline Miss Ratio",
        "Scheduler Reliability vs Deadline Tightness",
    ),
    (
        "avg_makespan",
        "Average Makespan",
        "Execution Time vs Deadline Tightness",
    ),
    (
        "avg_dropped_ratio",
        "Dropped Task Ratio",
        "Task Dropping Behavior vs Deadline Tightness",
    ),
    (
        "avg_context_switches_per_task",
        "Avg Context Switches / Task",
        "Context Switch Overhead vs Deadline Tightness",
    ),
]

SCHEDULER_STYLES = {
    "global-edf": {"marker": "o", "linestyle": "-"},
    "partitioned-edf": {"marker": "s", "linestyle": "--"},
    "mllf": {"marker": "^", "linestyle": "-."},
    "proposed": {"marker": "D", "linestyle": "-"},
    "env-proposed": {"marker": "v", "linestyle": "--"},
    "env": {"marker": "v", "linestyle": "--"},
}

PRIORITY_COLS = ["misses_p1", "misses_p2", "misses_p3", "misses_p4", "misses_p5"]
PRIORITY_COLORS = ["#d7191c", "#fdae61", "#ffffbf", "#abd9e9", "#2c7bb6"]  # P1 (Critical/Red) to P5 (Low/Blue)


def load_all_algorithm_data() -> pd.DataFrame:
    """Loads all phase1_sweep_*.csv files from the output directory and combines them."""
    csv_files = list(OUTPUT_DIR.glob("phase1_sweep_*.csv"))

    if not csv_files:
        fallback = OUTPUT_DIR / "phase1_sweep.csv"
        if fallback.exists():
            csv_files = [fallback]
        else:
            raise FileNotFoundError(
                f"No CSV files matching 'phase1_sweep_*.csv' found in '{OUTPUT_DIR}'."
            )

    dataframes = []
    for filepath in csv_files:
        df = pd.read_csv(filepath)

        if "scheduler" not in df.columns:
            sched_name = filepath.stem.replace("phase1_sweep_", "")
            df["scheduler"] = sched_name

        dataframes.append(df)

    combined_df = pd.concat(dataframes, ignore_index=True)
    return combined_df


def plot_inter_algorithm_comparison(
    df: pd.DataFrame, utilization: float, metric: str, ylabel: str, title: str, save_dir: Path
):
    """Plots line charts comparing algorithms for a metric across tightness levels."""
    util_df = df[abs(df["utilization"] - utilization) < 1e-5]

    if util_df.empty:
        return

    cores_list = sorted(util_df["num_cores"].unique())
    weather_list = sorted(util_df["weather"].unique())
    schedulers = sorted(util_df["scheduler"].unique())

    fig, axes = plt.subplots(
        nrows=len(cores_list),
        ncols=len(weather_list),
        figsize=(16, 5 * len(cores_list)),
        sharex=True,
        sharey=(metric != "avg_makespan"),
    )

    if len(cores_list) == 1:
        axes = [axes]
    if len(weather_list) == 1:
        axes = [[ax] for ax in axes]

    for r, cores in enumerate(cores_list):
        for c, weather in enumerate(weather_list):
            ax = axes[r][c]

            for sched in schedulers:
                subset = util_df[
                    (util_df["scheduler"] == sched)
                    & (util_df["num_cores"] == cores)
                    & (util_df["weather"] == weather)
                ].sort_values("tightness")

                if subset.empty or metric not in subset.columns:
                    continue

                style = SCHEDULER_STYLES.get(sched, {"marker": "o", "linestyle": "-"})

                ax.plot(
                    subset["tightness"],
                    subset[metric],
                    label=sched.upper(),
                    marker=style["marker"],
                    linestyle=style["linestyle"],
                    linewidth=1.8,
                    markersize=5,
                )

            ax.set_title(f"{cores} Cores | Weather: {weather.capitalize()}", fontsize=11)
            ax.set_xlabel("Deadline Tightness")
            ax.set_ylabel(ylabel)
            ax.grid(True, alpha=0.3)

            if r == 0 and c == 0:
                ax.legend(loc="best", fontsize=9)

    fig.suptitle(
        f"{title} (Utilization: {utilization})\n(Inter-Algorithm Performance Comparison)",
        fontsize=14,
        y=0.99,
    )
    plt.tight_layout()

    save_path = save_dir / f"comparison_{metric}.png"
    plt.savefig(save_path, dpi=300, bbox_inches="tight")
    plt.close()


def plot_priority_miss_stacked_bars(df: pd.DataFrame, utilization: float, save_dir: Path):
    """Generates Stacked Bar Charts showing deadline misses broken down by priority levels (P1 to P5)."""
    util_df = df[abs(df["utilization"] - utilization) < 1e-5]

    if util_df.empty or not all(col in util_df.columns for col in PRIORITY_COLS):
        return

    cores_list = sorted(util_df["num_cores"].unique())
    weather_list = sorted(util_df["weather"].unique())
    schedulers = sorted(util_df["scheduler"].unique())

    fig, axes = plt.subplots(
        nrows=len(cores_list),
        ncols=len(weather_list),
        figsize=(18, 6 * len(cores_list)),
        sharey=True,
    )

    if len(cores_list) == 1:
        axes = [axes]
    if len(weather_list) == 1:
        axes = [[ax] for ax in axes]

    for r, cores in enumerate(cores_list):
        for c, weather in enumerate(weather_list):
            ax = axes[r][c]

            # Aggregate total misses by priority across tightness points per scheduler
            grouped = (
                util_df[(util_df["num_cores"] == cores) & (util_df["weather"] == weather)]
                .groupby("scheduler")[PRIORITY_COLS]
                .sum()
                .reindex(schedulers)
                .fillna(0)
            )

            bottom = pd.Series(0.0, index=grouped.index)

            for idx, p_col in enumerate(PRIORITY_COLS):
                values = grouped[p_col]
                ax.bar(
                    grouped.index,
                    values,
                    bottom=bottom,
                    label=f"Priority P{idx + 1}",
                    color=PRIORITY_COLORS[idx],
                    edgecolor="black",
                    linewidth=0.5,
                    width=0.5,
                )
                bottom += values

            ax.set_title(f"{cores} Cores | Weather: {weather.capitalize()}", fontsize=11)
            ax.set_ylabel("Total Priority Misses")
            ax.set_xticklabels([s.upper() for s in grouped.index], rotation=15)
            ax.grid(True, axis="y", alpha=0.3)

            if r == 0 and c == 0:
                ax.legend(title="Priority Level", loc="upper left", fontsize=9)

    fig.suptitle(
        f"Priority Miss Analysis (Utilization: {utilization})\n(Stacked Bar Distribution: P1 Critical to P5 Low Priority)",
        fontsize=14,
        y=0.99,
    )
    plt.tight_layout()

    save_path = save_dir / "priority_miss_analysis_stacked.png"
    plt.savefig(save_path, dpi=300, bbox_inches="tight")
    plt.close()


def main():
    try:
        df = load_all_algorithm_data()
    except FileNotFoundError as e:
        print(f"Error: {e}")
        return

    if "utilization" not in df.columns:
        print("Error: 'utilization' column missing from CSV files.")
        return

    utilizations = sorted(df["utilization"].unique())
    print(f"Loaded data for algorithms: {df['scheduler'].unique().tolist()}")
    print(f"Found utilization sweep points: {utilizations}\n")

    for util in utilizations:
        util_dir = IMAGE_BASE_DIR / f"utilization_{util}"
        util_dir.mkdir(parents=True, exist_ok=True)

        print(f"Generating plots for Utilization = {util} -> Output folder: {util_dir}/")

        # Standard metric comparisons
        for metric, ylabel, title in METRICS:
            plot_inter_algorithm_comparison(df, util, metric, ylabel, title, util_dir)

        # Priority Miss Analysis (Stacked Bar Charts)
        plot_priority_miss_stacked_bars(df, util, util_dir)

    print(f"\nAll plots, including Priority Miss Stacked Bar Charts, have been saved to '{IMAGE_BASE_DIR}/'.")


if __name__ == "__main__":
    main()