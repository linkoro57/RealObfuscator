use crate::{
    config::ObfuscationConfig,
    constants::{Constants, Indirection},
    control_flow::{ControlFlow, ControlVisitor},
    function_vm::FunctionVm,
    lexer::{lex, Kind},
    names::{Names, Renamer},
    syntax,
};
use darklua_core::{
    nodes::*,
    process::{NodePostProcessor, NodePostVisitor, NodeProcessor, NodeVisitor, ScopeVisitor},
    rules::{ContextBuilder, RemoveTypes, Rule},
    Resources,
};
use std::{collections::BTreeMap, fmt};

#[derive(Debug)]
pub struct ObfuscationError(pub String);
impl fmt::Display for ObfuscationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Obfuscation failed: {}", self.0)
    }
}
impl std::error::Error for ObfuscationError {}

/// Counts applied transformations, including zero when a pass has no eligible input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObfuscationReport {
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub renamed_bindings: usize,
    pub encrypted_strings: usize,
    pub string_references: usize,
    pub obfuscated_numbers: usize,
    pub hidden_fields: usize,
    pub flattened_regions: usize,
    pub virtualized_statements: usize,
    pub decoy_blocks: usize,
    pub opaque_predicates: usize,
    pub integrity_checks: usize,
    pub vm_functions: usize,
    pub vm_chunks: usize,
    pub vm_chunk_fallback: Option<String>,
    pub vm_instructions: usize,
    pub vm_opcode_variants: usize,
    pub vm_fused_instructions: usize,
    pub vm_native_functions: usize,
    pub vm_native_bridges: usize,
    pub vm_iteration_bridges: usize,
    pub vm_closure_bridges: usize,
    pub vm_fallback_reasons: BTreeMap<String, usize>,
}

#[derive(Clone, Debug)]
pub struct ObfuscationOutput {
    pub code: String,
    pub report: ObfuscationReport,
}

pub struct Obfuscator {
    pub config: ObfuscationConfig,
}

// Type functions execute in the analyzer's separate environment. Runtime helpers
// must never be injected there when callers choose to preserve annotations.
struct RuntimeVisitor;
impl<T: NodeProcessor + NodePostProcessor> NodePostVisitor<T> for RuntimeVisitor {
    fn visit_type_function_statement(_: &mut TypeFunctionStatement, _: &mut T) {}
    fn visit_type(_: &mut Type, _: &mut T) {}
}
struct FieldVisitor;
impl<T: NodeProcessor> NodeVisitor<T> for FieldVisitor {
    fn visit_type_function(_: &mut TypeFunctionStatement, _: &mut T) {}
    fn visit_type(_: &mut Type, _: &mut T) {}
}

impl Obfuscator {
    pub fn new(config: ObfuscationConfig) -> Self {
        Self { config }
    }

    pub fn obfuscate(&self, source: &str) -> Result<String, ObfuscationError> {
        self.obfuscate_with_report(source).map(|output| output.code)
    }

    pub fn obfuscate_with_report(
        &self,
        source: &str,
    ) -> Result<ObfuscationOutput, ObfuscationError> {
        // The Luau parser and AST visitors recurse. Windows' default main-thread
        // stack is too small even for ordinary scripts in debug builds.
        stacker::maybe_grow(16 * 1024 * 1024, 32 * 1024 * 1024, || {
            self.obfuscate_inner(source)
        })
    }

