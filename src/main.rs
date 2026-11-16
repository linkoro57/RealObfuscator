#![forbid(unsafe_code)]

mod file_io;

use file_io::{atomic_write, collect, resolved};
use realobfuscator::{DeadCodeDensity, ObfuscationConfig, Obfuscator, ProtectionLevel};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
    time::Instant,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const HELP: &str = "Usage: realobfuscator [OPTIONS] <SOURCE>

Options:
  -o, --output <PATH>       Output file or directory
      --mode <MODE>         lightweight (default), rename, native, vm, or hardened
                            vm compiles supported functions to register bytecode
      --require-function-vm Fail if a function needs native fallback (VM mode)
      --require-chunk-vm    Fail if the main chunk needs native fallback
      --vm-functions-only  Compile functions; retain native main-chunk control flow
      --stats               Show counts of transformations actually applied
      --pretty              Format generated code across multiple lines
      --decode <lazy|eager> String decoding strategy
      --keep-types          Preserve Luau type annotations and declarations
      --wrap-in-iife        Isolate source locals from helpers in a function frame
      --string-cache        Retain decoded strings for speed
      --no-string-cache     Decode strings without retaining a plaintext cache
      --split-strings       Fragment long source strings before encoding
      --no-split-strings    Keep source strings in one piece
      --vm-scatter         Scatter VM data into shuffled columns (VM mode)
      --no-vm-scatter      Keep ordinary VM descriptor tables
      --vm-rolling          Decode VM instructions while reading (default)
      --vm-cached           Cache decoded VM instructions
      --vm-packed           Use compact VM payloads
      --vm-unpacked         Use table VM payloads
      --vm-bound-strings    Bind compiled string constants to their VM prototype
      --vm-unbound-strings  Use the legacy shared-pool string protection
      --vm-literal-programs Reconstruct bound strings through VM instruction recipes
      --no-vm-literal-programs Use the older prototype-bound literal payloads
      --require-bound-strings Reject runtime strings outside bound VM constants
  -q, --quiet               Show errors only
  -h, --help                Print help
  -V, --version             Print version
      --help-all            Show compatibility options
";
const EXTRA: &str = "
Compatibility options:
  -i, --input <PATH>
  -l, --level <1..4>
      --vm
      --antitamper / --no-antitamper
      --strings / --no-strings
      --cff / --no-cff
      --deadcode / --no-deadcode
      --density <low|medium|high>
      --seed <INTEGER>
      --vm-hardening <basic|client-max|max>
      --lightweight
      --rename-only
";

#[derive(Default)]
struct Args {
    source: Option<PathBuf>,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    mode: Option<String>,
    level: Option<u8>,
    vm: bool,
    quiet: bool,
    seed: Option<u64>,
    strings: Option<bool>,
    cff: Option<bool>,
    dead: Option<bool>,
    antitamper: Option<bool>,
    density: Option<DeadCodeDensity>,
    hardening: Option<String>,
    rolling: Option<bool>,
    packed: Option<bool>,
    lightweight: bool,
    rename_only: bool,
    stats: bool,
    pretty: bool,
    keep_types: bool,
    wrap_in_iife: bool,
    decode: Option<String>,
    cache_strings: Option<bool>,
    require_function_vm: bool,
    require_chunk_vm: bool,
    functions_only: bool,
    bind_strings: Option<bool>,
    literal_programs: Option<bool>,
    require_bound_strings: bool,
    split_strings: Option<bool>,
    scatter: Option<bool>,
}

fn usage(message: &str) -> ! {
    eprintln!("error: {message}\n\nTry '--help' for more information.");
    process::exit(2)
}

fn value(it: &mut impl Iterator<Item = String>, option: &str) -> String {
    it.next()
        .unwrap_or_else(|| usage(&format!("{option} requires a value")))
}

