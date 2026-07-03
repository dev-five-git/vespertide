//! Synthetic workload harness for `cargo flamegraph`. Generates a
//! deterministic 100-table workspace and drives `vespertide-lsp` through
//! its hot paths so the profiler captures representative samples.
//!
//! Supports machine-readable JSON output and baseline comparison:
//! - No flags: human-readable summary
//! - `--json <path>`: write JSON to path
//! - `--baseline <path>`: compare against prior JSON
//! - `--workload <synthetic|realistic>`: choose request cadence

mod fixture;
mod workload;

use anyhow::Result;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
enum WorkloadMode {
    Synthetic,
    Realistic,
}

struct CliArgs {
    json_path: Option<String>,
    baseline_path: Option<String>,
    workload_mode: WorkloadMode,
}

impl WorkloadMode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "synthetic" => Some(Self::Synthetic),
            "realistic" => Some(Self::Realistic),
            _ => None,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Synthetic => "synthetic",
            Self::Realistic => "realistic",
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let cli = parse_args(&args);

    let started = std::time::Instant::now();
    let scenario = fixture::build_workspace(100)?;
    println!("running workload: {}", cli.workload_mode.as_str());
    let timings = match cli.workload_mode {
        WorkloadMode::Synthetic => workload::run(&scenario)?,
        WorkloadMode::Realistic => workload::run_realistic(&scenario)?,
    };
    let total_secs = started.elapsed().as_secs_f64();

    print_summary(&timings, total_secs);
    if let Some(path) = &cli.json_path {
        write_json(path, &timings)?;
    }
    if let Some(path) = &cli.baseline_path {
        compare_baseline(path, &timings)?;
    }

    Ok(())
}

fn parse_args(args: &[String]) -> CliArgs {
    let mut json_path = None;
    let mut baseline_path = None;
    let mut workload_mode = WorkloadMode::Synthetic;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => {
                if i + 1 < args.len() {
                    json_path = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("--json requires a path argument");
                    std::process::exit(1);
                }
            }
            "--baseline" => {
                if i + 1 < args.len() {
                    baseline_path = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("--baseline requires a path argument");
                    std::process::exit(1);
                }
            }
            "--workload" => {
                if i + 1 < args.len() {
                    let value = &args[i + 1];
                    let Some(mode) = WorkloadMode::parse(value) else {
                        eprintln!("unknown workload: {value}");
                        std::process::exit(1);
                    };
                    workload_mode = mode;
                    i += 2;
                } else {
                    eprintln!("--workload requires synthetic or realistic");
                    std::process::exit(1);
                }
            }
            _ => {
                eprintln!("unknown flag: {}", args[i]);
                std::process::exit(1);
            }
        }
    }

    CliArgs {
        json_path,
        baseline_path,
        workload_mode,
    }
}

fn print_summary(timings: &[workload::PhaseTiming], total_secs: f64) {
    for timing in timings {
        println!(
            "phase {} ({} × {}):    {:.2}s  ({} items)",
            phase_number(&timing.name),
            timing.name,
            timing.calls,
            timing.wall_secs,
            timing.items
        );
        if let Some(latency) = &timing.latency {
            print_latency(latency);
        }
    }
    println!("total: {total_secs:.2}s");
}

fn write_json(path: &str, timings: &[workload::PhaseTiming]) -> Result<()> {
    let json = serde_json::to_string_pretty(timings)?;
    fs::write(path, json)?;
    Ok(())
}

fn compare_baseline(path: &str, timings: &[workload::PhaseTiming]) -> Result<()> {
    if !Path::new(path).exists() {
        eprintln!("baseline file not found: {path}");
        std::process::exit(1);
    }
    let baseline_json = fs::read_to_string(path)?;
    let baseline: Vec<workload::PhaseTiming> = serde_json::from_str(&baseline_json)?;

    println!(
        "\n{:<30} {:<12} {:<12} {:<10}",
        "phase", "baseline", "current", "delta"
    );
    println!("{}", "-".repeat(64));

    let mut baseline_total = 0.0;
    let mut current_total = 0.0;

    for (baseline_timing, current_timing) in baseline.iter().zip(timings.iter()) {
        baseline_total += baseline_timing.wall_secs;
        current_total += current_timing.wall_secs;
        print_timing_delta(baseline_timing, current_timing);
    }

    println!("{}", "-".repeat(64));
    println!(
        "{:<30} {:<12} {:<12} {:+.1}%",
        "total",
        format_secs(baseline_total),
        format_secs(current_total),
        delta_percent(baseline_total, current_total)
    );
    Ok(())
}

fn print_timing_delta(
    baseline_timing: &workload::PhaseTiming,
    current_timing: &workload::PhaseTiming,
) {
    println!(
        "{:<30} {:<12} {:<12} {:+.1}%",
        format!("{} ({})", baseline_timing.name, baseline_timing.calls),
        format_secs(baseline_timing.wall_secs),
        format_secs(current_timing.wall_secs),
        delta_percent(baseline_timing.wall_secs, current_timing.wall_secs)
    );

    if let (Some(baseline_latency), Some(current_latency)) =
        (&baseline_timing.latency, &current_timing.latency)
    {
        print_latency_delta("  p50:", baseline_latency.p50_us, current_latency.p50_us);
        print_latency_delta("  p95:", baseline_latency.p95_us, current_latency.p95_us);
        print_latency_delta("  p99:", baseline_latency.p99_us, current_latency.p99_us);
    }
}

fn phase_number(name: &str) -> &'static str {
    match name {
        "diagnostics" => "1",
        "completion" => "2",
        "semantic_tokens" => "3",
        "workspace_symbols" => "4",
        "drift" => "5",
        "did_change_to_first_request" => "6",
        _ => "?",
    }
}

fn print_latency(latency: &workload::LatencyStats) {
    println!(
        "  latency: p50={:.0}μs p95={:.0}μs p99={:.0}μs max={:.0}μs \
         (min={:.0}μs mean={:.0}μs n={})",
        latency.p50_us,
        latency.p95_us,
        latency.p99_us,
        latency.max_us,
        latency.min_us,
        latency.mean_us,
        latency.samples
    );
}

fn print_latency_delta(label: &str, baseline_us: f64, current_us: f64) {
    println!(
        "{:<30} {:<12} {:<12} {:+.1}%",
        label,
        format_us(baseline_us),
        format_us(current_us),
        delta_percent(baseline_us, current_us)
    );
}

fn format_secs(secs: f64) -> String {
    format!("{secs:.2}s")
}

fn format_us(us: f64) -> String {
    format!("{us:.0}μs")
}

fn delta_percent(baseline: f64, current: f64) -> f64 {
    if baseline > 0.0 {
        ((current - baseline) / baseline) * 100.0
    } else {
        0.0
    }
}