    fn obfuscate_inner(&self, source: &str) -> Result<ObfuscationOutput, ObfuscationError> {
        self.config.validate().map_err(ObfuscationError)?;
        if self.config.build_id.contains(['\r', '\n']) {
            return Err(ObfuscationError(
                "build_id must not contain newlines".into(),
            ));
        }
        // Parsing comes before all mutations. Invalid source is never written as output.
        let mut block = syntax::parse(source)?;
        let tokens = lex(source).map_err(|e| ObfuscationError(e.to_string()))?;
        let mut report = ObfuscationReport {
            input_bytes: source.len(),
            ..Default::default()
        };
        let resources = Resources::from_memory();
        let context = ContextBuilder::new("input.luau", &resources, source).build();
        if self.config.strip_types {
            RemoveTypes::default()
                .process(&mut block, &context)
                .map_err(ObfuscationError)?;
        }
        // Domain-separate files: a fixed project seed is deterministic without reusing
        // the same names, pool keys and dispatch layout in every module.
        let source_hash = source
            .as_bytes()
            .iter()
            .fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
            });
        let mut root_rng = self.config.rng.clone();
        let rng = crate::rng::Rng::new(root_rng.next_u64() ^ source_hash);
        let mut names = Names::new(
            source,
            rng,
            self.config.name_min_length,
            self.config.name_max_length,
        );
        if self.config.rename_identifiers {
            let mut renamer = Renamer::new(&mut names);
            ScopeVisitor::visit_block(&mut block, &mut renamer);
            renamer.restore_type_functions(&mut block);
            report.renamed_bindings = renamer.count;
        }
        if self.config.table_indirection {
            let mut indirection = Indirection { count: 0 };
            FieldVisitor::visit_block(&mut block, &mut indirection);
            report.hidden_fields = indirection.count;
        }
        let mut vm_runner = None;
        let (mut vm_data, vm_runtime) = if self.config.virtualize {
            let mut compiler = FunctionVm::new(&self.config, &mut names, &mut report);
            vm_runner = Some(compiler.runner().to_owned());
            RuntimeVisitor::visit_block(&mut block, &mut compiler);
            compiler.compile_chunk(&mut block);
            compiler.finish()
        } else {
            (Block::default(), Block::default())
        };
        if self.config.require_function_vm && report.vm_native_functions != 0 {
            let reasons = report
                .vm_fallback_reasons
                .iter()
                .map(|(reason, count)| format!("{reason}: {count}"))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ObfuscationError(format!(
                "{} functions require native fallback ({reasons})",
                report.vm_native_functions
            )));
        }
        if self.config.require_chunk_vm && report.vm_chunks == 0 {
            return Err(ObfuscationError(format!(
                "main chunk requires native fallback ({})",
                report
                    .vm_chunk_fallback
                    .as_deref()
                    .unwrap_or("chunk compilation disabled")
            )));
        }
        let mut constants = Constants::new(&self.config, &mut names, &mut report);
        RuntimeVisitor::visit_block(&mut block, &mut constants);
        RuntimeVisitor::visit_block(&mut vm_data, &mut constants);
        let mut prefix = constants.prefix();
        for statement in vm_data.take_statements() {
            prefix.push_statement(statement);
        }
        for statement in vm_runtime.iter_statements().cloned() {
            prefix.push_statement(statement);
        }
        if self.config.control_flow_flatten || self.config.virtualize {
            let byte = names.fresh();
            let xor = names.fresh();
            let mix = names.fresh();
            let band = names.fresh();
            let fail = names.fresh();
            let mut pass = ControlFlow {
                config: &self.config,
                names: &mut names,
                report: &mut report,
                byte: byte.clone(),
                xor: xor.clone(),
                mix: mix.clone(),
                band: band.clone(),
                fail: fail.clone(),
                skip_runner: vm_runner.as_deref(),
            };
            ControlVisitor::visit_block(&mut block, &mut pass);
            if report.flattened_regions != 0 {
                // Capture runtime helpers before source declarations can shadow globals.
                let helpers = if self.config.virtualize {
                    format!("local {byte},{xor},{fail},{band}=string.byte,bit32.bxor,error,bit32.band;local {mix}=(function(x,l,r)return function(s)s=x(s,l(s,13));s=x(s,r(s,17));s=x(s,l(s,5));return s end end)(bit32.bxor,bit32.lshift,bit32.rshift);")
                } else if self.config.anti_tamper {
                    format!("local {fail}=error;")
                } else {
                    String::new()
                };
                for statement in syntax::generated(&helpers).take_statements() {
                    prefix.push_statement(statement);
                }
            }
        }
        let mut helper_renamer = Renamer::new(&mut names);
        helper_renamer.preserve_root_bindings(&prefix);
        ScopeVisitor::visit_block(&mut prefix, &mut helper_renamer);
        if let Some(error) = names.error {
            return Err(ObfuscationError(error));
        }
        let mut body = prefix.take_statements();
        body.extend(block.take_statements());
        block.set_statements(body);
        if self.config.wrap_in_iife {
            block = syntax::parse(&format!(
                "return(function(...) {} end)(...)",
                syntax::generate(&block, self.config.minify)
            ))?;
        }
        let body = syntax::generate(&block, self.config.minify);
        // Reparse the final artifact so generator/transform defects fail before writing.
        syntax::parse(&body)
            .map_err(|e| ObfuscationError(format!("generated output failed validation: {e}")))?;
        let mut code = String::from("--!nocheck\n--!nolint\n");
        for token in tokens
            .iter()
            .take_while(|t| matches!(t.kind, Kind::Comment | Kind::Directive))
        {
            if token.kind == Kind::Directive
                && matches!(
                    token.text.trim(),
                    "--!native" | "--!optimize 0" | "--!optimize 1" | "--!optimize 2"
                )
            {
                code.push_str(&token.text);
                code.push('\n');
            }
        }
        code.push_str(&format!(
            "-- RealObfuscator [{}] | https://github.com/linkoro57/RealObfuscator\n",
            self.config.build_id
        ));
        if !self.config.strip_comments {
            // Retain comment text, detached from source positions after AST rewriting.
            for token in &tokens {
                if token.kind == Kind::Comment && !token.text.starts_with("#!") {
                    code.push_str(&token.text);
                    code.push('\n');
                }
            }
        }
        code.push_str(&body);
        report.output_bytes = code.len();
        Ok(ObfuscationOutput { code, report })
    }
}

