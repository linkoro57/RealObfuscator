//! Compile supported bodies transactionally: a failed candidate keeps its native body.
//!
//! The compiler emits logical instructions. Encoding assigns physical locations;
//! runtime generation consumes the same descriptor schema to read them back.
mod bridge;
mod compiler;
mod encoding;
mod instruction;
mod literals;
mod runtime;
mod scatter;
mod schema;

use self::{compiler::Program, instruction::Op, runtime::runtime, schema::Schema};
use crate::{
    config::{ObfuscationConfig, VmHardening},
    names::{Names, Renamer},
    obfuscator::ObfuscationReport,
    syntax,
};
use darklua_core::{
    nodes::*,
    process::{NodePostProcessor, NodeProcessor, NodeVisitor, ScopeVisitor},
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct FunctionVm<'a> {
    config: &'a ObfuscationConfig,
    hardening: VmHardening,
    names: &'a mut Names,
    report: &'a mut ObfuscationReport,
    runner: String,
    bank: String,
    programs: Vec<(u32, Program)>,
    schema: Schema,
    decoder: String,
}

impl<'a> FunctionVm<'a> {
    pub fn runner(&self) -> &str {
        &self.runner
    }
    pub fn new(
        config: &'a ObfuscationConfig,
        hardening: VmHardening,
        names: &'a mut Names,
        report: &'a mut ObfuscationReport,
    ) -> Self {
        let runner = names.fresh();
        let bank = names.fresh();
        let schema = if config.vm_scatter_data {
            Schema::sparse(&mut names.rng)
        } else {
            Schema::new(&mut names.rng)
        };
        let decoder = if config.vm_bind_strings {
            names.fresh()
        } else {
            String::new()
        };
        Self {
            config,
            hardening,
            names,
            report,
            runner,
            bank,
            programs: Vec::new(),
            schema,
            decoder,
        }
    }
    fn compile(&mut self, block: &mut Block, parameters: Vec<String>, variadic: bool) {
        self.compile_target(block, parameters, variadic, false);
    }
    pub fn compile_chunk(&mut self, block: &mut Block) {
        if !self.config.virtualize_chunk {
            return;
        }
        if self
            .report
            .vm_fallback_reasons
            .contains_key("environment or stack introspection")
        {
            self.report.vm_chunk_fallback =
                Some("source contains native stack/environment introspection".into());
            return;
        }
        self.compile_target(block, Vec::new(), true, true);
    }
    fn compile_target(
        &mut self,
        block: &mut Block,
        parameters: Vec<String>,
        variadic: bool,
        chunk: bool,
    ) {
        let mut program =
            match compiler::compile(block, &parameters, self.names.fresh(), self.names.fresh()) {
                Ok(program) => program,
                Err(reason) => {
                    if chunk {
                        self.report.vm_chunk_fallback = Some(reason.to_owned());
                    } else {
                        self.report.vm_native_functions += 1;
                        *self
                            .report
                            .vm_fallback_reasons
                            .entry(reason.to_owned())
                            .or_default() += 1;
                    }
                    return;
                }
            };
        let id = loop {
            let id = self.names.rng.range(1, 1 << 24) as u32;
            if self.programs.iter().all(|(existing, _)| *existing != id) {
                break id;
            }
        };
        if self.config.vm_bind_strings {
            literals::bind_native_strings(
                &mut program,
                &self.bank,
                id,
                &self.decoder,
                &self.schema,
            );
        }
        let external = program.external.join(",");
        let args = parameters.join(",");
        *block = syntax::generated(&format!(
            "return {}({}[{id}],{{{external}}},{{{args}}}{})",
            self.runner,
            self.bank,
            if variadic { ",..." } else { "" }
        ));
        if chunk {
            self.report.vm_chunks += 1;
        } else {
            self.report.vm_functions += 1;
        }
        self.report.vm_instructions += program.code.len();
        self.report.vm_fused_instructions += program.fused;
        self.report.vm_native_bridges += program.native_bridges;
        self.report.vm_iteration_bridges += program.iteration_bridges;
        self.report.vm_closure_bridges += program.closure_bridges;
        self.report.virtualized_statements += program.statements;
        self.programs.push((id, program));
    }

