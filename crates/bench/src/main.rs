//! `shopcore-bench` CLI: `seed` builds the dataset, `run` measures the
//! scenarios and writes `results.json` + `results.md`.
//!
//! Usage:
//!   shopcore-bench seed --size <full|smoke|tiny> --db <path> [--seed <u64>]
//!   shopcore-bench run --db <path> --size <full|smoke> [--ui-results <path>]
//!     [--out <dir>] [--update-baseline] [--baseline <path>]
//!     [--artifacts-dir <dir>] [--target-pc]
//!
//! Exit codes: 0 = pass, 1 = usage/bench error, 2 = baseline regression.

use std::path::{Path, PathBuf};

use shopcore_bench::report::{Baseline, MachineRecord, Results};
use shopcore_bench::scenarios;
use shopcore_bench::{BenchError, schema, seed};

/// ISO-8601 UTC timestamp, computed from the Unix epoch without a date
/// library (civil-from-days algorithm).
fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let secs_of_day = secs % 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u64::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u64::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = if month <= 2 { y + 1 } else { y };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs_of_day / 3_600,
        (secs_of_day % 3_600) / 60,
        secs_of_day % 60
    )
}

/// Tiny flag parser: `--key value` and boolean `--flag`.
#[derive(Debug, Default)]
struct Args {
    /// Positional subcommand.
    cmd: String,
    /// Named values.
    values: Vec<(String, String)>,
    /// Boolean flags.
    flags: Vec<String>,
}

impl Args {
    fn parse() -> Self {
        let mut args = Args::default();
        let mut iter = std::env::args().skip(1).peekable();
        if let Some(cmd) = iter.next() {
            args.cmd = cmd;
        }
        while let Some(arg) = iter.next() {
            if let Some(key) = arg.strip_prefix("--") {
                if key.contains('=') {
                    let mut parts = key.splitn(2, '=');
                    let name = parts.next().unwrap_or("").to_string();
                    let value = parts.next().unwrap_or("").to_string();
                    args.values.push((name, value));
                } else if iter.peek().is_some_and(|next| !next.starts_with("--")) {
                    let value = iter.next().unwrap_or_default();
                    args.values.push((key.to_string(), value));
                } else {
                    args.flags.push(key.to_string());
                }
            }
        }
        args
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn has(&self, key: &str) -> bool {
        self.flags.iter().any(|f| f == key)
    }
}

fn dataset_size(name: &str) -> Result<seed::DatasetSize, BenchError> {
    match name {
        "full" => Ok(seed::DatasetSize::full()),
        "smoke" => Ok(seed::DatasetSize::smoke()),
        "tiny" => Ok(seed::DatasetSize::tiny()),
        other => Err(BenchError::Usage(format!(
            "unknown --size {other:?} (want full|smoke|tiny)"
        ))),
    }
}

fn cmd_seed(args: &Args) -> Result<(), BenchError> {
    let size = dataset_size(args.get("size").unwrap_or("full"))?;
    let db = args
        .get("db")
        .map_or_else(|| PathBuf::from("target/bench/bench.db"), PathBuf::from);
    let seed_value = args
        .get("seed")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(seed::DEFAULT_SEED);
    if let Some(parent) = db.parent() {
        std::fs::create_dir_all(parent).map_err(BenchError::Io)?;
    }
    if db.exists() {
        std::fs::remove_file(&db).map_err(BenchError::Io)?;
    }
    let mut conn = rusqlite::Connection::open(&db).map_err(BenchError::Sql)?;
    schema::create_schema(&conn)?;
    let stats = seed::seed(&mut conn, &size, seed_value)?;
    let db_bytes = std::fs::metadata(&db).map_or(0, |m| m.len());
    println!(
        "seeded {}: {} skus, {} barcodes, {} customers, {} sales, {} lines, {} movements in {} ms (db {} bytes)",
        db.display(),
        stats.skus,
        stats.barcodes,
        stats.customers,
        stats.sales,
        stats.sale_lines,
        stats.movements,
        stats.elapsed_ms,
        db_bytes
    );
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn cmd_run(args: &Args) -> Result<i32, BenchError> {
    let size_name = args.get("size").unwrap_or("full");
    let size = dataset_size(size_name)?;
    let db = args
        .get("db")
        .map_or_else(|| PathBuf::from("target/bench/bench.db"), PathBuf::from);
    if !db.exists() {
        return Err(BenchError::Usage(format!(
            "database {} does not exist — run `just seed-large` first",
            db.display()
        )));
    }
    let out_dir = args
        .get("out")
        .map_or_else(|| PathBuf::from("target/bench/results"), PathBuf::from);
    std::fs::create_dir_all(&out_dir).map_err(BenchError::Io)?;
    let ui = match args.get("ui-results") {
        Some(path) => Some(scenarios::load_ui_results(Path::new(path))?),
        None => None,
    };
    let artifacts_dir = args.get("artifacts-dir").map(PathBuf::from);
    // Unique per run so the bench sales never collide with a previous run's.
    let run_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0));

