//! `shopcore-bench` CLI: `seed` builds the dataset, `run` measures the
//! scenarios and writes `results.json` + `results.md`, `verify` proves
//! a seeded database (checksum + per-SKU balance check).
//!
//! Usage:
//!   shopcore-bench seed --size <full|smoke|tiny> --db <path> [--seed <u64>] [--fresh]
//!     (resumes an interrupted seed by default; --fresh deletes and starts over)
//!   shopcore-bench run --db <path> --size <full|smoke> [--ui-results <path>]
//!     [--out <dir>] [--update-baseline] [--baseline <path>]
//!     [--artifacts-dir <dir>] [--target-pc]
//!   shopcore-bench verify --db <path>
//!   shopcore-bench merge-ui --results-dir <dir> --ui-results <path>
//!     (adds the debug-route UI results, scenarios 7-8, to an existing
//!      results.json / results.md; run the debug route in the app first)
//!
//! Exit codes: 0 = pass, 1 = usage/bench error, 2 = baseline regression.

use std::path::{Path, PathBuf};

use shopcore_bench::report::{Baseline, MachineRecord, Results, ScenarioResult};
use shopcore_bench::scenarios;
use shopcore_bench::{BenchError, real_schema, seed};

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

/// Estimated disk space a seed needs (database + WAL + headroom).
/// Measured 2026-11-10: full = 2.3 GB database, smoke = 42 MB.
fn required_bytes(size: &seed::DatasetSize) -> u64 {
    if size.skus >= 500_000 {
        4_000_000_000 // ~4 GB: 2.3 GB database + WAL + headroom
    } else if size.skus >= 10_000 {
        200_000_000 // ~200 MB
    } else {
        20_000_000 // ~20 MB
    }
}

/// Free disk space (bytes) on the filesystem holding `path`, or `None`
/// if it cannot be determined. Linux: `df`. Windows: PowerShell
/// `Get-PSDrive` on the path's drive letter.
fn free_space_bytes(path: &Path) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let dir = path.parent().map_or(".", |p| p.to_str().unwrap_or("."));
        let output = std::process::Command::new("df")
            .args(["-k", "--output=avail", dir])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let kb: u64 = text.lines().nth(1)?.trim().parse().ok()?;
        Some(kb * 1024)
    }
    #[cfg(target_os = "windows")]
    {
        let drive = path.components().next().map_or_else(
            || "C:".to_string(),
            |c| c.as_os_str().to_string_lossy().into_owned(),
        );
        let output = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("(Get-PSDrive {drive}).Free"),
            ])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        text.parse::<u64>().ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = path;
        None
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
    // Free disk space: print what the seed needs and refuse to start
    // if there is not enough room (a seed that runs out of disk
    // mid-way leaves a corrupt partial database).
    let need = required_bytes(&size);
    println!(
        "seed needs ~{} MB free disk space for {}",
        need / 1_000_000,
        db.display()
    );
    match free_space_bytes(&db) {
        Some(free) => {
            println!("free disk space: {} MB", free / 1_000_000);
            if free < need {
                return Err(BenchError::Usage(format!(
                    "not enough free disk space: need ~{} MB, have {} MB",
                    need / 1_000_000,
                    free / 1_000_000
                )));
            }
        }
        None => println!("free disk space: unknown (could not determine)"),
    }
    // Resume by default: if the database already exists, the seeder
    // continues from the last committed batch instead of starting
    // over. `--fresh` deletes it and seeds from scratch.
    let fresh = args.has("fresh");
    if db.exists() && fresh {
        // Delete the WAL sidecar files too: a stale -wal from a killed
        // run must not survive next to a fresh database file.
        std::fs::remove_file(&db).map_err(BenchError::Io)?;
        let wal = PathBuf::from(format!("{}-wal", db.display()));
        if wal.exists() {
            std::fs::remove_file(&wal).map_err(BenchError::Io)?;
        }
        let shm = PathBuf::from(format!("{}-shm", db.display()));
        if shm.exists() {
            std::fs::remove_file(&shm).map_err(BenchError::Io)?;
        }
    }
    let mut conn = rusqlite::Connection::open(&db).map_err(BenchError::Sql)?;
    real_schema::create_schema(&mut conn)?;
    let stats = seed::seed(&mut conn, &size, seed_value)?;
    let db_bytes = std::fs::metadata(&db).map_or(0, |m| m.len());
    println!(
        "seeded {}: {} skus, {} barcodes, {} sales, {} lines, {} movements in {} ms (db {} bytes)",
        db.display(),
        stats.skus,
        stats.barcodes,
        stats.sales,
        stats.sale_lines,
        stats.movements,
        stats.elapsed_ms,
        db_bytes
    );
    // Deterministic checksum of the dataset (counts + sums). A fresh
    // seed with the same seed value must print the same string; the
    // smoke checksum is recorded in the `smoke_seed_checksum` test.
    let sum = seed::checksum(&conn)?;
    println!("checksum: {sum}");
    Ok(())
}