fn parse() -> Args {
    let mut args = Args::default();
    let mut it = env::args().skip(1);
    while let Some(option) = it.next() {
        match option.as_str() {
            "-h" | "--help" => {
                println!("RealObfuscator {VERSION}\n\n{HELP}");
                process::exit(0)
            }
            "--help-all" => {
                println!("RealObfuscator {VERSION}\n\n{HELP}{EXTRA}");
                process::exit(0)
            }
            "-V" | "--version" => {
                println!("RealObfuscator {VERSION}");
                process::exit(0)
            }
            "-o" | "--output" => args.output = Some(value(&mut it, &option).into()),
            "-i" | "--input" => args.input = Some(value(&mut it, &option).into()),
            "--mode" => args.mode = Some(value(&mut it, &option)),
            "-l" | "--level" => {
                args.level = Some(
                    value(&mut it, &option)
                        .parse()
                        .unwrap_or_else(|_| usage("level must be 1..4")),
                )
            }
            "--vm" => args.vm = true,
            "-q" | "--quiet" => args.quiet = true,
            "--stats" => args.stats = true,
            "--pretty" => args.pretty = true,
            "--keep-types" => args.keep_types = true,
            "--wrap-in-iife" => args.wrap_in_iife = true,
            "--string-cache" => args.cache_strings = Some(true),
            "--no-string-cache" => args.cache_strings = Some(false),
            "--split-strings" => args.split_strings = Some(true),
            "--no-split-strings" => args.split_strings = Some(false),
            "--vm-scatter" => args.scatter = Some(true),
            "--no-vm-scatter" => args.scatter = Some(false),
            "--require-function-vm" => args.require_function_vm = true,
            "--require-chunk-vm" => args.require_chunk_vm = true,
            "--vm-functions-only" => args.functions_only = true,
            "--vm-bound-strings" => args.bind_strings = Some(true),
            "--vm-literal-programs" => args.literal_programs = Some(true),
            "--no-vm-literal-programs" => args.literal_programs = Some(false),
            "--vm-unbound-strings" => args.bind_strings = Some(false),
            "--require-bound-strings" => args.require_bound_strings = true,
            "--decode" => args.decode = Some(value(&mut it, &option)),
            "--seed" => {
                args.seed = Some(
                    value(&mut it, &option)
                        .parse()
                        .unwrap_or_else(|_| usage("seed must be an integer")),
                )
            }
            "--strings" => args.strings = Some(true),
            "--no-strings" => args.strings = Some(false),
            "--cff" => args.cff = Some(true),
            "--no-cff" => args.cff = Some(false),
            "--deadcode" => args.dead = Some(true),
            "--no-deadcode" => args.dead = Some(false),
            "--antitamper" => args.antitamper = Some(true),
            "--no-antitamper" => args.antitamper = Some(false),
            "--density" => {
                args.density = Some(match value(&mut it, &option).as_str() {
                    "low" => DeadCodeDensity::Low,
                    "medium" => DeadCodeDensity::Medium,
                    "high" => DeadCodeDensity::High,
                    _ => usage("density must be low, medium, or high"),
                })
            }
            "--vm-hardening" => args.hardening = Some(value(&mut it, &option)),
            "--vm-rolling" => args.rolling = Some(true),
            "--vm-cached" => args.rolling = Some(false),
            "--vm-packed" => args.packed = Some(true),
            "--vm-unpacked" => args.packed = Some(false),
            "--lightweight" => args.lightweight = true,
            "--rename-only" => args.rename_only = true,
            "-r" | "--recursive" => {}
            "--" => {
                for path in it.by_ref() {
                    if args.source.replace(path.into()).is_some() {
                        usage("only one SOURCE may be provided")
                    }
                }
            }
            _ if option.starts_with('-') => usage(&format!("unknown option {option}")),
            _ => {
                if args.source.replace(option.into()).is_some() {
                    usage("only one SOURCE may be provided")
                }
            }
        }
    }
    args
}

