use realobfuscator::{
    DeadCodeDensity, ObfuscationConfig, ProtectionLevel, StringDecodeMode, VmHardening,
};

pub struct Profile {
    pub name: String,
    pub config: ObfuscationConfig,
}

type ConfigurationVariant = (&'static str, fn(&mut ObfuscationConfig));

pub fn all() -> Vec<Profile> {
    let light = ObfuscationConfig::lightweight().with_seed(42);
    let native = ObfuscationConfig::default()
        .with_seed(42)
        .with_level(ProtectionLevel::Maximum);
    let vm = ObfuscationConfig::default()
        .with_seed(42)
        .with_level(ProtectionLevel::Paranoid);
    let mut profiles = Vec::new();
    let mut add = |name: &str, config| {
        profiles.push(Profile {
            name: name.into(),
            config,
        })
    };
    let mut rename = light.clone();
    rename.encrypt_strings = false;
    add("rename", rename);
    add("lightweight", light.clone());
    add("native", native.clone());
    // Stable first eight entries form the standard suite; first four form quick.
    for (name, hardening, cached) in [
        ("vm-basic-cached-packed", VmHardening::Basic, true),
        ("vm-basic-rolling-packed", VmHardening::Basic, false),
        ("vm-max-cached-packed", VmHardening::Max, true),
        ("vm-max-rolling-packed", VmHardening::Max, false),
    ] {
        let mut config = vm.clone().with_vm_hardening(hardening);
        config.vm_predecode_bytecode = cached;
        add(name, config);
    }
    let mut no_integrity = native.clone();
    no_integrity.anti_tamper = false;
    add("native-no-integrity", no_integrity);
    for hardening in [VmHardening::Basic, VmHardening::ClientMax, VmHardening::Max] {
        for packed in [false, true] {
            for cached in [false, true] {
                for cache_strings in [false, true] {
                    let mut config = vm.clone().with_vm_hardening(hardening);
                    config.vm_pack_bytecode = packed;
                    config.vm_pack_constants = packed;
                    config.vm_predecode_bytecode = cached;
                    config.cache_strings = cache_strings;
                    let name = format!(
                        "vm-{}-{}-{}{}",
                        hardening.as_str(),
                        if cached { "cached" } else { "rolling" },
                        if packed { "packed" } else { "unpacked" },
                        if cache_strings { "-string-cache" } else { "" }
                    );
                    if !profiles.iter().any(|p| p.name == name) {
                        profiles.push(Profile { name, config });
                    }
                }
            }
        }
    }
    let variants: &[ConfigurationVariant] = &[
        ("native-no-strings", |c| c.encrypt_strings = false),
        ("native-no-numbers", |c| c.obfuscate_numbers = false),
        ("native-no-fields", |c| c.table_indirection = false),
        ("native-no-cff", |c| c.control_flow_flatten = false),
        ("native-no-decoys", |c| c.inject_dead_code = false),
        ("native-no-string-cache", |c| c.cache_strings = false),
        ("native-eager", |c| {
            c.string_decode_mode = StringDecodeMode::Eager.as_str().into()
        }),
        ("native-pretty", |c| c.minify = false),
        ("native-keep-types", |c| c.strip_types = false),
        ("native-high-density", |c| {
            c.dead_code_density = DeadCodeDensity::High
        }),
        ("native-low-density", |c| {
            c.dead_code_density = DeadCodeDensity::Low
        }),
        ("native-mba-1", |c| c.mba_depth = 1),
        ("native-mba-3", |c| c.mba_depth = 3),
    ];
    for (name, configure) in variants {
        let mut config = native.clone();
        configure(&mut config);
        profiles.push(Profile {
            name: (*name).into(),
            config,
        });
    }
    for (name, configure) in [
        (
            "vm-functions-only",
            (|c: &mut ObfuscationConfig| c.virtualize_chunk = false) as fn(&mut ObfuscationConfig),
        ),
        ("vm-no-integrity", |c: &mut ObfuscationConfig| {
            c.anti_tamper = false
        }),
        ("vm-no-renaming", |c: &mut ObfuscationConfig| {
            c.rename_identifiers = false
        }),
    ] {
        let mut config = vm.clone();
        configure(&mut config);
        profiles.push(Profile {
            name: name.into(),
            config,
        });
    }
    profiles
}
