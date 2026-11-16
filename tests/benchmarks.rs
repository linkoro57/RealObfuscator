//! Correctness checks for the benchmark inputs, statistics and report protocol.
#![forbid(unsafe_code)]
#[allow(dead_code)]
#[path = "../benches/support/mod.rs"]
mod support;

use realobfuscator::{rng::Rng, ObfuscationReport, Obfuscator};
use std::{collections::BTreeSet, fs, path::PathBuf, time::Duration};
use support::{
    corpus,
    measurement::{measure, Statistics},
    profiles,
    report::{Case, Report},
    runtime::{self, Runtime},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "realobfuscator-bench-test-{}-{:016x}",
            std::process::id(),
            Rng::random().next_u64()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        // Only known benchmark-created paths; avoid recursive cleanup.
        for file in [
            "runtime/driver.luau",
            "runtime/module.luau",
            "runtime-timeout/driver.luau",
            "runtime-timeout/module.luau",
            "reports/metadata.txt",
            "reports/summary.csv",
            "reports/samples.csv",
        ] {
            let _ = fs::remove_file(self.0.join(file));
        }
        for directory in ["runtime", "runtime-timeout", "reports", ""] {
            let _ = fs::remove_dir(self.0.join(directory));
        }
    }
}

#[test]
fn statistics_handle_even_odd_and_single_sample_distributions() {
    let even = Statistics::from_samples(&[4.0, 1.0, 3.0, 2.0]);
    assert_eq!(
        (even.min, even.median, even.p95, even.max, even.mean),
        (1.0, 2.5, 4.0, 4.0, 2.5)
    );
    assert!((even.stddev - 1.25f64.sqrt()).abs() < 1e-12);
    assert_eq!(Statistics::from_samples(&[3.0, 1.0, 2.0]).median, 2.0);
    let single = Statistics::from_samples(&[7.0]);
    assert_eq!((single.median, single.p95, single.stddev), (7.0, 7.0, 0.0));
    let long: Vec<_> = (1..=100).map(f64::from).collect();
    assert_eq!(Statistics::from_samples(&long).p95, 95.0);
}

#[test]
fn measurement_calibrates_and_propagates_operation_failures() {
    let mut calls = 0;
    let measured = measure(3, Duration::from_millis(1), || {
        calls += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 1 + 3 * measured.batch);
    assert_eq!(measured.samples_ms.len(), 3);
    assert!(measured
        .samples_ms
        .iter()
        .all(|time| time.is_finite() && *time >= 0.0));
    assert!(measure(3, Duration::from_millis(1), || Err(
        "deliberate failure".into()
    ))
    .is_err());
}

#[test]
fn profiles_are_valid_unique_and_include_the_full_vm_cross_product() {
    let profiles = profiles::all();
    let names: BTreeSet<_> = profiles.iter().map(|profile| &profile.name).collect();
    assert_eq!(names.len(), profiles.len());
    for profile in &profiles {
        profile.config.validate().unwrap();
    }
    let variants: BTreeSet<_> = profiles
        .iter()
        .filter(|profile| {
            profile.name.starts_with("vm-")
                && !matches!(
                    profile.name.as_str(),
                    "vm-functions-only" | "vm-no-integrity" | "vm-no-renaming"
                )
        })
        .map(|profile| {
            (
                &profile.config.vm_hardening,
                profile.config.vm_pack_bytecode,
                profile.config.vm_predecode_bytecode,
                profile.config.cache_strings,
            )
        })
        .collect();
    assert_eq!(variants.len(), 3 * 2 * 2 * 2);
}

#[test]
fn csv_reports_preserve_sample_counts_and_refuse_existing_directories() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("reports");
    let mut report = Report::create(&directory, "test metadata").unwrap();
    let transformations = ObfuscationReport::default();
    let case = Case {
        workload: "contains,comma",
        scale: 8,
        profile: "test\"profile",
        seed: 42,
        input_bytes: 100,
        output_bytes: 200,
        report: &transformations,
    };
    let measured = measure(3, Duration::from_millis(1), || Ok(())).unwrap();
    report.record("obfuscate", &case, &measured).unwrap();
    drop(report);
    let summary = fs::read_to_string(directory.join("summary.csv")).unwrap();
    assert_eq!(summary.lines().count(), 2);
    assert!(summary.contains("\"contains,comma\""));
    assert!(summary.contains("\"test\"\"profile\""));
    assert_eq!(
        fs::read_to_string(directory.join("samples.csv"))
            .unwrap()
            .lines()
            .count(),
        4
    );
    assert!(Report::create(&directory, "must not overwrite").is_err());
}

#[test]
fn every_benchmark_workload_preserves_its_runtime_checksum() {
    let executable = match runtime::locate() {
        Ok(executable) => executable,
        Err(error) => {
            assert!(
                std::env::var_os("REQUIRE_LUAU_RUNTIME").is_none()
                    && std::env::var_os("LUAU_BIN").is_none(),
                "{error}"
            );
            eprintln!("SKIP benchmark runtime checks: {error}");
            return;
        }
    };
    let workspace = Workspace::new();
    let runtime = Runtime::new(
        &workspace.0.join("runtime"),
        &executable,
        1,
        2,
        1,
        Duration::from_secs(10),
    )
    .unwrap();
    let profiles = profiles::all();
    for workload in corpus::WORKLOADS {
        let source = corpus::source(workload, 3);
        let original = runtime
            .measure(&source, None)
            .unwrap_or_else(|error| panic!("original {workload}: {error}"));
        for name in [
            "lightweight",
            "native",
            "vm-basic-cached-packed",
            "vm-max-rolling-packed",
        ] {
            let profile = profiles
                .iter()
                .find(|profile| profile.name == name)
                .unwrap();
            let output = Obfuscator::new(profile.config.clone())
                .obfuscate(&source)
                .unwrap_or_else(|error| panic!("{workload}/{name}: {error}"));
            let result = runtime
                .measure(&output, Some(&original.checksum))
                .unwrap_or_else(|error| panic!("{workload}/{name}: {error}"));
            assert_eq!(result.module_load.samples_ms.len(), 1);
            assert_eq!(result.warm_execution.samples_ms.len(), 2);
        }
    }
    assert!(runtime
        .measure("return function() return 1 end", Some("2"))
        .is_err());
    assert!(runtime
        .measure("error('deliberate runtime error')", None)
        .is_err());
}

#[test]
fn nonterminating_runtime_is_killed_by_the_deadline() {
    let Ok(executable) = runtime::locate() else {
        return;
    };
    let workspace = Workspace::new();
    let runtime = Runtime::new(
        &workspace.0.join("runtime-timeout"),
        &executable,
        1,
        1,
        1,
        Duration::from_millis(100),
    )
    .unwrap();
    let error = runtime
        .measure("return function() while true do end end", None)
        .err()
        .expect("infinite loop must time out");
    assert!(error.to_string().contains("timed out"));
}