fn config(args: &Args) -> ObfuscationConfig {
    if args.source.is_some() && args.input.is_some() {
        usage("use SOURCE or --input, not both")
    }
    let primary = u8::from(args.mode.is_some())
        + u8::from(args.level.is_some())
        + u8::from(args.lightweight)
        + u8::from(args.rename_only)
        + u8::from(args.vm);
    if primary > 1 {
        usage("choose one of --mode, --level, --lightweight, --rename-only, or --vm")
    }
    let mode = if args.lightweight {
        Some("lightweight")
    } else if args.rename_only {
        Some("rename")
    } else if args.vm {
        Some("vm")
    } else {
        args.mode.as_deref()
    };
    let mut config = match mode {
        Some("lightweight") => ObfuscationConfig::lightweight(),
        Some("rename") => {
            let mut c = ObfuscationConfig::lightweight();
            c.encrypt_strings = false;
            c
        }
        Some("native") => ObfuscationConfig::default().with_level(ProtectionLevel::Maximum),
        Some("vm") => ObfuscationConfig::default().with_level(ProtectionLevel::Paranoid),
        Some("hardened") => ObfuscationConfig::hardened(),
        Some(_) => usage("mode must be lightweight, rename, native, vm, or hardened"),
        None => {
            if let Some(level) = args.level {
                ObfuscationConfig::default()
                    .with_level(ProtectionLevel::try_from(level).unwrap_or_else(|e| usage(&e)))
            } else if args.antitamper == Some(true)
                || args.cff == Some(true)
                || args.dead == Some(true)
            {
                ObfuscationConfig::default()
            } else {
                ObfuscationConfig::lightweight()
            }
        }
    };
    if let Some(seed) = args.seed {
        config = config.with_seed(seed)
    }
    if let Some(value) = args.strings {
        config.encrypt_strings = value
    }
    if let Some(value) = args.split_strings {
        config.split_strings = value;
    }
    if let Some(value) = args.scatter {
        config.vm_scatter_data = value;
    }
    if let Some(value) = args.cff {
        config.control_flow_flatten = value
    }
    if let Some(value) = args.dead {
        config.inject_dead_code = value
    }
    if let Some(value) = args.antitamper {
        config.anti_tamper = value
    }
    if let Some(value) = args.density {
        config.dead_code_density = value
    }
    if let Some(value) = &args.decode {
        config.string_decode_mode = value.clone()
    }
    config.minify = !args.pretty;
    config.strip_types = !args.keep_types;
    config.wrap_in_iife = args.wrap_in_iife;
    config.require_function_vm |= args.require_function_vm;
    config.require_chunk_vm |= args.require_chunk_vm;
    if let Some(bound) = args.bind_strings {
        config.vm_bind_strings = bound;
    }
    if let Some(programs) = args.literal_programs {
        config.vm_literal_programs = programs;
    }
    if args.require_bound_strings {
        if args.bind_strings == Some(false) {
            usage("--require-bound-strings conflicts with --vm-unbound-strings")
        }
        config.vm_bind_strings = true;
        config.require_bound_strings = true;
    }
    if args.functions_only {
        if !config.virtualize {
            usage("--vm-functions-only requires VM mode")
        }
        config.virtualize_chunk = false;
    }
    if let Some(cache) = args.cache_strings {
        config.cache_strings = cache;
    }
    if let Some(value) = &args.hardening {
        if !config.virtualize {
            usage("--vm-hardening requires VM mode")
        }
        config.vm_hardening = value.clone();
    }
    if let Some(value) = args.rolling {
        if !config.virtualize {
            usage("--vm-rolling/--vm-cached requires VM mode")
        }
        config.vm_predecode_bytecode = !value;
    }
    if let Some(value) = args.packed {
        if !config.virtualize {
            usage("--vm-packed/--vm-unpacked requires VM mode")
        }
        config.vm_pack_bytecode = value;
        config.vm_pack_constants = value;
    }
    config
}