    /// Build shared immutable descriptors and a diversified interpreter, separately
    /// so literal protection can process data without recursively virtualizing helpers.
    pub fn finish(self) -> (Block, Block) {
        if self.programs.is_empty() {
            return (Block::default(), Block::default());
        }
        let ops: BTreeSet<_> = self
            .programs
            .iter()
            .flat_map(|(_, p)| p.code.iter().map(|i| i.op.clone()))
            .collect();
        let aliases = self.hardening.opcode_aliases();
        let mut taken = BTreeSet::new();
        let mut codes = BTreeMap::<Op, Vec<u32>>::new();
        for op in ops {
            let mut ids = Vec::new();
            for _ in 0..aliases {
                let id = loop {
                    let id = self.names.rng.range(257, 65536) as u32;
                    if taken.insert(id) {
                        break id;
                    }
                };
                ids.push(id);
            }
            codes.insert(op, ids);
        }
        self.report.vm_opcode_variants += taken.len();
        let mut bank = Vec::with_capacity(self.programs.len());
        for (id, program) in &self.programs {
            let values = encoding::descriptor(
                program,
                &codes,
                self.config.vm_pack_bytecode,
                self.config.vm_bind_strings,
                &mut self.names.rng,
            );
            if self.config.vm_bind_strings {
                let bound = program
                    .constants
                    .iter()
                    .filter(|value| matches!(value, Expression::String(_)))
                    .count();
                self.report.vm_bound_strings += bound;
                if self.config.anti_tamper {
                    self.report.integrity_checks += bound;
                }
            }
            let fields = values
                .into_iter()
                .map(|(field, value)| {
                    TableIndexEntry::new(self.schema.slot(field) as f64, value).into()
                })
                .collect();
            bank.push(TableIndexEntry::new(*id as f64, TableExpression::new(fields)).into());
            if self.config.anti_tamper {
                self.report.integrity_checks += program.code.len();
            }
        }
        let bank = if self.config.vm_scatter_data {
            scatter::emit(
                &self.bank,
                TableExpression::new(bank),
                self.names,
                self.report,
            )
        } else {
            Block::from(
                VariableAssignment::from_variable(self.bank.as_str())
                    .with_value(TableExpression::new(bank)),
            )
        };
        let runtime = runtime(
            &self.runner,
            &self.schema,
            &codes,
            self.config,
            self.hardening,
            &self.decoder,
        );
        let mut runtime = syntax::generated(&runtime);
        let mut renamer = Renamer::new(self.names);
        renamer.preserve_name(&self.runner);
        if self.config.vm_bind_strings {
            renamer.preserve_name(&self.decoder);
        }
        ScopeVisitor::visit_block(&mut runtime, &mut renamer);
        (bank, runtime)
    }
}

impl NodeProcessor for FunctionVm<'_> {}
impl NodePostProcessor for FunctionVm<'_> {
    fn process_after_local_function_statement(&mut self, function: &mut FunctionAssignment) {
        let parameters = function
            .iter_parameters()
            .map(|p| p.get_name().to_owned())
            .collect();
        let variadic = function.is_variadic();
        self.compile(function.mutate_block(), parameters, variadic);
    }
    fn process_after_function_expression(&mut self, function: &mut FunctionExpression) {
        let parameters = function
            .iter_parameters()
            .map(|p| p.get_name().to_owned())
            .collect();
        let variadic = function.is_variadic();
        self.compile(function.mutate_block(), parameters, variadic);
    }
    fn process_after_function_statement(&mut self, function: &mut FunctionStatement) {
        let mut parameters = Vec::new();
        if function.get_name().has_method() {
            parameters.push("self".to_owned());
        }
        parameters.extend(function.iter_parameters().map(|p| p.get_name().to_owned()));
        let variadic = function.is_variadic();
        self.compile(function.mutate_block(), parameters, variadic);
    }
}