    // Warm the page cache so the measurements are not cold-cache noise.
    scenarios::warmup(&db, run_id)?;

    let mut results = Vec::new();
    results.extend(scenarios::scenario_barcode_lookup(&db, &size)?);
    results.extend(scenarios::scenario_name_search(&db, &size)?);
    results.extend(scenarios::scenario_keyset_pages(&db, &size)?);
    results.push(scenarios::scenario_sale_commit(&db, run_id)?);
    results.push(scenarios::scenario_sale_commit_under_load(&db, run_id)?);
    results.extend(scenarios::scenario_reports(&db)?);
    results.push(scenarios::scenario_startup(ui.as_ref()));
    results.extend(scenarios::scenario_ui(ui.as_ref()));
    results.push(scenarios::scenario_memory(&db));
    results.push(scenarios::scenario_installers(artifacts_dir.as_deref()));

    let provisional = !args.has("target-pc");
    let run = Results {
        generated_at: iso_now(),
        machine: MachineRecord::detect(),
        provisional,
        dataset: size_name.to_string(),
        scenarios: results,
    };
    let json_path = out_dir.join("results.json");
    let md_path = out_dir.join("results.md");
    std::fs::write(&json_path, run.to_json()?).map_err(BenchError::Io)?;
    std::fs::write(&md_path, run.to_markdown()).map_err(BenchError::Io)?;
    println!("wrote {} and {}", json_path.display(), md_path.display());

    // Smoke runs compare against the stored baseline (relative tripwire).
    if size_name == "smoke" {
        let baseline_path = args.get("baseline").map_or_else(
            || PathBuf::from("crates/bench/baseline/smoke-baseline.json"),
            PathBuf::from,
        );
        if args.has("update-baseline") {
            let baseline = Baseline::from_results(&run);
            baseline.save(&baseline_path)?;
            println!("baseline updated: {}", baseline_path.display());
            return Ok(0);
        }
        if baseline_path.exists() {
            let baseline = Baseline::load(&baseline_path)?;
            let violations = baseline.violations(&run);
            if violations.is_empty() {
                println!("baseline check passed (no metric over 5x baseline)");
                return Ok(0);
            }
            eprintln!("BASELINE REGRESSION (smoke tripwire, > 5x baseline):");
            for violation in &violations {
                eprintln!("  - {violation}");
            }
            return Ok(2);
        }
        println!(
            "no baseline at {} — run with --update-baseline to create one",
            baseline_path.display()
        );
    }
    Ok(0)
}

fn main() -> std::process::ExitCode {
    let args = Args::parse();
    match args.cmd.as_str() {
        "seed" => match cmd_seed(&args) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: {err}");
                std::process::ExitCode::from(1)
            }
        },
        "run" => match cmd_run(&args) {
            Ok(code) => std::process::ExitCode::from(u8::try_from(code).unwrap_or(1)),
            Err(err) => {
                eprintln!("error: {err}");
                std::process::ExitCode::from(1)
            }
        },
        other => {
            eprintln!("unknown command {other:?} (want seed|run)");
            std::process::ExitCode::from(1)
        }
    }
}
