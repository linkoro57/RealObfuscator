use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> std::path::PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let p = std::env::temp_dir().join(format!(
        "realobfuscator-{}-{n}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&p).unwrap();
    p
}

#[test]
fn cli_obfuscates_a_file_and_is_reproducible() {
    let dir = temp_dir();
    let input = dir.join("input.luau");
    let a = dir.join("a.luau");
    let b = dir.join("b.luau");
    fs::write(&input, "local message='hello';return message,17").unwrap();
    for output in [&a, &b] {
        let status = Command::new(env!("CARGO_BIN_EXE_realobfuscator"))
            .args([
                input.to_str().unwrap(),
                "-o",
                output.to_str().unwrap(),
                "--mode",
                "native",
                "--seed",
                "42",
                "--quiet",
            ])
            .status()
            .unwrap();
        assert!(status.success());
    }
    assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
    assert!(fs::read_to_string(&a)
        .unwrap()
        .starts_with("--!nocheck\n--!nolint"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_rejects_an_input_output_collision() {
    let dir = temp_dir();
    let input = dir.join("input.lua");
    fs::write(&input, "return 1").unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_realobfuscator"))
        .args([input.to_str().unwrap(), "-o", input.to_str().unwrap()])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
    assert_eq!(fs::read_to_string(&input).unwrap(), "return 1");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_rejects_overlapping_directories_before_writing() {
    let dir = temp_dir();
    let input = dir.join("src");
    fs::create_dir_all(&input).unwrap();
    fs::write(input.join("main.luau"), "return 42").unwrap();
    for output in [
        input.join("build"),
        dir.clone(),
        input.join("missing/../build"),
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_realobfuscator"))
            .arg(&input)
            .arg("-o")
            .arg(output)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
    }
    assert_eq!(
        fs::read_to_string(input.join("main.luau")).unwrap(),
        "return 42"
    );
    assert!(!input.join("build").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_input_does_not_replace_an_existing_output() {
    let dir = temp_dir();
    let input = dir.join("bad.luau");
    let output = dir.join("output.luau");
    fs::write(&input, "local =").unwrap();
    fs::write(&output, "keep this").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_realobfuscator"))
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(fs::read_to_string(&output).unwrap(), "keep this");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn directory_builds_preserve_paths_and_report_applied_vm_work() {
    let dir = temp_dir();
    let input = dir.join("src");
    let output = dir.join("build");
    fs::create_dir_all(input.join("nested")).unwrap();
    fs::write(
        input.join("nested/module.LUAU"),
        "local x=1;x+=1;x*=2;print(x,'hello')",
    )
    .unwrap();
    fs::write(input.join("skip.obf.luau"), "invalid").unwrap();
    fs::write(input.join("notes.txt"), "invalid").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_realobfuscator"))
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .args(["--mode", "vm", "--vm-cached", "--stats", "--seed", "9"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.join("nested/module.LUAU").exists());
    assert!(!output.join("skip.obf.luau").exists());
    assert!(!output.join("notes.txt").exists());
    assert!(String::from_utf8_lossy(&result.stdout).contains("VM main chunks: 1"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn strict_function_vm_reports_coverage_and_preserves_output_on_fallback() {
    let dir = temp_dir();
    let input = dir.join("input.luau");
    let output = dir.join("output.luau");
    fs::write(&input, "local function f(x)return x*17+3 end;print(f(2))").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_realobfuscator"))
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .args([
            "--mode",
            "vm",
            "--require-function-vm",
            "--require-chunk-vm",
            "--stats",
            "--seed",
            "42",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("VM functions: 1"));
    let previous = fs::read(&output).unwrap();
    fs::write(&input, "local function f()return getfenv(1)end;return f").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_realobfuscator"))
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .args(["--mode", "vm", "--require-function-vm"])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("environment or stack introspection"));
    assert_eq!(fs::read(&output).unwrap(), previous);
    fs::remove_dir_all(dir).unwrap();
}
