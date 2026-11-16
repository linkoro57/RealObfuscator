//! Runtime checks for autonomous, instruction-dependent literal reconstruction.
use realobfuscator::{ObfuscationConfig, Obfuscator, VmHardening};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "realobfuscator-literal-programs-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn runtime() -> Option<PathBuf> {
    let path = std::env::var_os("LUAU_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/runtime/0.741")
                .join(if cfg!(windows) { "luau.exe" } else { "luau" })
        });
    if path.is_file() {
        Some(path)
    } else {
        assert!(
            std::env::var_os("LUAU_BIN").is_none()
                && std::env::var_os("REQUIRE_LUAU_RUNTIME").is_none(),
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
            panic!("literal runtime deadline: {}", path.display())
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn every_byte_and_native_bridge_literals_survive_stateful_records_and_cache_variants() {
    let Some(runtime) = runtime() else { return };
    let workspace = Workspace::new();
    let bytes = (0..=255).map(|b| format!("\\{b:03}")).collect::<String>();
    let source = format!(
        r#"
        local function digest(s)
            local hash=0
            for i=1,#s do hash=(hash*31+string.byte(s,i))%1000003 end
            return #s,hash
        end
        local function values()return {{'{bytes}','','a','ab','abc','abcd','abcde','a long literal that must survive shuffled reconstruction and repeated access'}}end
        local receiver={{}}
        function receiver:identity(value)return value end
        for turn=1,3 do
            for _,s in values() do print(digest(s))end
            print(digest(receiver:identity('native bridge argument\000\255\128')))
        end
    "#
    );
    let original = workspace.0.join("original.luau");
    fs::write(&original, &source).unwrap();
    let expected = run(&runtime, &original);
    assert!(expected.status.success());
    for seed in [0, 42] {
        for packed in [false, true] {
            for cached in [false, true] {
                let mut config = ObfuscationConfig::hardened()
                    .with_seed(seed)
                    .with_vm_hardening(if seed == 0 {
                        VmHardening::Basic
                    } else {
                        VmHardening::Max
                    });
                config.vm_pack_bytecode = packed;
                config.vm_pack_constants = packed;
                config.vm_predecode_bytecode = cached;
                let protected = Obfuscator::new(config)
                    .obfuscate_with_report(&source)
                    .unwrap();
                assert!(protected.report.vm_literal_instructions > 100);
                assert!(protected.report.vm_bound_strings > 8);
                assert_eq!(protected.report.vm_unbound_string_references, 0);
                let path = workspace.0.join("protected.luau");
                fs::write(&path, protected.code).unwrap();
                let actual = run(&runtime, &path);
                assert!(
                    actual.status.success(),
                    "seed={seed}, packed={packed}, cached={cached}: {}",
                    String::from_utf8_lossy(&actual.stderr)
                );
                assert_eq!(actual.stdout, expected.stdout);
            }
        }
    }
}
