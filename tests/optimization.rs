//! Differential tests for the opt-in source optimizer, including Luau value arity.
use realobfuscator::{ObfuscationConfig, Obfuscator, ProtectionLevel};
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
            "realobfuscator-optimizer-{}-{}",
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
            panic!("optimizer runtime deadline: {}", path.display())
        }
        thread::sleep(Duration::from_millis(5));
    }
}
const SEMANTICS: &str = r#"
local events={}
local function hit(n) events[#events+1]=n;return n,nil,n+1 end
local function empty() end
local function pack(...) return select('#',...),... end
local function choose(...) return (false or ...),(if true then ... else 99) end
print(pack(true and hit(1)))
print(pack(false or hit(2)))
print(pack(if 0 then hit(3) else hit(99)))
print(pack(if false then 99 elseif false then 98 else empty()))
print(pack(false or empty()))
print(pack(choose()),pack(choose(7,8)))
print(false and hit(98),true or hit(97),'' or hit(96),0 and hit(4))
local flag=true
if flag then hit(5) elseif false then hit(95) elseif true then hit(94) else hit(93) end
flag=false
if flag then hit(92) elseif nil then hit(91) elseif true then local x=6;hit(x) else hit(90) end
local x=11
if 2+3==5 then local x=12;hit(x) else hit(89) end
print(x)
local readers={}
for i=1,4 do
    if true then local captured=i;readers[i]=function()return captured end end
    if i==2 then continue end
    if false then hit(88) end
    hit(i+20)
end
for _,reader in readers do print(reader()) end
while false do hit(87) end
while nil do hit(86) end
local object=setmetatable({},{__add=function()hit(30);return 31 end,__mul=function()hit(32);return 33 end})
print(object+0,object*1)
local safe=pcall(function()return object-0 end);print(safe)
print(1/(-0),1/(-0+0),1/(0*-1),9007199254740991+1,9007199254740992+2)
local nan=0/0;print(nan==nan,1/0,-1/0)
print(6/3,5/2,-7%3,-7//3,2^3,1.25+2.5)
print(#'\000\255\128','a\000'..'\255b','\255'=='\255',nil==false,0<=-0)
print(table.concat(events,','))
"#;

#[test]
fn optimization_preserves_effects_scope_arity_and_numeric_boundaries_across_profiles() {
    let Some(runtime) = runtime() else { return };
    let workspace = Workspace::new();
    let path = workspace.0.join("script.luau");
    fs::write(&path, SEMANTICS).unwrap();
    let expected = run(&runtime, &path);
    assert!(
        expected.status.success(),
        "{}",
        String::from_utf8_lossy(&expected.stderr)
    );
    for (index, mut config) in [
        ObfuscationConfig::lightweight(),
        ObfuscationConfig::default(),
        ObfuscationConfig::default().with_level(ProtectionLevel::Paranoid),
        ObfuscationConfig::hardened(),
        ObfuscationConfig::compatible(),
    ]
    .into_iter()
    .enumerate()
    {
        config.optimize_code = true;
        config.vm_predecode_bytecode = index % 2 == 0;
        config.vm_pack_bytecode = index % 2 == 1;
        config.vm_pack_constants = index % 2 == 1;
        let output = Obfuscator::new(config.with_seed(42))
            .obfuscate_with_report(SEMANTICS)
            .unwrap();
        assert!(output.report.optimization_enabled && output.report.optimized_expressions > 8);
        assert!(output.report.removed_branches > 4 && output.report.removed_loops == 2);
        assert!(
            realobfuscator::detect_watermark(&output.code)
                .unwrap()
                .content_matches
        );
        fs::write(&path, output.code).unwrap();
        let actual = run(&runtime, &path);
        assert!(
            actual.status.success(),
            "profile={index}: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "profile={index}");
    }
}

#[test]
fn optimization_reduces_actual_vm_work_and_stays_opt_in() {
    let source="local function f(x)local k=(19+23)*(7-2);if 3*7==22 then x+=98765 elseif false then x*=91 else x+=k end;while false do x-=17 end;return x,not false,#'abc' end;print(f(2))";
    let config = ObfuscationConfig::hardened().with_seed(42);
    let baseline = Obfuscator::new(config.clone())
        .obfuscate_with_report(source)
        .unwrap();
    assert!(!baseline.report.optimization_enabled);
    assert_eq!(
        baseline.report.optimized_expressions
            + baseline.report.removed_branches
            + baseline.report.removed_loops
            + baseline.report.removed_empty_blocks,
        0
    );
    let mut optimized = config;
    optimized.optimize_code = true;
    let result = Obfuscator::new(optimized)
        .obfuscate_with_report(source)
        .unwrap();
    assert!(result.report.vm_instructions < baseline.report.vm_instructions);
    assert!(result.report.output_bytes < baseline.report.output_bytes);
    assert_eq!(result.report.vm_functions, baseline.report.vm_functions);
    if let Some(runtime) = runtime() {
        let workspace = Workspace::new();
        let path = workspace.0.join("script.luau");
        fs::write(&path, baseline.code).unwrap();
        let expected = run(&runtime, &path);
        fs::write(&path, result.code).unwrap();
        let actual = run(&runtime, &path);
        assert!(actual.status.success());
        assert_eq!(actual.stdout, expected.stdout);
    }
}

#[test]
fn optimization_keeps_type_functions_and_strict_environment_guard() {
    let mut config = ObfuscationConfig::lightweight().with_seed(42);
    config.optimize_code = true;
    config.strip_types = false;
    config.encrypt_strings = false;
    let result = Obfuscator::new(config)
        .obfuscate_with_report(
            "type function make()local n=2+3;return types.number end;local x=2+3;print(x)",
        )
        .unwrap();
    assert_eq!(result.report.optimized_expressions, 1);
    assert!(result.code.contains("2+3"));
    let mut strict = ObfuscationConfig::hardened().with_seed(42);
    strict.optimize_code = true;
    assert!(Obfuscator::new(strict)
        .obfuscate("if false then getfenv() end;return 17")
        .is_err());
}