fn one(input: &Path, output: &Path, obfuscator: &Obfuscator, args: &Args) -> bool {
    let result = (|| {
        let source = fs::read(input).map_err(|e| e.to_string())?;
        let output_code = obfuscator
            .obfuscate_bytes_with_report(&source)
            .map_err(|e| e.to_string())?;
        atomic_write(output, &output_code.code)
            .map_err(|error| format!("cannot write {}: {error}", output.display()))?;
        if !args.quiet {
            let report = &output_code.report;
            if report.vm_unbound_string_references != 0 {
                eprintln!("Warning: {} runtime string references remain outside bound VM constants; use --require-bound-strings to reject this output.", report.vm_unbound_string_references);
            }
            if let Some(reason) = &report.vm_chunk_fallback {
                eprintln!("Warning: main chunk remains native ({reason}); use --require-chunk-vm to reject this fallback.");
            }
            if report.vm_native_functions != 0 {
                eprintln!(
                    "Warning: {} functions remain native; use --stats for reasons or --require-function-vm to reject these fallbacks.",
                    report.vm_native_functions
                );
            }
            println!(
                "{} -> {} ({} -> {} bytes)",
                input.display(),
                output.display(),
                report.input_bytes,
                report.output_bytes
            );
            if args.stats {
                println!(
                    "  split strings: {}; scattered VM tables: {}; scattered VM entries: {}",
                    report.split_strings, report.vm_scattered_tables, report.vm_scattered_entries
                );
                if obfuscator.config.vm_bind_strings {
                    println!(
                        "  literal recipe instructions: {}",
                        report.vm_literal_instructions
                    );
                    println!(
                        "  VM bound strings: {}; unbound runtime string references: {}",
                        report.vm_bound_strings, report.vm_unbound_string_references
                    );
                }
                println!("  bindings: {}; strings: {} ({} references); numbers: {}; fields: {}; dispatch regions: {}; VM statements: {}; decoys: {}; predicates: {}; integrity checks: {}",
                    report.renamed_bindings, report.encrypted_strings, report.string_references, report.obfuscated_numbers,
                    report.hidden_fields, report.flattened_regions, report.virtualized_statements, report.decoy_blocks,
                    report.opaque_predicates, report.integrity_checks);
                println!("  VM functions: {}; bytecode instructions: {}; opcode variants: {}; fused instructions: {}; native functions: {}; method bridges: {}",
                    report.vm_functions, report.vm_instructions, report.vm_opcode_variants, report.vm_fused_instructions, report.vm_native_functions, report.vm_native_bridges);
                if report.vm_iteration_bridges != 0 {
                    println!(
                        "  native iteration bridges: {}",
                        report.vm_iteration_bridges
                    );
                }
                if report.vm_closure_bridges != 0 {
                    println!(
                        "  captured-cell closure factories: {}",
                        report.vm_closure_bridges
                    );
                }
                if obfuscator.config.virtualize_chunk && obfuscator.config.virtualize {
                    println!("  VM main chunks: {}", report.vm_chunks);
                    if let Some(reason) = &report.vm_chunk_fallback {
                        println!("    main chunk fallback: {reason}");
                    }
                }
                for (reason, count) in &report.vm_fallback_reasons {
                    println!("    native fallback: {reason}: {count}");
                }
            }
        }
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        eprintln!("Error: {}: {error}", input.display());
        false
    } else {
        true
    }
}

fn main() {
    let args = parse();
    let input = args
        .source
        .clone()
        .or(args.input.clone())
        .unwrap_or_else(|| usage("provide a Luau file or directory"));
    if !input.exists() {
        eprintln!("Error: input path not found: {}", input.display());
        process::exit(1)
    }
    let config = config(&args);
    if let Err(error) = config.validate() {
        usage(&error)
    }
    let obfuscator = Obfuscator::new(config);
    let start = Instant::now();
    let input_absolute = resolved(&input).unwrap_or_else(|e| usage(&e.to_string()));
    let output = args.output.clone().unwrap_or_else(|| {
        if input.is_dir() {
            input_absolute.with_file_name(format!(
                "{}-obfuscated",
                input_absolute
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ))
        } else {
            let extension = input
                .extension()
                .and_then(|s| s.to_str())
                .filter(|s| matches!(s.to_ascii_lowercase().as_str(), "lua" | "luau"))
                .unwrap_or("luau");
            input.with_file_name(format!(
                "{}.obf.{extension}",
                input.file_stem().unwrap().to_string_lossy()
            ))
        }
    });
    let output_absolute = resolved(&output).unwrap_or_else(|e| usage(&e.to_string()));
    if output_absolute == input_absolute {
        usage("output must differ from input")
    }
    let success = if input.is_file() {
        if output.is_dir() {
            usage("file input needs a file output")
        }
        one(&input, &output, &obfuscator, &args)
    } else {
        if output.is_file() {
            usage("directory input needs a directory output")
        }
        if output_absolute.starts_with(&input_absolute)
            || input_absolute.starts_with(&output_absolute)
        {
            usage(
                "input and output directories must not overlap; choose a sibling output directory",
            )
        }
        let mut files = Vec::new();
        if let Err(error) = collect(&input, &mut files) {
            eprintln!("Error: {error}");
            process::exit(1)
        }
        files.sort();
        if files.is_empty() {
            eprintln!("Error: no .lua or .luau files found in {}", input.display());
            process::exit(1)
        }
        let mut good = 0;
        for file in &files {
            let target = output.join(file.strip_prefix(&input).unwrap());
            if one(file, &target, &obfuscator, &args) {
                good += 1;
            }
        }
        if !args.quiet {
            println!(
                "{good} files written to {}; {} failed ({:.2}s).",
                output.display(),
                files.len() - good,
                start.elapsed().as_secs_f64()
            );
        }
        good == files.len()
    };
    if !success {
        process::exit(1)
    }
}
