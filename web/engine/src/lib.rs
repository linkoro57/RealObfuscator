//! Browser bridge: validate explicit settings and use the same Rust pipeline as CLI.
use realobfuscator::{DeadCodeDensity, ObfuscationConfig, Obfuscator, ProtectionLevel};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    preset: String,
    seed: Option<String>,
    rename_identifiers: Option<bool>,
    encrypt_strings: Option<bool>,
    split_strings: Option<bool>,
    cache_strings: Option<bool>,
    obfuscate_numbers: Option<bool>,
    control_flow_flatten: Option<bool>,
    opaque_predicates: Option<bool>,
    inject_dead_code: Option<bool>,
    table_indirection: Option<bool>,
    anti_tamper: Option<bool>,
    virtualize: Option<bool>,
    virtualize_chunk: Option<bool>,
    vm_scatter_data: Option<bool>,
    vm_bind_strings: Option<bool>,
    vm_literal_programs: Option<bool>,
    vm_native_string_fallback: Option<bool>,
    require_bound_strings: Option<bool>,
    require_function_vm: Option<bool>,
    require_chunk_vm: Option<bool>,
    name_min_length: Option<usize>,
    name_max_length: Option<usize>,
    string_decode_mode: Option<String>,
    mba_depth: Option<u8>,
    skip_trivial_numbers: Option<bool>,
    min_blocks_for_cff: Option<usize>,
    dead_code_density: Option<String>,
    fake_function_count: Option<usize>,
    wrap_in_iife: Option<bool>,
    vm_hardening: Option<String>,
    vm_pack_bytecode: Option<bool>,
    vm_pack_constants: Option<bool>,
    vm_predecode_bytecode: Option<bool>,
    minify: Option<bool>,
    strip_comments: Option<bool>,
    strip_types: Option<bool>,
    build_id: Option<String>,
}

impl Settings {
    fn config(self) -> Result<ObfuscationConfig, String> {
        let mut config = match self.preset.as_str() {
            "" | "lightweight" => ObfuscationConfig::lightweight(),
            "rename" => {
                let mut c = ObfuscationConfig::lightweight();
                c.encrypt_strings = false;
                c
            }
            "native" => ObfuscationConfig::default().with_level(ProtectionLevel::Maximum),
            "vm" => ObfuscationConfig::default().with_level(ProtectionLevel::Paranoid),
            "hardened" => ObfuscationConfig::hardened(),
            "compatible" => ObfuscationConfig::compatible(),
            _ => return Err("Unknown preset".into()),
        };
        if let Some(seed) = self.seed.filter(|s| !s.trim().is_empty()) {
            config = config.with_seed(
                seed.trim()
                    .parse()
                    .map_err(|_| "Seed must be an unsigned 64-bit integer")?,
            );
        }
        macro_rules! apply { ($($field:ident),* $(,)?) => { $(if let Some(value)=self.$field { config.$field=value; })* }; }
        apply!(
            rename_identifiers,
            encrypt_strings,
            split_strings,
            cache_strings,
            obfuscate_numbers,
            control_flow_flatten,
            opaque_predicates,
            inject_dead_code,
            table_indirection,
            anti_tamper,
            virtualize,
            virtualize_chunk,
            vm_scatter_data,
            vm_bind_strings,
            vm_literal_programs,
            vm_native_string_fallback,
            require_bound_strings,
            require_function_vm,
            require_chunk_vm,
            name_min_length,
            name_max_length,
            string_decode_mode,
            mba_depth,
            skip_trivial_numbers,
            min_blocks_for_cff,
            fake_function_count,
            wrap_in_iife,
            vm_hardening,
            vm_pack_bytecode,
            vm_pack_constants,
            vm_predecode_bytecode,
            minify,
            strip_comments,
            strip_types,
            build_id
        );
        if let Some(density) = self.dead_code_density {
            config.dead_code_density = match density.as_str() {
                "low" => DeadCodeDensity::Low,
                "medium" => DeadCodeDensity::Medium,
                "high" => DeadCodeDensity::High,
                _ => return Err("Dead-code density must be low, medium or high".into()),
            };
        }
        config.validate()?;
        Ok(config)
    }
}

#[derive(Serialize)]
struct Response<'a> {
    code: &'a str,
    report: serde_json::Value,
    warnings: Vec<String>,
}