/// Verify a seeded database: print the checksum, check sale totals
/// equal line totals, and check every per-SKU balance equals the sum
/// of that SKU's movements. Exit 0 only if every check passes.
fn cmd_verify(args: &Args) -> Result<i32, BenchError> {
    let db = args
        .get("db")
        .map_or_else(|| PathBuf::from("target/bench/bench.db"), PathBuf::from);
    if !db.exists() {
        return Err(BenchError::Usage(format!(
            "database {} does not exist — run `just seed-large` first",
            db.display()
        )));
    }
    let conn = scenarios::open_db(&db)?;
    let report = seed::verify(&conn)?;
    println!("checksum: {}", report.checksum);
    println!("sale_sum == line_sum: {}", report.sale_line_sums_match);
    println!(
        "balance mismatches (per-SKU balance != sum of movements): {}",
        report.balance_mismatches
    );
    println!(
        "movements without a balance row: {}",
        report.movements_without_balance
    );
    if report.sale_line_sums_match
        && report.balance_mismatches == 0
        && report.movements_without_balance == 0
    {
        println!("verify: OK");
        Ok(0)
    } else {
        eprintln!("verify: FAILED — see the counts above");
        Ok(1)
    }
}

/// Merge debug-route UI results (scenarios 7-8) into an existing
/// `results.json` / `results.md` pair, produced by a `run` without
/// `--ui-results`. The UI file is the `shopcore-ui-results.json` the
/// debug route (Ctrl+Shift+B) writes to the OS temp folder.
fn cmd_merge_ui(args: &Args) -> Result<i32, BenchError> {
    let results_dir = args
        .get("results-dir")
        .map_or_else(|| PathBuf::from("target/bench/results"), PathBuf::from);
    let ui_path = args.get("ui-results").map_or_else(
        || {
            Err(BenchError::Usage(
                "merge-ui needs --ui-results <path to shopcore-ui-results.json>".to_string(),
            ))
        },
        |p| Ok(PathBuf::from(p)),
    )?;
    let ui = scenarios::load_ui_results(&ui_path)?;
    let json_path = results_dir.join("results.json");
    let mut run = Results::load(&json_path)?;
    let mut merged: Vec<ScenarioResult> = Vec::with_capacity(run.scenarios.len());
    let mut ui_inserted = false;
    for scenario in std::mem::take(&mut run.scenarios) {
        if scenario.name == "7 startup" {
            merged.push(scenarios::scenario_startup(Some(&ui)));
        } else if scenario.name.starts_with("8 UI") {
            if !ui_inserted {
                merged.extend(scenarios::scenario_ui(Some(&ui)));
                ui_inserted = true;
            }
            // Drop the old (unmeasured or stale) "8 UI ..." rows.
        } else {
            merged.push(scenario);
        }
    }
    if !ui_inserted {
        // No UI rows existed at all (older results file): append them.
        merged.push(scenarios::scenario_startup(Some(&ui)));
        merged.extend(scenarios::scenario_ui(Some(&ui)));
    }
    run.scenarios = merged;
    let md_path = results_dir.join("results.md");
    std::fs::write(&json_path, run.to_json()?).map_err(BenchError::Io)?;
    std::fs::write(&md_path, run.to_markdown()).map_err(BenchError::Io)?;
    println!(
        "merged UI results (startup {:.0} ms, scroll p95 {:.2} ms, {} long tasks) into {}",
        ui.startup_ms,
        ui.scroll_p95_ms,
        ui.long_tasks,
        json_path.display()
    );
    Ok(0)
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
        "verify" => match cmd_verify(&args) {
            Ok(code) => std::process::ExitCode::from(u8::try_from(code).unwrap_or(1)),
            Err(err) => {
                eprintln!("error: {err}");
                std::process::ExitCode::from(1)
            }
        },
        "merge-ui" => match cmd_merge_ui(&args) {
            Ok(code) => std::process::ExitCode::from(u8::try_from(code).unwrap_or(1)),
            Err(err) => {
                eprintln!("error: {err}");
                std::process::ExitCode::from(1)
            }
        },
        other => {
            eprintln!("unknown command {other:?} (want seed|run|verify|merge-ui)");
            std::process::ExitCode::from(1)
        }
    }
}
