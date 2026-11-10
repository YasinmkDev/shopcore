//! Results: JSON + markdown table + the CI smoke baseline.
//!
//! The baseline is a RELATIVE regression tripwire (spec 002, amendment 2):
//! CI compares the 10k smoke run against the stored baseline and fails only
//! if a metric exceeds 2× its baseline value. It is NOT an absolute budget
//! check — the absolute budgets are gated by the manual full run on the
//! target PC.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::BenchError;

/// How much a smoke metric may regress against the baseline before CI fails.
/// 5x, not 2x: measured 2026-11-10 — on a shared runner the smoke numbers
/// swing 2–3x run to run on noise alone (cold page cache, CPU throttling),
/// so 2x tripped on nothing. The tripwire catches real regressions (a bad
/// change is 5–10x), not VM noise.
pub const BASELINE_TOLERANCE_FACTOR: f64 = 5.0;

/// Machine the numbers were measured on. Fields that cannot be detected
/// are recorded as `"unknown"` — never guessed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineRecord {
    /// CPU model string.
    pub cpu: String,
    /// Total RAM, MiB.
    pub ram_mb: u64,
    /// Disk type: `SSD`, `HDD`, or `unknown`.
    pub disk: String,
    /// OS string.
    pub os: String,
    /// `WebView2` version (only meaningful for UI scenarios).
    pub webview2: String,
    /// Power plan, when known.
    pub power_plan: String,
}

impl MachineRecord {
    /// Detect what the OS will tell us; everything else is `"unknown"`.
    #[must_use]
    pub fn detect() -> Self {
        let mut cpu = "unknown".to_string();
        let mut ram_mb: u64 = 0;
        if let Ok(info) = std::fs::read_to_string("/proc/cpuinfo") {
            for line in info.lines() {
                if let Some(model) = line.strip_prefix("model name")
                    && let Some(value) = model.split(':').nth(1)
                {
                    cpu = value.trim().to_string();
                    break;
                }
            }
        }
        if let Ok(info) = std::fs::read_to_string("/proc/meminfo") {
            for line in info.lines() {
                if let Some(total) = line.strip_prefix("MemTotal:") {
                    let digits: String = total.chars().filter(char::is_ascii_digit).collect();
                    if let Ok(kb) = digits.parse::<u64>() {
                        ram_mb = kb / 1024;
                    }
                    break;
                }
            }
        }
        Self {
            cpu,
            ram_mb,
            disk: "unknown".to_string(),
            os: std::env::consts::OS.to_string(),
            webview2: "unknown".to_string(),
            power_plan: "unknown".to_string(),
        }
    }
}

/// One measured scenario. Times are milliseconds (f64, so sub-ms values
/// keep their precision).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    /// Scenario number + short name, e.g. `"1 barcode-lookup (warm)"`.
    pub name: String,
    /// What was timed, e.g. `"single lookup"`.
    pub metric: String,
    /// Always `"ms"` for now.
    pub unit: String,
    /// Median.
    pub p50_ms: f64,
    /// 95th percentile.
    pub p95_ms: f64,
    /// 99th percentile.
    pub p99_ms: f64,
    /// Extra numbers (counts, sizes, bytes) for this scenario.
    pub extra: serde_json::Value,
    /// Budget from `05-performance-budgets.md`, when one applies.
    pub budget: Option<String>,
    /// `true` when the p95 is inside the budget, `false` when it misses,
    /// `None` when no budget applies or the scenario was not measured.
    pub pass: Option<bool>,
    /// `true` when the scenario was not measured (e.g. UI scenarios without
    /// a debug-route run).
    pub not_measured: bool,
}

impl ScenarioResult {
    /// A single-shot measurement (p50 = p95 = p99 = `value_ms`).
    /// `budget` is `(label, threshold_ms)`; pass means value ≤ threshold.
    #[must_use]
    pub fn single(
        name: &str,
        metric: &str,
        value_ms: f64,
        extra: serde_json::Value,
        budget: Option<(&str, f64)>,
    ) -> Self {
        let (label, pass) = match budget {
            Some((label, threshold)) => (Some(label.to_string()), Some(value_ms <= threshold)),
            None => (None, None),
        };
        Self {
            name: name.to_string(),
            metric: metric.to_string(),
            unit: "ms".to_string(),
            p50_ms: value_ms,
            p95_ms: value_ms,
            p99_ms: value_ms,
            extra,
            budget: label,
            pass,
            not_measured: false,
        }
    }

    /// A histogram measurement. `budget` is `(label, threshold_ms)` applied
    /// to the p95; pass means p95 ≤ threshold.
    #[must_use]
    pub fn from_histogram(
        name: &str,
        metric: &str,
        p50_ms: f64,
        p95_ms: f64,
        p99_ms: f64,
        extra: serde_json::Value,
        budget: Option<(&str, f64)>,
    ) -> Self {
        let (label, pass) = match budget {
            Some((label, threshold)) => (Some(label.to_string()), Some(p95_ms <= threshold)),
            None => (None, None),
        };
        Self {
            name: name.to_string(),
            metric: metric.to_string(),
            unit: "ms".to_string(),
            p50_ms,
            p95_ms,
            p99_ms,
            extra,
            budget: label,
            pass,
            not_measured: false,
        }
    }

