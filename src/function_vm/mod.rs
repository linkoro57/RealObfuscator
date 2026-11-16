//! Compile supported bodies transactionally: a failed candidate keeps its native body.
//!
//! The compiler emits logical instructions. Encoding assigns physical locations;
//! runtime generation consumes the same descriptor schema to read them back.
mod bridge;
mod compiler;
mod encoding;
mod instruction;
mod literal_programs;
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
    rules::{ContextBuilder, ConvertFunctionToAssign, FlawlessRule},
    Resources,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn environment_sensitive(block: &mut Block) -> bool {
    let mut capture = bridge::Capture {
        parameter: String::new(),
        bindings: Default::default(),
        masks: Vec::new(),
        cells: Default::default(),
        unsupported: false,
        nested_closure: false,
        introspection: false,
    };
    ScopeVisitor::visit_block(block, &mut capture);
    capture.introspection
}

fn recursive_method(function: &FunctionStatement) -> bool {
    let Some(method) = function.get_name().get_method() else {
        return false;
    };
    struct Search<'a> {
        method: &'a str,
        found: bool,
    }
    impl NodeProcessor for Search<'_> {
        fn process_function_call(&mut self, call: &mut FunctionCall) {
            if call
                .get_method()
                .is_some_and(|m| m.get_name() == self.method)
                && matches!(call.get_prefix(),Prefix::Identifier(id) if id.get_name()=="self")
            {
                self.found = true;
            }
        }
    }
    let mut search = Search {
        method: method.get_name(),
        found: false,
    };
    darklua_core::process::DefaultVisitor::visit_block(
        &mut function.get_block().clone(),
        &mut search,
    );
    search.found
}

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
    environment_sensitive: bool,
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
        environment_sensitive: bool,
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
            environment_sensitive,
        }
    }
    fn compile(
        &mut self,
        block: &mut Block,
        parameters: Vec<String>,
        variadic: bool,
    ) -> Option<Expression> {
        self.compile_target(block, parameters, variadic, false)
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
    ) -> Option<Expression> {
        if self.environment_sensitive {
            let reason = "environment or stack introspection";
            if chunk {
                self.report.vm_chunk_fallback = Some(reason.into());
            } else {
                self.report.vm_native_functions += 1;
                *self
                    .report
                    .vm_fallback_reasons
                    .entry(reason.into())
                    .or_default() += 1;
            }
            return None;
        }
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
                    return None;
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
        if self.config.vm_literal_programs {
            match literal_programs::prepare(&mut program, &mut self.names.rng) {
                Ok(count) => self.report.vm_literal_instructions += count,
                Err(reason) => {
                    if chunk {
                        self.report.vm_chunk_fallback = Some(reason.into());
                    } else {
                        self.report.vm_native_functions += 1;
                        *self
                            .report
                            .vm_fallback_reasons
                            .entry(reason.into())
                            .or_default() += 1;
                    }
                    return None;
                }
            }
        }
        let external = program.external.join(",");
        let bridged_iteration = program
            .code
            .iter()
            .any(|instruction| instruction.op == Op::GenericFor);
        let factory = format!(
            "{}({}[{id}],{{{external}}},{},{variadic},{bridged_iteration})",
            self.runner,
            self.bank,
            parameters.len()
        );
        let replacement = if chunk || !self.config.strip_types {
            let mut args = parameters;
            if variadic {
                args.push("...".into());
            }
            *block = syntax::generated(&format!("return {factory}({})", args.join(",")));
            None
        } else {
            let expression = syntax::generated(&format!("return {factory}"));
            let Some(LastStatement::Return(ret)) = expression.get_last_statement() else {
                unreachable!()
            };
            let value = ret.iter_expressions().next().unwrap().clone();
            Some(value)
        };
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
        replacement
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
    fn process_after_expression(&mut self, expression: &mut Expression) {
        if let Expression::Function(function) = expression {
            let parameters = function
                .iter_parameters()
                .map(|p| p.get_name().to_owned())
                .collect();
            let variadic = function.is_variadic();
            if let Some(factory) = self.compile(function.mutate_block(), parameters, variadic) {
                *expression = factory;
            }
        }
    }
    fn process_after_block(&mut self, block: &mut Block) {
        let mut output = Vec::new();
        for statement in block.take_statements() {
            match statement {
                Statement::LocalFunction(mut function) => {
                    let parameters = function
                        .iter_parameters()
                        .map(|p| p.get_name().to_owned())
                        .collect();
                    let variadic = function.is_variadic();
                    if let Some(factory) =
                        self.compile(function.mutate_block(), parameters, variadic)
                    {
                        let name = function.get_name().to_owned();
                        output.push(VariableAssignment::from_variable(name.as_str()).into());
                        output.push(
                            AssignStatement::new(vec![Variable::new(name)], vec![factory]).into(),
                        );
                    } else {
                        output.push(Statement::LocalFunction(function));
                    }
                }
                Statement::Function(mut function) => {
                    if !self.environment_sensitive && recursive_method(&function) {
                        self.report.vm_native_functions += 1;
                        *self
                            .report
                            .vm_fallback_reasons
                            .entry("recursive native method calls".into())
                            .or_default() += 1;
                        output.push(Statement::Function(function));
                    } else if !self.config.strip_types {
                        let mut parameters = Vec::new();
                        if function.get_name().has_method() {
                            parameters.push("self".into());
                        }
                        parameters
                            .extend(function.iter_parameters().map(|p| p.get_name().to_owned()));
                        let variadic = function.is_variadic();
                        self.compile(function.mutate_block(), parameters, variadic);
                        output.push(Statement::Function(function));
                    } else {
                        let mut converted = Block::from(Statement::Function(function));
                        let resources = Resources::from_memory();
                        let context = ContextBuilder::new("function.luau", &resources, "").build();
                        ConvertFunctionToAssign::default()
                            .flawless_process(&mut converted, &context);
                        for mut statement in converted.take_statements() {
                            if let Statement::Assign(assignment) = &mut statement {
                                for value in assignment.iter_mut_values() {
                                    self.process_after_expression(value);
                                }
                            }
                            output.push(statement);
                        }
                    }
                }
                other => output.push(other),
            }
        }
        block.set_statements(output);
    }
}
