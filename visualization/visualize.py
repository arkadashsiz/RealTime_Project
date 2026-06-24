import pandas as pd
import matplotlib.pyplot as plt
from pathlib import Path

CSV_FILE = Path("../output/phase1_sweep.csv")


def load_data():
    return pd.read_csv(CSV_FILE)


def plot_metric(df, metric, ylabel, title):
    plt.figure(figsize=(10, 6))

    for cores in sorted(df["num_cores"].unique()):
        for weather in sorted(df["weather"].unique()):

            subset = df[
                (df["num_cores"] == cores)
                & (df["weather"] == weather)
            ].sort_values("tightness")

            plt.plot(
                subset["tightness"],
                subset[metric],
                marker="o",
                label=f"{cores} cores - {weather}",
            )

    plt.xlabel("Deadline Tightness")
    plt.ylabel(ylabel)
    plt.title(title)
    plt.grid(True, alpha=0.3)
    plt.legend()
    plt.tight_layout()


def main():
    df = load_data()

    plot_metric(
        df,
        "avg_deadline_miss_ratio",
        "Deadline Miss Ratio",
        "Scheduler Reliability vs Deadline Tightness",
    )

    plot_metric(
        df,
        "avg_makespan",
        "Average Makespan",
        "Execution Time vs Deadline Tightness",
    )

    plot_metric(
        df,
        "avg_dropped_ratio",
        "Dropped Task Ratio",
        "Task Dropping Behavior vs Deadline Tightness",
    )

    plt.show()


if __name__ == "__main__":
    main()