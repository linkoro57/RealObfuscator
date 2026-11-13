use realobfuscator::{DeadCodeDensity, ObfuscationConfig, Obfuscator, ProtectionLevel};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
static NEXT_WORKSPACE: AtomicU64 = AtomicU64::new(0);
impl Workspace {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "realobfuscator-runtime-{}-{unique}-{}",
            std::process::id(),
            NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed)
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
        assert!(path.is_file(), "LUAU_BIN must name an executable file");
        return Some(path);
    }
    let name = if cfg!(windows) { "luau.exe" } else { "luau" };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/runtime/0.741")
        .join(name);
    if path.is_file() {
        Some(path)
    } else {
        assert!(
            std::env::var_os("REQUIRE_LUAU_RUNTIME").is_none(),
            "install Luau or set LUAU_BIN to run required differential tests"
        );
        eprintln!("SKIP: Luau executable missing; set LUAU_BIN (REQUIRE_LUAU_RUNTIME=1 makes this an error)");
        None
    }
}

fn run(runtime: &Path, path: &Path) -> Output {
    // A broken dispatcher must fail the test, rather than hang the entire suite.
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
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!(
                "runtime timeout for {}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn profiles() -> Vec<ObfuscationConfig> {
    let mut rename = ObfuscationConfig::lightweight().with_seed(42);
    rename.encrypt_strings = false;
    let mut profiles = vec![
        rename,
        ObfuscationConfig::lightweight().with_seed(42),
        ObfuscationConfig::default()
            .with_seed(42)
            .with_level(ProtectionLevel::Maximum),
    ];
    for packed in [false, true] {
        for cached in [false, true] {
            let mut config = ObfuscationConfig::default()
                .with_seed(42)
                .with_level(ProtectionLevel::Paranoid);
            config.vm_pack_bytecode = packed;
            config.vm_pack_constants = packed;
            config.vm_predecode_bytecode = cached;
            profiles.push(config);
        }
    }
    let mut unrenamed = ObfuscationConfig::default()
        .with_seed(13)
        .with_level(ProtectionLevel::Paranoid);
    unrenamed.rename_identifiers = false;
    unrenamed.minify = false;
    unrenamed.anti_tamper = false;
    unrenamed.dead_code_density = DeadCodeDensity::High;
    profiles.push(unrenamed);
    let mut typed = ObfuscationConfig::default()
        .with_seed(7)
        .with_level(ProtectionLevel::Maximum);
    typed.strip_types = false;
    typed.skip_trivial_numbers = false;
    typed.string_decode_mode = "eager".into();
    profiles.push(typed);
    let mut plain_strings = ObfuscationConfig::default()
        .with_seed(8)
        .with_level(ProtectionLevel::Maximum);
    plain_strings.encrypt_strings = false;
    profiles.push(plain_strings);
    profiles
}

const CASES: &[(&str, &str)] = &[
    (
        "scopes",
        r#"
        value=91
        local x=4
        local a,b=x,7
        local before=function() return x end
        local x=x+1
        x=x+2
        local function fact(n) if n==0 then return 1 end return n*fact(n-1) end
        local function f(value) value=value+3;return value end
        do local x=88;x=x+1;print(x)end
        print(x,before(),a,b,value,f(3),fact(6))
        local self=23
        local t={value=5}
        function t:plus(n) self.value=self.value+n;return self.value end
        print(t:plus(3),self,t.value)
    "#,
    ),
    (
        "loops",
        r#"
        local i=7
        local sum=0
        for i=i,9 do sum+=i end
        local callbacks={}
        for k,v in {3,5,8} do callbacks[k]=function()return v end end
        repeat local next=sum-1;sum=next until next<20
        for j=1,5 do
            if j==2 then continue end
            if j==5 then break end
            sum+=j;sum*=2;sum-=1
        end
        local function withBreak()
            local j=0
            while true do j+=1;if j==2 then break end end
            return j
        end
        print(i,sum,callbacks[1](),callbacks[3](),withBreak())
    "#,
    ),
    (
        "strings",
        r#"
        local data={'', 'repeat', "repeat", '\000\255\128\001', '\u{1F680}', 'a\z  \n  b', '\x41\065'}
        local long=[==[
line one
line two
]==]
        for i,s in data do print(i,#s,string.byte(s,1,#s))end
        print(#long, string.byte(long,1,#long))
        local x=7
        print(`score {x} 100% {'nested'} {`inside {x+1}`}`)
        print 'sugar call'
        local lines='one\
two'
        print(#lines, string.byte(lines,1,#lines))
        local t=setmetatable({}, {__tostring=function() return 'object' end})
        print(`{t}:{nil}:{true}`)
    "#,
    ),
    (
        "multivalue",
        r#"
        type Result<T> = {
            value: T,
            callback: (number, ...string) -> (number, string?)
        }
        export type Alias = Result<number>
        local function values(...: any): ...any return ... end
        local function f<T>(x: T, ...: any): (T, ...any) return x, ... end
        local a,b,c=values(1,nil,3)
        local obj: Result<number>?=nil
        local function cast() return values(4,5)::number end
        print(select('#',values(1,nil,3)),a,b,c,f('value',2,nil,4))
        print(select('#',cast()),cast(),obj)
        print((if a==1 then 'yes' else 'no'), 7//2, 2^3^2, 1 .. 'tail')
    "#,
    ),
    (
        "dispatch",
        r#"
        local c,i,hash=5,6,7
        local string,bit32,table,error='shadow','shadow','shadow','shadow'
        local x=0
        x+=1;x*=2;x-=3;x+=4
        c+=x;i+=c;hash+=i
        print(x,c,i,hash,string,bit32,table,error)
        local thread=coroutine.create(function()
            x+=1;coroutine.yield(x);x*=3;coroutine.yield(x);x-=4
            return x
        end)
        print(coroutine.resume(thread));print(coroutine.resume(thread));print(coroutine.resume(thread))
    "#,
    ),
    (
        "numbers",
        r#"
        print(0,-0,1,-1,4294967295,4294967296,9007199254740991,9007199254740992)
        print(0xff_ff,0b1010_1111,.5,1.25,1e-9,1e309,1/(-0),-2^2)
        local x=17; x=x+1;x=x*3;x=x-7;print(x)
    "#,
    ),
    (
        "environment",
        r#"
        local x=0
        local functions={}
        local function cb() return x end
        local t=setmetatable({}, {__index=function(_,k) print('read',k);return 2 end,__newindex=function(_,k,v) print('write',k,v) end})
        x=t.foo; t.foo=x+2; x=t.foo
        for j=1,3 do
            x+=j
            functions[j]=function() return x,j end
            x+=2
            x-=1
        end
        print(cb(), functions[1](),functions[3]())
        print(getfenv(1)==getfenv(0))
    "#,
    ),
];

#[test]
fn all_profiles_preserve_runtime_behavior() {
    let Some(runtime) = runtime() else {
        return;
    };
    let workspace = Workspace::new();
    for (name, source) in CASES {
        let input = workspace.0.join(format!("{name}.luau"));
        fs::write(&input, source).unwrap();
        let expected = run(&runtime, &input);
        assert!(
            expected.status.success(),
            "invalid fixture {name}: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        for (i, config) in profiles().into_iter().enumerate() {
            let output = Obfuscator::new(config)
                .obfuscate_with_report(source)
                .unwrap();
            let path = workspace.0.join(format!("{name}-{i}.luau"));
            fs::write(&path, &output.code).unwrap();
            let actual = run(&runtime, &path);
            assert!(
                actual.status.success(),
                "{name} profile {i}: {}\n{}",
                String::from_utf8_lossy(&actual.stderr),
                output.code
            );
            assert_eq!(expected.stdout, actual.stdout, "{name} profile {i}");
        }
    }
}

#[test]
fn seeds_diversify_dispatch_and_runtime_results_are_stable() {
    let Some(runtime) = runtime() else {
        return;
    };
    let workspace = Workspace::new();
    let source = "local x=7;x+=1;x*=5;x-=3;print(x,'protected')";
    let mut previous = None;
    for seed in 0..16 {
        let mut config = ObfuscationConfig::default()
            .with_seed(seed)
            .with_level(ProtectionLevel::Paranoid);
        config.name_min_length = 1;
        config.name_max_length = 3;
        let output = Obfuscator::new(config)
            .obfuscate_with_report(source)
            .unwrap();
        assert!(output.report.virtualized_statements >= 4);
        assert!(output.report.integrity_checks >= 2);
        assert!(!output.code.contains("protected"));
        assert_ne!(previous.as_ref(), Some(&output.code));
        let path = workspace.0.join(format!("seed-{seed}.luau"));
        fs::write(&path, &output.code).unwrap();
        let actual = run(&runtime, &path);
        assert!(
            actual.status.success(),
            "seed {seed}: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&actual.stdout).replace('\r', ""),
            "37\tprotected\n"
        );
        previous = Some(output.code);
    }
}

#[test]
fn string_payload_tampering_fails_before_the_source_runs() {
    use darklua_core::{
        nodes::StringExpression,
        process::{DefaultVisitor, NodeProcessor, NodeVisitor},
    };
    struct Corrupt(bool);
    impl NodeProcessor for Corrupt {
        fn process_string_expression(&mut self, string: &mut StringExpression) {
            if !self.0 && !string.get_value().is_empty() {
                let mut bytes = string.get_value().to_vec();
                bytes[0] ^= 1;
                *string = StringExpression::from_value(bytes);
                self.0 = true;
            }
        }
    }
    let Some(runtime) = runtime() else {
        return;
    };
    let workspace = Workspace::new();
    let mut config = ObfuscationConfig::lightweight().with_seed(4);
    config.anti_tamper = true;
    config.string_decode_mode = "lazy".into();
    let output = Obfuscator::new(config)
        .obfuscate("print('integrity protected')")
        .unwrap();
    let mut block = darklua_core::Parser::default().parse(&output).unwrap();
    let mut corruption = Corrupt(false);
    DefaultVisitor::visit_block(&mut block, &mut corruption);
    assert!(corruption.0);
    use darklua_core::generator::{DenseLuaGenerator, LuaGenerator};
    let mut generator = DenseLuaGenerator::default();
    generator.write_block(&block);
    let path = workspace.0.join("tampered.luau");
    fs::write(&path, generator.into_string()).unwrap();
    let actual = run(&runtime, &path);
    assert!(!actual.status.success());
    assert!(
        actual.stdout.is_empty(),
        "tampered string must fail before print"
    );
}

#[test]
fn large_regions_are_chunked_and_keep_their_order() {
    let Some(runtime) = runtime() else {
        return;
    };
    let workspace = Workspace::new();
    let mut source = String::from("local x=0;");
    for i in 1..=300 {
        source.push_str(&format!("x+={i};"));
    }
    source.push_str("print(x)");
    for level in [ProtectionLevel::Maximum, ProtectionLevel::Paranoid] {
        let config = ObfuscationConfig::default()
            .with_seed(321)
            .with_level(level);
        let output = Obfuscator::new(config)
            .obfuscate_with_report(&source)
            .unwrap();
        assert_eq!(output.report.flattened_regions, 3);
        let path = workspace.0.join(format!("large-{}.luau", level as u8));
        fs::write(&path, output.code).unwrap();
        let actual = run(&runtime, &path);
        assert!(
            actual.status.success(),
            "{}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&actual.stdout).replace('\r', ""),
            "45150\n"
        );
    }
}
