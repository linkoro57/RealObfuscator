//! Dependency-free benchmark driver. Invoke through `cargo bench`, not `cargo test`.
#![forbid(unsafe_code)]
mod support;

use realobfuscator::{lexer, rng::Rng, ObfuscationReport, Obfuscator};
use std::{
    collections::BTreeMap,
    env,
    hint::black_box,
    path::PathBuf,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use support::{
    corpus,
    measurement::{measure, Statistics},
    profiles,
    report::{Case, Report},
    runtime::{self, Runtime},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const HELP: &str = "Usage: cargo bench --bench obfuscation -- [OPTIONS]
  --suite quick|standard|full  Matrix size (default: standard)
  --samples N                 Timed samples per operation (default: 5)
  --sample-ms N               Target batch duration, excluding calibration (default: 10)
  --scales N,N                Override workload sizes (full: 8,32,128; others: 32)
  --seeds N,N                 Override build seeds (full: 7,42,1337; others: 42)
  --workload TEXT,TEXT         Select names by substring; prefix = for exact matching
  --profile TEXT,TEXT          Select names by substring; prefix = for exact matching
  --runtime                   Also measure Luau; missing interpreter is an error
  --runtime-only              Omit Rust measurements; enable Luau measurements
  --processes N               Fresh Luau processes per case (default: 3)
  --iterations N              Work units per warm invocation (default: 5)
  --timeout-secs N            Timeout per Luau process (default: 20)
  --order-seed N              Reproducibly shuffle case order (default: 2026)
  --output PATH               New report directory (default: target/benchmarks/<run>)
  --no-micro                  Omit lexer, emitter and RNG microbenchmarks
  --list                      List selected workloads/profiles and case count
  --help                      Print this help

Set LUAU_BIN to select an interpreter. CSV files contain milliseconds per operation;
one warm runtime operation processes --iterations work units. Full matrices can be
expensive: use --list and filters before starting. No downloads or new dependencies.";

struct Options {
    suite: String,
    samples: usize,
    target: Duration,
    scales: Option<Vec<usize>>,
    seeds: Option<Vec<u64>>,
    workloads: Option<String>,
    profiles: Option<String>,
    runtime: bool,
    pipeline: bool,
    micro: bool,
    processes: usize,
    iterations: usize,
    timeout: Duration,
    order_seed: u64,
    output: Option<PathBuf>,
    list: bool,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut options = Self {
            suite: "standard".into(),
            samples: 5,
            target: Duration::from_millis(10),
            scales: None,
            seeds: None,
            workloads: None,
            profiles: None,
            runtime: false,
            pipeline: true,
            micro: true,
            processes: 3,
            iterations: 5,
            timeout: Duration::from_secs(20),
            order_seed: 2026,
            output: None,
            list: false,
        };
        let mut arguments = env::args().skip(1);
        while let Some(argument) = arguments.next() {
            let mut value = || {
                arguments
                    .next()
                    .ok_or_else(|| format!("{argument} requires a value"))
            };
            match argument.as_str() {
                "--bench" => {}
                "--suite" => options.suite = value()?,
                "--samples" => options.samples = bounded(&value()?, 1, 1000)?,
                "--sample-ms" => {
                    options.target = Duration::from_millis(bounded(&value()?, 1, 10_000)? as u64)
                }
                "--scales" => {
                    options.scales = Some(
                        value()?
                            .split(',')
                            .map(|v| bounded(v, 1, 2048))
                            .collect::<Result<_>>()?,
                    )
                }
                "--seeds" => {
                    options.seeds = Some(
                        value()?
                            .split(',')
                            .map(|v| v.parse().map_err(Into::into))
                            .collect::<Result<_>>()?,
                    )
                }
                "--workload" => options.workloads = Some(value()?),
                "--profile" => options.profiles = Some(value()?),
                "--runtime" => options.runtime = true,
                "--runtime-only" => {
                    options.runtime = true;
                    options.pipeline = false;
                    options.micro = false;
                }
                "--processes" => options.processes = bounded(&value()?, 1, 1000)?,
                "--iterations" => options.iterations = bounded(&value()?, 1, 1_000_000)?,
                "--timeout-secs" => {
                    options.timeout = Duration::from_secs(bounded(&value()?, 1, 600)? as u64)
                }
                "--order-seed" => options.order_seed = value()?.parse()?,
                "--output" => options.output = Some(value()?.into()),
                "--no-micro" => options.micro = false,
                "--list" => options.list = true,
                "--help" | "-h" => {
                    println!("{HELP}");
                    std::process::exit(0);
                }
                _ => return Err(format!("unknown argument: {argument}\n{HELP}").into()),
            }
        }
        if !matches!(options.suite.as_str(), "quick" | "standard" | "full") {
            return Err("suite must be quick, standard or full".into());
        }
        Ok(options)
    }
}

fn bounded(value: &str, min: usize, max: usize) -> Result<usize> {
    let number = value.parse()?;
    if !(min..=max).contains(&number) {
        return Err(format!("expected {min}..={max}, got {value}").into());
    }
    Ok(number)
}

fn selected(name: &str, filter: Option<&str>) -> bool {
    filter.is_none_or(|filter| {
        filter.split(',').any(|part| {
            if let Some(exact) = part.strip_prefix('=') {
                name == exact
            } else {
                !part.is_empty() && name.contains(part)
            }
        })
    })
}

fn main() {
    // Cargo bench supplies --bench; test --all-targets can invoke a custom
    // harness with no arguments. Follow the Criterion convention: no timings
    // unless benchmark mode was explicitly selected. Correctness has its own tests.
    if !env::args()
        .any(|argument| matches!(argument.as_str(), "--bench" | "--list" | "--help" | "-h"))
    {
        return;
    }
    if let Err(error) = run() {
        eprintln!("benchmark error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let options = Options::parse()?;
    let full = options.suite == "full";
    let workload_count = if full {
        corpus::WORKLOADS.len()
    } else if options.suite == "quick" {
        6
    } else {
        16
    };
    let profile_count = if full {
        usize::MAX
    } else if options.suite == "quick" {
        4
    } else {
        8
    };
    let workloads: Vec<_> = corpus::WORKLOADS
        .iter()
        .copied()
        .take(workload_count)
        .filter(|name| selected(name, options.workloads.as_deref()))
        .collect();
    let profiles: Vec<_> = profiles::all()
        .into_iter()
        .take(profile_count)
        .filter(|profile| selected(&profile.name, options.profiles.as_deref()))
        .collect();
    if workloads.is_empty() || profiles.is_empty() {
        return Err(
            "filters selected no cases; use --suite full --list to inspect all names".into(),
        );
    }
    let scales =
        options
            .scales
            .clone()
            .unwrap_or_else(|| if full { vec![8, 32, 128] } else { vec![32] });
    let seeds =
        options
            .seeds
            .clone()
            .unwrap_or_else(|| if full { vec![7, 42, 1337] } else { vec![42] });
    let mut cases = Vec::new();
    for workload in &workloads {
        for scale in &scales {
            for (profile, _) in profiles.iter().enumerate() {
                for seed in &seeds {
                    cases.push((*workload, *scale, profile, *seed));
                }
            }
        }
    }
    println!(
        "{} cases; {} workloads x {} scales x {} profiles x {} seeds",
        cases.len(),
        workloads.len(),
        scales.len(),
        profiles.len(),
        seeds.len()
    );
    if options.list {
        println!(
            "workloads: {}\nscales: {scales:?}\nseeds: {seeds:?}\nprofiles:\n{}",
            workloads.join(", "),
            profiles
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        );
        return Ok(());
    }
    if cfg!(debug_assertions) {
        return Err(
            "benchmarks require an optimized build: use cargo bench --bench obfuscation".into(),
        );
    }
    let interpreter = if options.runtime {
        Some(runtime::locate()?)
    } else {
        None
    };
    let run_id = format!(
        "{}-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        std::process::id()
    );
    let directory = options.output.clone().unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/benchmarks")
            .join(run_id)
    });
    let metadata = metadata(&options, interpreter.as_deref());
    let mut report = Report::create(&directory, &metadata)?;
    let runtime = interpreter
        .as_ref()
        .map(|executable| {
            Runtime::new(
                &directory.join("runtime"),
                executable,
                options.processes,
                options.samples,
                options.iterations,
                options.timeout,
            )
        })
        .transpose()?;
    println!("reports: {}", directory.display());
    let empty_report = ObfuscationReport::default();
    let mut checksums = BTreeMap::new();
    for workload in &workloads {
        for scale in &scales {
            let source = corpus::source(workload, *scale);
            let baseline = Case {
                workload,
                scale: *scale,
                profile: "original",
                seed: 0,
                input_bytes: source.len(),
                output_bytes: source.len(),
                report: &empty_report,
            };
            if options.micro {
                let tokens = lexer::lex(&source)?;
                let lex = measure(options.samples, options.target, || {
                    black_box(lexer::lex(black_box(&source))?);
                    Ok(())
                })?;
                report.record("lex", &baseline, &lex)?;
                let emit = measure(options.samples, options.target, || {
                    black_box(lexer::emit(black_box(&tokens)));
                    Ok(())
                })?;
                report.record("emit", &baseline, &emit)?;
            }
            if let Some(runtime) = &runtime {
                let result = runtime.measure(&source, None)?;
                record_runtime(&mut report, &baseline, &result)?;
                checksums.insert((*workload, *scale), result.checksum);
            }
        }
    }
    if options.micro {
        micro_rng(&mut report, &options)?;
    }
    Rng::new(options.order_seed).shuffle(&mut cases);
    for (index, (workload, scale, profile_index, seed)) in cases.iter().enumerate() {
        let profile = &profiles[*profile_index];
        let source = corpus::source(workload, *scale);
        let obfuscator = Obfuscator::new(profile.config.clone().with_seed(*seed));
        let output = obfuscator
            .obfuscate_with_report(&source)
            .map_err(|error| format!("{workload}/{scale}/{}/{seed}: {error}", profile.name))?;
        let case = Case {
            workload,
            scale: *scale,
            profile: &profile.name,
            seed: *seed,
            input_bytes: source.len(),
            output_bytes: output.code.len(),
            report: &output.report,
        };
        if options.pipeline {
            let measurement = measure(options.samples, options.target, || {
                black_box(obfuscator.obfuscate_with_report(black_box(&source))?);
                Ok(())
            })?;
            report.record("obfuscate", &case, &measurement)?;
            println!(
                "[{}/{}] {workload}/{scale}/{} seed={seed}: {:.3} ms, {} bytes",
                index + 1,
                cases.len(),
                profile.name,
                Statistics::from_samples(&measurement.samples_ms).median,
                output.code.len()
            );
        }
        if let Some(runtime) = &runtime {
            let expected = checksums
                .get(&(*workload, *scale))
                .ok_or("missing baseline checksum")?;
            let result = runtime
                .measure(&output.code, Some(expected))
                .map_err(|error| format!("{workload}/{scale}/{}/{seed}: {error}", profile.name))?;
            record_runtime(&mut report, &case, &result)?;
            println!(
                "  Luau [{}/{}] {workload}/{scale}/{}: {:.3} ms warm",
                index + 1,
                cases.len(),
                profile.name,
                Statistics::from_samples(&result.warm_execution.samples_ms).median
            );
        }
    }
    std::fs::write(
        directory.join("complete.txt"),
        format!("completed_cases={}\n", cases.len()),
    )?;
    println!(
        "Completed {} cases. CSV reports: {}",
        cases.len(),
        directory.display()
    );
    Ok(())
}

fn record_runtime(
    report: &mut Report,
    case: &Case<'_>,
    result: &runtime::RuntimeResult,
) -> Result<()> {
    report.record("process_wall", case, &result.process_wall)?;
    report.record("module_load_cpu", case, &result.module_load)?;
    report.record("warm_execution_cpu", case, &result.warm_execution)?;
    Ok(())
}

fn micro_rng(report: &mut Report, options: &Options) -> Result<()> {
    let empty = ObfuscationReport::default();
    let case = Case {
        workload: "rng",
        scale: 1,
        profile: "original",
        seed: 42,
        input_bytes: 0,
        output_bytes: 0,
        report: &empty,
    };
    let mut rng = Rng::new(42);
    let next = measure(options.samples, options.target, || {
        black_box(rng.next_u64());
        Ok(())
    })?;
    report.record("rng_next", &case, &next)?;
    let range = measure(options.samples, options.target, || {
        black_box(rng.range(1, 65536));
        Ok(())
    })?;
    report.record("rng_range", &case, &range)?;
    let name = measure(options.samples, options.target, || {
        black_box(rng.name(8, 14));
        Ok(())
    })?;
    report.record("rng_name", &case, &name)?;
    for size in [16, 128, 1024] {
        let mut values: Vec<_> = (0..size).collect();
        let shuffled = measure(options.samples, options.target, || {
            rng.shuffle(black_box(&mut values));
            Ok(())
        })?;
        report.record(
            "rng_shuffle",
            &Case {
                scale: size,
                ..case
            },
            &shuffled,
        )?;
    }
    Ok(())
}

fn metadata(options: &Options, interpreter: Option<&std::path::Path>) -> String {
    let command = |program: &str, arguments: &[&str]| {
        Command::new(program)
            .args(arguments)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .unwrap_or_else(|| "unavailable".into())
    };
    let fingerprint = interpreter
        .and_then(|path| std::fs::read(path).ok())
        .map(|bytes| {
            let hash = bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
            format!("fnv1a64:{hash:016x};bytes:{}", bytes.len())
        })
        .unwrap_or_else(|| "disabled".into());
    format!("package={}\nos={}\narch={}\nrustc={}\ncommit={}\ngit_status={}\narguments={:?}\nsuite={}\nsamples={}\ntarget_batch_ms={}\nprocesses={}\niterations={}\norder_seed={}\nluau={}\nluau_fingerprint={}\nclock=Rust Instant wall time; Luau os.clock CPU time\nquantile=nearest-rank p95; median averages the middle pair\nprocess_wall=entire driver including load, warmups, timed calls, output and process overhead\nwarm_execution=one invocation with iterations work units; samples pooled across processes\nbatching=one calibration; 1..1000000 operations per Rust sample\n",
        env!("CARGO_PKG_VERSION"), env::consts::OS, env::consts::ARCH, command("rustc", &["--version"]), command("git", &["rev-parse", "HEAD"]), command("git", &["status", "--porcelain"]).replace('\n', "; "), env::args().collect::<Vec<_>>(), options.suite, options.samples, options.target.as_millis(), options.processes, options.iterations, options.order_seed, interpreter.map(|path| path.display().to_string()).unwrap_or_else(|| "disabled".into()), fingerprint)
}
