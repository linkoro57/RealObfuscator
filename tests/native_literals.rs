//! Native fallback literals must retain environments, stack frames and protocols.
use realobfuscator::{ObfuscationConfig, Obfuscator};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "realobfuscator-native-literals-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn runtime() -> Option<PathBuf> {
    let explicit = std::env::var_os("LUAU_BIN");
    let path = explicit.clone().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/runtime/0.741")
            .join(if cfg!(windows) { "luau.exe" } else { "luau" })
    });
    if path.is_file() {
        Some(path)
    } else {
        assert!(
            explicit.is_none() && std::env::var_os("REQUIRE_LUAU_RUNTIME").is_none(),
            "install Luau or set LUAU_BIN"
        );
        None
    }
}

fn run(runtime: &Path, path: &Path) -> Output {
    let mut child = Command::new(runtime)
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if start.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("native literal runtime deadline: {}", path.display());
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn native_frames_environment_mutation_and_interpolation_survive_bound_literals() {
    let Some(runtime) = runtime() else { return };
    let workspace = Workspace::new();
    let source = r#"
        local function identity(...) return ... end
        local function read()
            return 'bound native literal', counter, debug.info(1,'f')==read,
                identity 'binary\000\255', '', 'a long native string split across multiple bound fragments'
        end
        local env={counter=32,debug=debug}
        setfenv(read,env)
        print(getfenv(read)==env,pcall(read))
        local seen={}
        local function value(n)
            seen[#seen+1]=n
            return setmetatable({}, {__tostring=function() seen[#seen+1]=n+10;return tostring(n) end})
        end
        local receiver={}
        function receiver:send(s) return s end
        print(receiver:send(`100% {value(1)} / {value(2)}\000`))
        print(table.concat(seen,','))
        print(receiver:send 'shorthand native method literal')
        local function empty() end
        local function many() return 17,18 end
        local function varargs(...) return `arg={...}` end
        print(`empty={empty()}`,`many={many()}`,varargs(),varargs(19,20))
    "#;
    let original = workspace.0.join("original.luau");
    fs::write(&original, source).unwrap();
    let expected = run(&runtime, &original);
    assert!(
        expected.status.success(),
        "{}",
        String::from_utf8_lossy(&expected.stderr)
    );
    for recipes in [false, true] {
        for cached in [false, true] {
            let mut config = ObfuscationConfig::compatible().with_seed(if cached { 42 } else { 0 });
            config.vm_literal_programs = recipes;
            config.vm_predecode_bytecode = cached;
            config.vm_pack_bytecode = !cached;
            config.vm_pack_constants = !cached;
            let result = Obfuscator::new(config)
                .obfuscate_with_report(source)
                .unwrap();
            assert_eq!(result.report.vm_functions, 0);
            assert_eq!(result.report.vm_chunks, 0);
            assert!(result.report.vm_native_functions > 0);
            assert!(result.report.vm_chunk_fallback.is_some());
            assert_eq!(result.report.vm_native_literal_prototypes, 1);
            assert!(result.report.vm_native_bound_strings > 8);
            assert_eq!(result.report.vm_unbound_string_references, 0);
            let path = workspace.0.join("protected.luau");
            fs::write(&path, result.code).unwrap();
            let actual = run(&runtime, &path);
            assert!(
                actual.status.success(),
                "recipes={recipes} cached={cached}: {}",
                String::from_utf8_lossy(&actual.stderr)
            );
            assert_eq!(actual.stdout, expected.stdout);
        }
    }
}

#[test]
fn interpolations_adjust_empty_and_multiple_results_in_all_profiles() {
    let Some(runtime) = runtime() else { return };
    let workspace = Workspace::new();
    let source = r#"
        local function empty() end
        local function many() return 17,18 end
        local function format(...) return `arg={...}` end
        print(`empty={empty()}`,`many={many()}`,format(),format(19,20))
    "#;
    let original = workspace.0.join("original.luau");
    fs::write(&original, source).unwrap();
    let expected = run(&runtime, &original);
    assert!(expected.status.success());
    for config in [
        ObfuscationConfig::lightweight(),
        ObfuscationConfig::default(),
        ObfuscationConfig::hardened(),
        ObfuscationConfig::compatible(),
    ] {
        let output = Obfuscator::new(config.with_seed(42))
            .obfuscate(source)
            .unwrap();
        let path = workspace.0.join("protected.luau");
        fs::write(&path, output).unwrap();
        let actual = run(&runtime, &path);
        assert!(
            actual.status.success(),
            "{}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout);
    }
}

#[test]
fn compiled_method_bridges_can_bind_interpolation_through_literal_only_prototypes() {
    let Some(runtime) = runtime() else { return };
    let workspace = Workspace::new();
    let source =
        "local r={};function r:send(s)return s end;local n=17;print(r:send(`bridge {n}%`))";
    let result = Obfuscator::new(ObfuscationConfig::compatible().with_seed(7))
        .obfuscate_with_report(source)
        .unwrap();
    assert_eq!(result.report.vm_chunks, 1);
    assert!(result.report.vm_functions > 0);
    assert_eq!(result.report.vm_native_literal_prototypes, 1);
    assert_eq!(result.report.vm_unbound_string_references, 0);
    let path = workspace.0.join("protected.luau");
    fs::write(&path, result.code).unwrap();
    let actual = run(&runtime, &path);
    assert!(
        actual.status.success(),
        "{}",
        String::from_utf8_lossy(&actual.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&actual.stdout).trim(), "bridge 17%");
}