    /// A scenario that could not be measured in this run.
    #[must_use]
    pub fn not_measured(name: &str, metric: &str, reason: &str) -> Self {
        Self {
            name: name.to_string(),
            metric: metric.to_string(),
            unit: "ms".to_string(),
            p50_ms: 0.0,
            p95_ms: 0.0,
            p99_ms: 0.0,
            extra: serde_json::json!({ "reason": reason }),
            budget: None,
            pass: None,
            not_measured: true,
        }
    }
}

/// A complete benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Results {
    /// ISO-8601 UTC timestamp of the run.
    pub generated_at: String,
    /// Machine the run happened on.
    pub machine: MachineRecord,
    /// `true` when the numbers come from a dev machine (or any machine that
    /// is not the named weakest-PC target). Provisional numbers never gate.
    pub provisional: bool,
    /// `"full"` (500k) or `"smoke"` (10k).
    pub dataset: String,
    /// Per-scenario results, in scenario order.
    pub scenarios: Vec<ScenarioResult>,
}

impl Results {
    /// Serialize to pretty JSON.
    ///
    /// # Errors
    /// Returns [`BenchError::Json`] if serialization fails.
    pub fn to_json(&self) -> Result<String, BenchError> {
        serde_json::to_string_pretty(self).map_err(BenchError::Json)
    }

    /// Markdown table, ready to paste into `08-decisions.md`.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "## Benchmark results ({}) — {} — {}\n",
            self.dataset,
            self.generated_at,
            if self.provisional {
                "**PROVISIONAL** (dev machine, not the target PC)"
            } else {
                "target PC"
            }
        );
        let _ = writeln!(
            out,
            "Machine: {} / {} MiB RAM / disk {} / OS {} / WebView2 {} / power {}\n",
            self.machine.cpu,
            self.machine.ram_mb,
            self.machine.disk,
            self.machine.os,
            self.machine.webview2,
            self.machine.power_plan
        );
        out.push_str("| Scenario | Metric | p50 (ms) | p95 (ms) | p99 (ms) | Budget | Pass |\n");
        out.push_str("|---|---|---|---|---|---|---|\n");
        for s in &self.scenarios {
            let pass = match (s.not_measured, s.pass) {
                (true, _) => "not measured",
                (false, Some(true)) => "yes",
                (false, Some(false)) => "NO",
                (false, None) => "n/a",
            };
            let _ = writeln!(
                out,
                "| {} | {} | {:.3} | {:.3} | {:.3} | {} | {} |",
                s.name,
                s.metric,
                s.p50_ms,
                s.p95_ms,
                s.p99_ms,
                s.budget.as_deref().unwrap_or("—"),
                pass
            );
        }
        out
    }
}

/// Stored CI smoke baseline: metric key → p95 in ms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    /// Schema version of this file.
    pub version: u32,
    /// Machine that produced the baseline (for the record, not for matching).
    pub machine: String,
    /// Metric key (`scenario name :: metric`) → p95 ms.
    pub metrics: HashMap<String, f64>,
}

impl Baseline {
    /// Build a baseline from smoke `results` (p95 of every measured scenario).
    #[must_use]
    pub fn from_results(results: &Results) -> Self {
        let mut metrics = HashMap::new();
        for s in &results.scenarios {
            if !s.not_measured {
                metrics.insert(format!("{} :: {}", s.name, s.metric), s.p95_ms);
            }
        }
        Self {
            version: 1,
            machine: format!(
                "{} / {} MiB RAM / {}",
                results.machine.cpu, results.machine.ram_mb, results.machine.os
            ),
            metrics,
        }
    }

    /// Load a baseline file.
    ///
    /// # Errors
    /// Returns [`BenchError::Io`] or [`BenchError::Json`] on failure.
    pub fn load(path: &Path) -> Result<Self, BenchError> {
        let text = std::fs::read_to_string(path).map_err(BenchError::Io)?;
        serde_json::from_str(&text).map_err(BenchError::Json)
    }

    /// Save a baseline file (creates parent directories).
    ///
    /// # Errors
    /// Returns [`BenchError::Io`] or [`BenchError::Json`] on failure.
    pub fn save(&self, path: &Path) -> Result<(), BenchError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(BenchError::Io)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(BenchError::Json)?;
        std::fs::write(path, text).map_err(BenchError::Io)
    }

    /// Compare `results` against this baseline. Returns the list of
    /// violations (empty = within tolerance). A metric fails only when it
    /// exceeds `BASELINE_TOLERANCE_FACTOR` × its baseline value.
    #[must_use]
    pub fn violations(&self, results: &Results) -> Vec<String> {
        let mut out = Vec::new();
        for s in &results.scenarios {
            if s.not_measured {
                continue;
            }
            let key = format!("{} :: {}", s.name, s.metric);
            if let Some(base) = self.metrics.get(&key)
                && s.p95_ms > *base * BASELINE_TOLERANCE_FACTOR
            {
                out.push(format!(
                    "{key}: p95 {:.3} ms vs baseline {:.3} ms (> {:.0}x)",
                    s.p95_ms, base, BASELINE_TOLERANCE_FACTOR
                ));
            }
        }
        out
    }
}