impl Default for Obfuscator {
    fn default() -> Self {
        Self::new(ObfuscationConfig::default())
    }
}
pub fn obfuscate(
    source: &str,
    config: Option<ObfuscationConfig>,
) -> Result<String, ObfuscationError> {
    Obfuscator::new(config.unwrap_or_default()).obfuscate(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_across_instances_and_repeated_calls() {
        let config = ObfuscationConfig::default().with_seed(42);
        let obfuscator = Obfuscator::new(config.clone());
        let source = "local x=17;return x,'hi'";
        let a = obfuscator.obfuscate(source).unwrap();
        assert_eq!(a, obfuscator.obfuscate(source).unwrap());
        assert_eq!(a, Obfuscator::new(config).obfuscate(source).unwrap());
    }
    #[test]
    fn parses_before_mutating_and_rejects_invalid_configuration() {
        assert!(Obfuscator::default().obfuscate("local = 17").is_err());
        let config = ObfuscationConfig {
            name_max_length: usize::MAX,
            ..Default::default()
        };
        assert!(Obfuscator::new(config).obfuscate("return 1").is_err());
    }

    #[test]
    fn preserving_types_keeps_type_functions_in_their_own_environment() {
        let source = "local x=7;type function identity(x) local y=x;return y end;print(x)";
        let original = syntax::parse(source).unwrap();
        let expected = original
            .iter_statements()
            .find(|s| matches!(s, Statement::TypeFunction(_)))
            .unwrap();
        let mut config = ObfuscationConfig::default().with_seed(9);
        config.strip_types = false;
        let output = Obfuscator::new(config).obfuscate(source).unwrap();
        let actual = syntax::parse(&output).unwrap();
        assert_eq!(
            Some(expected),
            actual
                .iter_statements()
                .find(|s| matches!(s, Statement::TypeFunction(_)))
        );
    }

    #[test]
    fn small_identifier_spaces_fail_instead_of_hanging_or_colliding() {
        let mut source = String::new();
        for i in 0..60 {
            source.push_str(&format!("local v{i}={i};"));
        }
        let mut config = ObfuscationConfig::lightweight().with_seed(0);
        config.name_min_length = 1;
        config.name_max_length = 1;
        let error = Obfuscator::new(config).obfuscate(&source).unwrap_err();
        assert!(error.to_string().contains("namespace exhausted"));
    }

    #[test]
    fn function_bytecode_reports_real_coverage_and_strict_fallbacks() {
        let mut config = ObfuscationConfig::default()
            .with_seed(33)
            .with_level(crate::ProtectionLevel::Paranoid);
        config.require_function_vm = true;
        config.require_chunk_vm = true;
        let source = "local function make(n)local function next(x)n+=x;return n*17 end;return next end;local f=make(2);print(f(3))";
        let output = Obfuscator::new(config.clone())
            .obfuscate_with_report(source)
            .unwrap();
        assert_eq!(output.report.vm_functions, 2);
        assert_eq!(output.report.vm_chunks, 1);
        assert_eq!(output.report.vm_native_functions, 0);
        assert!(output.report.vm_closure_bridges > 0);
        assert!(output.report.vm_instructions > 15);
        assert!(output.report.vm_fused_instructions > 0);
        assert!(output.report.vm_opcode_variants > 10);
        let error = Obfuscator::new(config.clone())
            .obfuscate("local function f()return getfenv(1)end;return f")
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("environment or stack introspection"));
        let error = Obfuscator::new(config)
            .obfuscate("return getfenv(1)")
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("main chunk requires native fallback"));
    }

    #[test]
    fn hardening_profiles_change_opcode_diversification() {
        let mut basic = ObfuscationConfig::default()
            .with_seed(9)
            .with_level(crate::ProtectionLevel::Paranoid);
        basic.vm_hardening = "basic".into();
        let mut max = basic.clone();
        max.vm_hardening = "max".into();
        let source = "local function f(x)return x*13+7 end;print(f(2))";
        let a = Obfuscator::new(basic)
            .obfuscate_with_report(source)
            .unwrap();
        let b = Obfuscator::new(max).obfuscate_with_report(source).unwrap();
        assert_eq!(b.report.vm_opcode_variants, a.report.vm_opcode_variants * 2);
        assert_ne!(a.code, b.code);
    }
}