pub fn run(source: &str, json: &str) -> Result<String, String> {
    if source.len() > 1024 * 1024 {
        return Err("Browser limit: 1 MiB of source. Use the CLI for larger scripts.".into());
    }
    if source.trim().is_empty() {
        return Err("Add Lua or Luau source before obfuscating.".into());
    }
    let settings: Settings = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let config = settings.config()?;
    let output = Obfuscator::new(config)
        .obfuscate_with_report(source)
        .map_err(|e| e.to_string())?;
    let r = &output.report;
    let mut warnings = Vec::new();
    if let Some(reason) = &r.vm_chunk_fallback {
        warnings.push(format!("Main chunk remains native: {reason}"));
    }
    for (reason, count) in &r.vm_fallback_reasons {
        warnings.push(format!("{count} native function(s): {reason}"));
    }
    let report = serde_json::json!({
        "input_bytes":r.input_bytes,"output_bytes":r.output_bytes,"renamed_bindings":r.renamed_bindings,
        "encrypted_strings":r.encrypted_strings,"string_references":r.string_references,
        "split_strings":r.split_strings,"obfuscated_numbers":r.obfuscated_numbers,"hidden_fields":r.hidden_fields,
        "flattened_regions":r.flattened_regions,"virtualized_statements":r.virtualized_statements,
        "decoy_blocks":r.decoy_blocks,"opaque_predicates":r.opaque_predicates,"integrity_checks":r.integrity_checks,
        "vm_functions":r.vm_functions,"vm_chunks":r.vm_chunks,"vm_instructions":r.vm_instructions,
        "vm_opcode_variants":r.vm_opcode_variants,"vm_fused_instructions":r.vm_fused_instructions,
        "vm_native_functions":r.vm_native_functions,"vm_native_bridges":r.vm_native_bridges,
        "vm_iteration_bridges":r.vm_iteration_bridges,"vm_closure_bridges":r.vm_closure_bridges,
        "vm_bound_strings":r.vm_bound_strings,"vm_unbound_string_references":r.vm_unbound_string_references,
        "vm_literal_instructions":r.vm_literal_instructions,
        "vm_native_bound_strings":r.vm_native_bound_strings,"vm_native_literal_prototypes":r.vm_native_literal_prototypes,
        "vm_scattered_tables":r.vm_scattered_tables,"vm_scattered_entries":r.vm_scattered_entries,
        "vm_chunk_fallback":r.vm_chunk_fallback,"vm_fallback_reasons":r.vm_fallback_reasons
    });
    serde_json::to_string(&Response {
        code: &output.code,
        report,
        warnings,
    })
    .map_err(|e| e.to_string())
}

#[wasm_bindgen]
pub fn obfuscate(source: &str, settings: &str) -> Result<String, JsValue> {
    run(source, settings).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen]
pub fn decode_source(bytes: &[u8]) -> Result<String, JsValue> {
    realobfuscator::decode_source_bytes(bytes).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn identify(source: &str) -> Result<String, JsValue> {
    identify_script(source).map_err(|e| JsValue::from_str(&e))
}

pub fn identify_script(source: &str) -> Result<String, String> {
    let result = realobfuscator::detect_watermark(source).map_err(|e| e.0)?;
    serde_json::to_string(&serde_json::json!({
        "status": result.status(), "visible_header": result.visible_header,
        "embedded_watermark": result.embedded_watermark, "content_matches": result.content_matches,
        "authenticated": false
    }))
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detector_recognizes_output_without_executing_it() {
        let output = Obfuscator::new(ObfuscationConfig::lightweight().with_seed(42))
            .obfuscate("error('must never execute')")
            .unwrap();
        let report: serde_json::Value =
            serde_json::from_str(&identify_script(&output).unwrap()).unwrap();
        assert_eq!(report["status"], "matching_watermark");
        assert_eq!(report["authenticated"], false);
        assert_eq!(report["content_matches"], true);
        assert!(identify_script("local =").is_err());
    }
    #[test]
    fn compatible_binds_native_literals_and_reports_actual_coverage() {
        let source = "local function f()return getfenv(),'native fallback literal' end;print(f())";
        let options = r#"{"preset":"compatible","seed":"42"}"#;
        let value: serde_json::Value =
            serde_json::from_str(&run(source, options).unwrap()).unwrap();
        assert_eq!(value["report"]["vm_chunks"], 0);
        assert_eq!(value["report"]["vm_functions"], 0);
        assert_eq!(value["report"]["vm_native_literal_prototypes"], 1);
        assert!(value["report"]["vm_native_bound_strings"].as_u64().unwrap() > 0);
        assert_eq!(value["report"]["vm_unbound_string_references"], 0);
        assert!(!value["warnings"].as_array().unwrap().is_empty());
        assert_eq!(
            value["code"],
            Obfuscator::new(ObfuscationConfig::compatible().with_seed(42))
                .obfuscate(source)
                .unwrap()
        );
        assert!(run(source, r#"{"preset":"hardened"}"#)
            .unwrap_err()
            .contains("Compatible preset"));
    }

    #[test]
    fn bridge_matches_native_pipeline_and_keeps_u64_seeds_exact() {
        let source = "local n=17;return n,'hello'";
        let options = r#"{"preset":"hardened","seed":"18446744073709551615"}"#;
        let value: serde_json::Value =
            serde_json::from_str(&run(source, options).unwrap()).unwrap();
        let native = Obfuscator::new(ObfuscationConfig::hardened().with_seed(u64::MAX))
            .obfuscate(source)
            .unwrap();
        assert_eq!(value["code"], native);
        assert_eq!(value["report"]["vm_chunks"], 1);
    }
    #[test]
    fn invalid_settings_and_source_return_errors() {
        for settings in [
            r#"{"preset":"missing"}"#,
            r#"{"seed":"-1"}"#,
            r#"{"vm_scatter_data":true}"#,
            r#"{"unknown":true}"#,
        ] {
            assert!(run("return 1", settings).is_err());
        }
        assert!(run("local =", "{}").is_err());
        assert!(run(" ", "{}").is_err());
    }
}
