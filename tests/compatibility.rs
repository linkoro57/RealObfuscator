//! Differential regressions for real frontend and runtime compatibility failures.
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
            "realobfuscator-compatibility-{}-{}",
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
    if let Some(path) = std::env::var_os("LUAU_BIN") {
        let path = PathBuf::from(path);
        assert!(path.is_file());
        return Some(path);
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/runtime/0.741")
        .join(if cfg!(windows) { "luau.exe" } else { "luau" });
    if path.is_file() {
        Some(path)
    } else {
        assert!(
            std::env::var_os("REQUIRE_LUAU_RUNTIME").is_none(),
            "install Luau or set LUAU_BIN"
        );
        None
    }
}
fn run(runtime: &Path, path: &Path) -> Output {
    let mut process = Command::new(runtime)
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if process.try_wait().unwrap().is_some() {
            return process.wait_with_output().unwrap();
        }
        if start.elapsed() > Duration::from_secs(20) {
            let _ = process.kill();
            panic!("runtime timed out: {}", path.display());
        }
        thread::sleep(Duration::from_millis(5));
    }
}
fn configs(seed: u64) -> Vec<ObfuscationConfig> {
    let vm = ObfuscationConfig::default()
        .with_seed(seed)
        .with_level(ProtectionLevel::Paranoid);
    let mut cached = vm.clone();
    cached.vm_predecode_bytecode = true;
    cached.vm_pack_bytecode = false;
    cached.vm_pack_constants = false;
    let mut preserved = vm.clone();
    preserved.strip_types = false;
    preserved.rename_identifiers = false;
    vec![
        ObfuscationConfig::lightweight().with_seed(seed),
        ObfuscationConfig::default()
            .with_seed(seed)
            .with_level(ProtectionLevel::Maximum),
        vm,
        cached,
        preserved,
        ObfuscationConfig::hardened().with_seed(seed),
    ]
}
fn equivalent(
    runtime: &Path,
    workspace: &Workspace,
    source: &[u8],
    configs: Vec<ObfuscationConfig>,
) {
    let input = workspace.0.join("original.luau");
    // A UTF-8 BOM is an accepted input convenience, not part of executable Luau.
    fs::write(
        &input,
        source.strip_prefix(b"\xef\xbb\xbf").unwrap_or(source),
    )
    .unwrap();
    let expected = run(runtime, &input);
    assert!(
        expected.status.success(),
        "invalid original fixture: {}",
        String::from_utf8_lossy(&expected.stderr)
    );
    for (index, config) in configs.into_iter().enumerate() {
        let protected = Obfuscator::new(config)
            .obfuscate_bytes_with_report(source)
            .unwrap();
        assert_eq!(protected.report.input_bytes, source.len());
        let path = workspace.0.join(format!("protected-{index}.luau"));
        fs::write(&path, protected.code).unwrap();
        let actual = run(runtime, &path);
        assert!(
            actual.status.success(),
            "profile {index}: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "profile {index}");
    }
}

#[test]
fn numeric_for_coercion_freezes_values_and_preserves_signed_zero() {
    let Some(runtime) = runtime() else { return };
    let workspace = Workspace::new();
    let source = br#"
        local log={}
        local function bound(label,value)table.insert(log,label);return value end
        for i=bound('start','3'),bound('limit','-1'),bound('step','-2')do print(i,typeof(i))end
        print(table.concat(log,','))
        for _,value in {-0,0,'-0','0'} do for i=value,value do print(1/i)end end
        for _,step in {0,-0,0/0,math.huge,-math.huge,'2','-2'} do
            local n=0
            for i='1','4',step do n+=1;if n>4 then break end end
            print(n)
        end
        for _,args in {{'bad',2,1},{1,'bad',1},{1,2,'bad'},{false,2,1},{1,{},1}}do
            print(pcall(function()for i=args[1],args[2],args[3]do break end end))
        end
    "#;
    // Compare only success booleans for deliberately invalid controls; source
    // names and line numbers inside error messages necessarily differ.
    let source = String::from_utf8(source.to_vec()).unwrap().replace(
        "print(pcall(function()for i=args[1],args[2],args[3]do break end end))",
        "local ok=pcall(function()for i=args[1],args[2],args[3]do break end end);print(ok)",
    );
    for seed in [0, 42] {
        equivalent(&runtime, &workspace, source.as_bytes(), configs(seed));
    }
}

#[test]
fn windows_newlines_bom_and_unicode_surrogates_round_trip() {
    let Some(runtime) = runtime() else { return };
    let workspace = Workspace::new();
    let source = r#"
        local function bytes(s)local out={};for i=1,#s do out[i]=string.byte(s,i)end;print(table.concat(out,','))end
        bytes('start\z
             end')
        bytes(`escaped \
newline {17}`)
        bytes([=[
long
literal]=])
        bytes('\u{D800}');bytes('\u{DFFF}');bytes(`\u{D800}{42}`)
        bytes('\\u{D800}');bytes('\u{10FFFF}');bytes([[\u{D800}]])
        local function typed(value:number):number return value+1 end
        print(typed(2))
    "#;
    for newline in ["\n", "\r\n", "\r"] {
        let source = source.replace('\n', newline);
        equivalent(&runtime, &workspace, source.as_bytes(), configs(42));
    }
    let source = format!("\u{feff}{source}");
    equivalent(&runtime, &workspace, source.as_bytes(), configs(7));
}

#[test]
fn raw_binary_literals_comments_and_nested_interpolation_round_trip() {
    let Some(runtime) = runtime() else { return };
    let workspace = Workspace::new();
    let mut source = b"-- raw comment \xff\r\nlocal a='".to_vec();
    source.extend(128..=255);
    source.extend_from_slice(b"';local b=[==[\r\n\xff\r\n\xfe]==];local c=`\xfd {({value='\xfc'}).value} {`\xfb`}`;for _,s in {a,b,c}do local hash=0;for i=1,#s do hash=(hash*31+string.byte(s,i))%1000003 end;print(#s,hash)end");
    equivalent(&runtime, &workspace, &source, configs(42));
}

#[test]
fn helper_scope_keeps_near_limit_native_chunks_compilable() {
    let Some(runtime) = runtime() else { return };
    let workspace = Workspace::new();
    for count in [185, 195, 199] {
        let names = (0..count)
            .map(|i| format!("v{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let values = (0..count)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let source = format!(
            "local {names}={values};print(v0,v{},'protected string',`value={{v0}}`)",
            count - 1
        );
        equivalent(&runtime, &workspace, source.as_bytes(), configs(42));
    }
    let names = (0..200)
        .map(|i| format!("v{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let values = (0..200)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let source = format!("local {names}={values};print(v0,v199,'protected string')");
    let mut native = ObfuscationConfig::default()
        .with_level(ProtectionLevel::Maximum)
        .with_seed(42);
    native.wrap_in_iife = true;
    equivalent(&runtime, &workspace, source.as_bytes(), vec![native]);
}

#[test]
fn environment_mutation_and_aliases_keep_affected_functions_native() {
    let Some(runtime) = runtime() else { return };
    let workspace = Workspace::new();
    let source = br#"
        counter=10
        local function read()return counter end
        local original=read
        local env={counter=32}
        assert(setfenv(read,env)==read)
        print(read(),original(),counter,getfenv(read)==env)
        local access=_G['getfenv'];print(access(read)==env)
    "#;
    let config = ObfuscationConfig::default()
        .with_seed(42)
        .with_level(ProtectionLevel::Paranoid);
    let output = Obfuscator::new(config.clone())
        .obfuscate_bytes_with_report(source)
        .unwrap();
    assert_eq!(output.report.vm_functions, 0);
    assert!(output.report.vm_native_functions > 0);
    equivalent(&runtime, &workspace, source, vec![config]);
    let strict = Obfuscator::new(ObfuscationConfig::hardened().with_seed(42));
    assert!(strict.obfuscate_bytes_with_report(source).is_err());
    let shadowed =
        "local function f(debug,getfenv,setfenv)return debug+getfenv+setfenv end;print(f(1,2,3))";
    assert!(
        strict
            .obfuscate_with_report(shadowed)
            .unwrap()
            .report
            .vm_functions
            > 0
    );
}

#[test]
fn recursive_functions_do_not_inherit_other_prototypes_iteration_frames() {
    let Some(runtime) = runtime() else { return };
    let workspace = Workspace::new();
    let source=b"local function deep(n)if n>0 then return deep(n-1)else return 101 end end;for _,value in {1,2}do print(value)end;print(deep(10000))";
    let mut config = ObfuscationConfig::hardened().with_seed(42);
    config.vm_predecode_bytecode = true;
    equivalent(&runtime, &workspace, source, vec![config]);
}

#[test]
fn recursive_namecalls_fall_back_before_exhausting_the_stack() {
    let Some(runtime) = runtime() else { return };
    let workspace = Workspace::new();
    let source=b"local a={};function a:deep(n)if n>0 then return self:deep(n-1)else return 101 end end;print(a:deep(10000))";
    let mut config = ObfuscationConfig::default()
        .with_seed(42)
        .with_level(ProtectionLevel::Paranoid);
    config.vm_predecode_bytecode = true;
    let report = Obfuscator::new(config.clone())
        .obfuscate_bytes_with_report(source)
        .unwrap()
        .report;
    assert_eq!(report.vm_native_functions, 1);
    assert!(report
        .vm_fallback_reasons
        .contains_key("recursive native method calls"));
    equivalent(&runtime, &workspace, source, vec![config]);
}
