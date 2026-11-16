//! Lower Luau AST nodes into logical register instructions.
use super::{
    bridge::{Bridge, Capture},
    instruction::{Instruction, Op},
};
use crate::syntax;
use darklua_core::{
    nodes::*,
    process::{DefaultVisitor, NodeVisitor, ScopeVisitor},
    rules::{ContextBuilder, ConvertFunctionToAssign, FlawlessRule},
    Resources,
};
use std::collections::{BTreeSet, HashMap};

#[derive(Default)]
struct Loop {
    breaks: Vec<usize>,
    continues: Vec<usize>,
}
enum Location {
    Register(u32),
    External(u32),
    Index(u32, u32),
}

/// A complete candidate, published only after its entire body compiles.
/// Registers are one-based; branch targets remain zero-based logical offsets.
/// The final empty return makes every fallthrough target valid before relocation.
pub(super) struct Program {
    pub code: Vec<Instruction>,
    pub constants: Vec<Expression>,
    pub external: Vec<String>,
    pub statements: usize,
    pub fused: usize,
    pub native_bridges: usize,
    pub iteration_bridges: usize,
    pub closure_bridges: usize,
}

struct Compiler {
    code: Vec<Instruction>,
    constants: Vec<Expression>,
    number_ids: HashMap<u64, u32>,
    string_ids: HashMap<Vec<u8>, u32>,
    nil_id: Option<u32>,
    boolean_ids: [Option<u32>; 2],
    external: Vec<String>,
    external_ids: HashMap<String, u32>,
    scopes: Vec<HashMap<String, u32>>,
    registers: u32,
    loops: Vec<Loop>,
    statements: usize,
    fused: usize,
    bridge_parameter: String,
    native_bridges: usize,
    iteration_bridges: usize,
    callback_parameter: String,
    closure_bridges: usize,
}

type CompileResult<T> = Result<T, &'static str>;

/// Compile without touching the input AST; errors discard all candidate state.
pub(super) fn compile(
    block: &Block,
    parameters: &[String],
    bridge_parameter: String,
    callback_parameter: String,
) -> CompileResult<Program> {
    let mut compiler = Compiler::new(parameters, bridge_parameter, callback_parameter);
    compiler.block(block, true)?;
    let empty = compiler.empty_list();
    compiler.emit(Op::Return, empty, 0, 0);
    // Cells are needed only when a bridge can retain or access the register map.
    // Pure VM bodies keep bindings directly in registers: the same instruction
    // positions and branch targets, without per-binding table allocations.
    if !compiler
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, Op::Closure | Op::Native | Op::GenericFor))
    {
        for instruction in &mut compiler.code {
            if matches!(instruction.op, Op::Bind | Op::CellGet | Op::CellSet) {
                instruction.op = Op::Move;
            }
        }
    }
    Ok(Program {
        code: compiler.code,
        constants: compiler.constants,
        external: compiler.external,
        statements: compiler.statements,
        fused: compiler.fused,
        native_bridges: compiler.native_bridges,
        iteration_bridges: compiler.iteration_bridges,
        closure_bridges: compiler.closure_bridges,
    })
}

impl Compiler {
    fn new(parameters: &[String], bridge_parameter: String, callback_parameter: String) -> Self {
        let mut compiler = Self {
            code: Vec::new(),
            constants: Vec::new(),
            number_ids: HashMap::new(),
            string_ids: HashMap::new(),
            nil_id: None,
            boolean_ids: [None; 2],
            external: Vec::new(),
            external_ids: HashMap::new(),
            scopes: vec![HashMap::new()],
            registers: 0,
            loops: Vec::new(),
            statements: 0,
            fused: 0,
            bridge_parameter,
            native_bridges: 0,
            iteration_bridges: 0,
            callback_parameter,
            closure_bridges: 0,
        };
        for parameter in parameters {
            let register = compiler.bind(parameter);
            compiler.emit(Op::Bind, register, register, 0);
        }
        compiler
    }
    fn reg(&mut self) -> u32 {
        self.registers += 1;
        self.registers
    }
    fn bind(&mut self, name: &str) -> u32 {
        let register = self.reg();
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.to_owned(), register);
        register
    }
    fn resolve(&self, name: &str) -> Option<u32> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }
    fn emit(&mut self, op: Op, a: u32, b: u32, c: u32) -> usize {
        let index = self.code.len();
        self.code.push(Instruction { op, a, b, c });
        index
    }
    fn patch(&mut self, index: usize, target: usize) {
        let instruction = &mut self.code[index];
        if instruction.op == Op::Jump {
            instruction.a = target as u32;
        } else {
            instruction.b = target as u32;
        }
    }
    fn jump(&mut self) -> usize {
        self.emit(Op::Jump, 0, 0, 0)
    }
    fn constant_id(&mut self, value: Expression) -> u32 {
        // Index literals by their exact runtime value. Number bits retain the
        // distinction between signed zeroes; strings are keyed by decoded bytes.
        // Only generated capture-list tables need structural AST comparison.
        let existing = match &value {
            Expression::Number(number) => self
                .number_ids
                .get(&number.compute_value().to_bits())
                .copied(),
            Expression::String(string) => self.string_ids.get(string.get_value()).copied(),
            Expression::Nil(_) => self.nil_id,
            Expression::False(_) => self.boolean_ids[0],
            Expression::True(_) => self.boolean_ids[1],
            _ => self
                .constants
                .iter()
                .position(|existing| *existing == value)
                .map(|index| index as u32 + 1),
        };
        if let Some(id) = existing {
            return id;
        }
        let id = self.constants.len() as u32 + 1;
        match &value {
            Expression::Number(number) => {
                self.number_ids.insert(number.compute_value().to_bits(), id);
            }
            Expression::String(string) => {
                self.string_ids.insert(string.get_value().to_vec(), id);
            }
            Expression::Nil(_) => self.nil_id = Some(id),
            Expression::False(_) => self.boolean_ids[0] = Some(id),
            Expression::True(_) => self.boolean_ids[1] = Some(id),
            _ => {}
        }
        self.constants.push(value);
        id
    }
    fn constant(&mut self, value: Expression) -> u32 {
        let id = self.constant_id(value);
        let register = self.reg();
        self.emit(Op::Constant, register, id, 0);
        register
    }
    fn external_code(&mut self, key: String, code: String) -> u32 {
        if let Some(id) = self.external_ids.get(&key) {
            return *id;
        }
        self.external.push(code);
        let id = self.external.len() as u32;
        self.external_ids.insert(key, id);
        id
    }
    fn external(&mut self, name: &str) -> CompileResult<u32> {
        // Native stack/environment introspection is intentionally left native.
        if matches!(name, "getfenv" | "setfenv" | "debug") {
            return Err("environment or stack introspection");
        }
        let value = &self.bridge_parameter;
        let write = &self.callback_parameter;
        Ok(self.external_code(name.to_owned(), format!("function({value},{write})if {write} then {name}={value} else return {name} end end")))
    }
    fn read(&mut self, name: &str) -> CompileResult<u32> {
        if let Some(register) = self.resolve(name) {
            // Freeze the value now; later argument evaluation may assign the binding.
            let copy = self.reg();
            self.emit(Op::CellGet, copy, register, 0);
            return Ok(copy);
        }
        let external = self.external(name)?;
        let register = self.reg();
        self.emit(Op::Read, register, external, 0);
        Ok(register)
    }
    fn prefix(&mut self, prefix: &Prefix) -> CompileResult<u32> {
        self.expr(&prefix.clone().into())
    }
    fn literal(expression: &Expression) -> bool {
        matches!(
            expression,
            Expression::Number(_)
                | Expression::String(_)
                | Expression::Nil(_)
                | Expression::True(_)
                | Expression::False(_)
        )
    }
    fn expr(&mut self, expression: &Expression) -> CompileResult<u32> {
        Ok(match expression {
            value if Self::literal(value) => self.constant(value.clone()),
            Expression::Identifier(identifier) => self.read(identifier.get_name())?,
            Expression::Parenthese(parentheses) => self.expr(parentheses.inner_expression())?,
            Expression::TypeCast(cast) => self.expr(cast.get_expression())?,
            Expression::TypeInstantiation(instantiation) => {
                self.prefix(instantiation.get_prefix())?
            }
            Expression::VariableArguments(_) => {
                let result = self.reg();
                self.emit(Op::Vararg, result, 0, 0);
                result
            }
            Expression::Unary(unary) => {
                let value = self.expr(unary.get_expression())?;
                let result = self.reg();
                self.emit(Op::Unary(unary.operator().to_str()), result, value, 0);
                result
            }
            Expression::Binary(binary) => {
                let left = self.expr(binary.left())?;
                let result = self.reg();
                let operator = binary.operator();
                if matches!(operator, BinaryOperator::And | BinaryOperator::Or) {
                    self.emit(Op::Move, result, left, 0);
                    let jump = self.emit(
                        if operator == BinaryOperator::And {
                            Op::JumpFalse
                        } else {
                            Op::JumpTrue
                        },
                        left,
                        0,
                        0,
                    );
                    let right = self.expr(binary.right())?;
                    self.emit(Op::Move, result, right, 0);
                    self.patch(jump, self.code.len());
                } else if Self::literal(binary.right()) {
                    let right = self.constant_id(binary.right().clone());
                    self.emit(Op::BinaryConstant(operator.to_str()), result, left, right);
                    self.fused += 1;
                } else {
                    let right = self.expr(binary.right())?;
                    self.emit(Op::Binary(operator.to_str()), result, left, right);
                }
                result
            }
            Expression::Call(call) => self.call(call, false)?,
            Expression::Index(index) => {
                let prefix = self.prefix(index.get_prefix())?;
                let key = self.expr(index.get_index())?;
                let result = self.reg();
                self.emit(Op::Index, result, prefix, key);
                result
            }
            Expression::Field(field) => {
                let prefix = self.prefix(field.get_prefix())?;
                let key = self.constant(
                    StringExpression::from_value(field.get_field().get_name().as_str()).into(),
                );
                let result = self.reg();
                self.emit(Op::Index, result, prefix, key);
                result
            }
            Expression::Table(table) => {
                let result = self.reg();
                let array_hint = table
                    .iter_entries()
                    .filter(|entry| matches!(entry, TableEntry::Value(_)))
                    .count() as u32;
                self.emit(Op::Table, result, array_hint, 0);
                let mut array_index = 1;
                let entries = table.get_entries();
                for (i, entry) in entries.iter().enumerate() {
                    match entry {
                        TableEntry::Field(field) => {
                            let key = self.constant(
                                StringExpression::from_value(field.get_field().get_name().as_str())
                                    .into(),
                            );
                            let value = self.expr(field.get_value())?;
                            self.emit(Op::SetIndex, result, key, value);
                        }
                        TableEntry::Index(index) => {
                            let key = self.expr(index.get_key())?;
                            let value = self.expr(index.get_value())?;
                            self.emit(Op::SetIndex, result, key, value);
                        }
                        TableEntry::Value(value)
                            if i + 1 == entries.len() && Self::expands(value) =>
                        {
                            let values = self.many(value)?;
                            self.emit(Op::TableExtend, result, values, array_index);
                        }
                        TableEntry::Value(value) => {
                            let key = self.constant((array_index as f64).into());
                            let value = self.expr(value)?;
                            self.emit(Op::SetIndex, result, key, value);
                            array_index += 1;
                        }
                    }
                }
                result
            }
            Expression::If(branches) => {
                let result = self.reg();
                let mut exits = Vec::new();
                let mut conditions = vec![(branches.get_condition(), branches.get_result())];
                conditions.extend(
                    branches
                        .iter_branches()
                        .map(|branch| (branch.get_condition(), branch.get_result())),
                );
                for (condition, value) in conditions {
                    let condition = self.expr(condition)?;
                    let next = self.emit(Op::JumpFalse, condition, 0, 0);
                    let value = self.expr(value)?;
                    self.emit(Op::Move, result, value, 0);
                    exits.push(self.jump());
                    self.patch(next, self.code.len());
                }
                let value = self.expr(branches.get_else_result())?;
                self.emit(Op::Move, result, value, 0);
                for exit in exits {
                    self.patch(exit, self.code.len());
                }
                result
            }
            Expression::InterpolatedString(string) => {
                let mut format = Vec::new();
                let mut values = Vec::new();
                for segment in string.iter_segments() {
                    match segment {
                        InterpolationSegment::String(segment) => {
                            for &byte in segment.get_value() {
                                format.push(byte);
                                if byte == b'%' {
                                    format.push(byte);
                                }
                            }
                        }
                        InterpolationSegment::Value(segment) => {
                            format.extend_from_slice(b"%*");
                            values.push(segment.get_expression().clone());
                        }
                    }
                }
                values.insert(0, StringExpression::from_value(format).into());
                let arguments = self.values(&values)?;
                let result = self.reg();
                self.emit(Op::Format, result, arguments, 0);
                result
            }
            Expression::Function(function) => self.closure(function)?,
            _ => return Err("unsupported expression"),
        })
    }
    fn expands(value: &Expression) -> bool {
        matches!(
            value,
            Expression::Call(_) | Expression::VariableArguments(_)
        )
    }
    fn many(&mut self, value: &Expression) -> CompileResult<u32> {
        match value {
            Expression::Call(call) => self.call(call, true),
            Expression::VariableArguments(_) => {
                let register = self.reg();
                self.emit(Op::VarargMany, register, 0, 0);
                Ok(register)
            }
            _ => unreachable!(),
        }
    }
    fn empty_list(&mut self) -> u32 {
        let list = self.reg();
        self.emit(Op::List, list, 0, 0);
        list
    }

    fn values(&mut self, values: &[Expression]) -> CompileResult<u32> {
        let list = self.empty_list();
        for (i, value) in values.iter().enumerate() {
            if i + 1 == values.len() && Self::expands(value) {
                let value = self.many(value)?;
                self.emit(Op::Extend, list, value, 0);
            } else {
                let value = self.expr(value)?;
                self.emit(Op::Push, list, value, 0);
            }
        }
        Ok(list)
    }
    fn call(&mut self, call: &FunctionCall, many: bool) -> CompileResult<u32> {
        if call.has_method() {
            // Keep the complete colon call native, including argument evaluation.
            // Extracting a method or delaying NAMECALL until after arguments changes
            // lookup order and can break Roblox userdata behavior.
            let mut wrapper = Block::from(ReturnStatement::one(call.clone()));
            let resources = Resources::from_memory();
            let context = ContextBuilder::new("method.luau", &resources, "").build();
            ConvertFunctionToAssign::default().flawless_process(&mut wrapper, &context);
            let bindings = self
                .scopes
                .iter()
                .flat_map(|scope| {
                    scope
                        .iter()
                        .map(|(name, register)| (name.clone(), *register))
                })
                .collect();
            let mut capture = Capture {
                parameter: self.bridge_parameter.clone(),
                bindings,
                masks: Vec::new(),
                cells: BTreeSet::new(),
                unsupported: false,
                nested_closure: false,
                introspection: false,
            };
            ScopeVisitor::visit_block(&mut wrapper, &mut capture);
            if capture.introspection {
                return Err("environment or stack introspection");
            }
            if capture.unsupported {
                return Err("type functions in method bridges");
            }
            let body = syntax::generate(&wrapper, true);
            // Escaping callbacks must retain the cells from this invocation, not
            // a live register map whose loop-local cells can later be replaced.
            // Snapshot cell references only: assignments stay shared with the VM.
            let factory = if capture.nested_closure {
                let cells = capture
                    .cells
                    .iter()
                    .map(|cell| format!("[{cell}]={}[{cell}]", self.callback_parameter))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "function({},...)local {}={{{cells}}};{body} end",
                    self.callback_parameter, self.bridge_parameter
                )
            } else {
                format!("function({},...){body} end", self.bridge_parameter)
            };
            let external = self.external_code(format!("method:{}", self.external.len()), factory);
            let result = self.reg();
            self.emit(Op::Native, result, external, u32::from(many));
            self.native_bridges += 1;
            return Ok(result);
        }
        let function = self.prefix(call.get_prefix())?;
        let values = match call.get_arguments() {
            Arguments::Tuple(tuple) => tuple.iter_values().cloned().collect(),
            Arguments::String(string) => vec![string.clone().into()],
            Arguments::Table(table) => vec![table.clone().into()],
        };
        let arguments = self.values(&values)?;
        let result = self.reg();
        self.emit(
            if many { Op::CallMany } else { Op::Call },
            result,
            function,
            arguments,
        );
        Ok(result)
    }
    fn location(&mut self, variable: &Variable) -> CompileResult<Location> {
        Ok(match variable {
            Variable::Identifier(name) => {
                if let Some(register) = self.resolve(name.get_name()) {
                    Location::Register(register)
                } else {
                    Location::External(self.external(name.get_name())?)
                }
            }
            Variable::Index(index) => Location::Index(
                self.prefix(index.get_prefix())?,
                self.expr(index.get_index())?,
            ),
            Variable::Field(field) => {
                let prefix = self.prefix(field.get_prefix())?;
                let key = self.constant(
                    StringExpression::from_value(field.get_field().get_name().as_str()).into(),
                );
                Location::Index(prefix, key)
            }
        })
    }
    fn closure(&mut self, function: &FunctionExpression) -> CompileResult<u32> {
        let mut wrapper = Block::from(ReturnStatement::one(function.clone()));
        let resources = Resources::from_memory();
        let context = ContextBuilder::new("closure.luau", &resources, "").build();
        ConvertFunctionToAssign::default().flawless_process(&mut wrapper, &context);
        let bindings = self
            .scopes
            .iter()
            .flat_map(|scope| {
                scope
                    .iter()
                    .map(|(name, register)| (name.clone(), *register))
            })
            .collect();
        let mut capture = Capture {
            parameter: self.bridge_parameter.clone(),
            bindings,
            masks: Vec::new(),
            cells: BTreeSet::new(),
            unsupported: false,
            nested_closure: false,
            introspection: false,
        };
        ScopeVisitor::visit_block(&mut wrapper, &mut capture);
        if capture.unsupported {
            return Err("type functions in closure factories");
        }
        if capture.introspection {
            return Err("environment or stack introspection");
        }
        let rendered = syntax::generate(&wrapper, true);
        let factory = format!("function({}){rendered} end", self.bridge_parameter);
        let external = self.external_code(format!("closure:{}", self.external.len()), factory);
        let captures = TableExpression::new(
            capture
                .cells
                .into_iter()
                .map(|cell| TableEntry::from_value(cell as f64))
                .collect(),
        );
        let captures = self.constant(captures.into());
        let result = self.reg();
        self.emit(Op::Closure, result, external, captures);
        self.closure_bridges += 1;
        Ok(result)
    }
    fn store(&mut self, location: &Location, value: u32) {
        match *location {
            Location::Register(register) => {
                self.emit(Op::CellSet, register, value, 0);
            }
            Location::External(id) => {
                self.emit(Op::Write, id, value, 0);
            }
            Location::Index(table, key) => {
                self.emit(Op::SetIndex, table, key, value);
            }
        }
    }
    fn block(&mut self, block: &Block, scope: bool) -> CompileResult<()> {
        if scope {
            self.scopes.push(HashMap::new());
        }
        for statement in block.iter_statements() {
            self.statements += 1;
            match statement {
                Statement::LocalAssign(assign) => {
                    let values = self.values(&assign.iter_values().cloned().collect::<Vec<_>>())?;
                    for (index, variable) in assign.iter_variables().enumerate() {
                        let register = self.bind(variable.get_name());
                        let value = self.reg();
                        self.emit(Op::Item, value, values, index as u32 + 1);
                        self.emit(Op::Bind, register, value, 0);
                    }
                }
                Statement::Assign(assign) => {
                    let locations = assign
                        .iter_variables()
                        .map(|variable| self.location(variable))
                        .collect::<CompileResult<Vec<_>>>()?;
                    let values = self.values(&assign.iter_values().cloned().collect::<Vec<_>>())?;
                    // Values and addresses are frozen before the ordered writes.
                    for (index, location) in locations.iter().enumerate() {
                        let value = self.reg();
                        self.emit(Op::Item, value, values, index as u32 + 1);
                        self.store(location, value);
                    }
                }
                Statement::CompoundAssign(assign) => {
                    let location = self.location(assign.get_variable())?;
                    let previous = self.reg();
                    match location {
                        Location::Register(register) => {
                            self.emit(Op::CellGet, previous, register, 0);
                        }
                        Location::External(id) => {
                            self.emit(Op::Read, previous, id, 0);
                        }
                        Location::Index(table, key) => {
                            self.emit(Op::Index, previous, table, key);
                        }
                    }
                    let right = self.expr(assign.get_value())?;
                    let result = self.reg();
                    self.emit(
                        Op::Binary(assign.get_operator().to_binary_operator().to_str()),
                        result,
                        previous,
                        right,
                    );
                    self.store(&location, result);
                }
                Statement::Call(call) => {
                    self.call(call, false)?;
                }
                Statement::Do(statement) => self.block(statement.get_block(), true)?,
                Statement::If(statement) => {
                    let mut exits = Vec::new();
                    for branch in statement.iter_branches() {
                        let condition = self.expr(branch.get_condition())?;
                        let next = self.emit(Op::JumpFalse, condition, 0, 0);
                        self.block(branch.get_block(), true)?;
                        exits.push(self.jump());
                        self.patch(next, self.code.len());
                    }
                    if let Some(otherwise) = statement.get_else_block() {
                        self.block(otherwise, true)?;
                    }
                    for exit in exits {
                        self.patch(exit, self.code.len());
                    }
                }
                Statement::While(statement) => {
                    let start = self.code.len();
                    let condition = self.expr(statement.get_condition())?;
                    let end = self.emit(Op::JumpFalse, condition, 0, 0);
                    self.loops.push(Loop::default());
                    self.block(statement.get_block(), true)?;
                    self.emit(Op::Jump, start as u32, 0, 0);
                    self.patch(end, self.code.len());
                    self.finish_loop(start);
                }
                Statement::Repeat(statement) => {
                    self.scopes.push(HashMap::new());
                    let start = self.code.len();
                    self.loops.push(Loop::default());
                    self.block(statement.get_block(), false)?;
                    let continuation = self.code.len();
                    let condition = self.expr(statement.get_condition())?;
                    self.emit(Op::JumpFalse, condition, start as u32, 0);
                    self.finish_loop(continuation);
                    self.scopes.pop();
                }
                Statement::NumericFor(statement) => {
                    // Evaluate loop bounds before introducing the loop binding.
                    let state = self.reg();
                    self.emit(Op::List, state, 0, 0);
                    let start = self.expr(statement.get_start())?;
                    self.emit(Op::Push, state, start, 0);
                    let limit = self.expr(statement.get_end())?;
                    self.emit(Op::Push, state, limit, 0);
                    let step = if let Some(step) = statement.get_step() {
                        self.expr(step)?
                    } else {
                        self.constant(1.0.into())
                    };
                    self.emit(Op::Push, state, step, 0);
                    self.emit(Op::ForPrepare, state, 0, 0);
                    self.scopes.push(HashMap::new());
                    let index = self.bind(statement.get_identifier().get_name());
                    let start = self.code.len();
                    let condition = self.reg();
                    self.emit(Op::ForCheck, condition, state, 0);
                    let end = self.emit(Op::JumpFalse, condition, 0, 0);
                    let value = self.reg();
                    self.emit(Op::Item, value, state, 1);
                    self.emit(Op::Bind, index, value, 0);
                    self.loops.push(Loop::default());
                    self.block(statement.get_block(), true)?;
                    let continuation = self.code.len();
                    self.emit(Op::ForNext, state, 0, 0);
                    self.emit(Op::Jump, start as u32, 0, 0);
                    self.patch(end, self.code.len());
                    self.finish_loop(continuation);
                    self.scopes.pop();
                }
                Statement::TypeDeclaration(_) | Statement::TypeFunction(_) => {
                    return Err("preserved type declarations")
                }
                Statement::GenericFor(statement) => {
                    let bindings = self
                        .scopes
                        .iter()
                        .flat_map(|scope| {
                            scope
                                .iter()
                                .map(|(name, register)| (name.clone(), *register))
                        })
                        .collect();
                    let mut bridge = Bridge {
                        parameter: self.bridge_parameter.clone(),
                        bindings,
                        nested_closure: false,
                        introspection: false,
                    };
                    let mut expressions: Vec<_> = statement.iter_expressions().cloned().collect();
                    for expression in &mut expressions {
                        DefaultVisitor::visit_expression(expression, &mut bridge);
                    }
                    if bridge.introspection {
                        return Err("environment or stack introspection");
                    }
                    if bridge.nested_closure {
                        return Err("nested closures in iterator expressions");
                    }
                    let return_values = ReturnStatement::new(expressions);
                    let rendered = syntax::generate(&Block::from(return_values), true);
                    let iterators = rendered.strip_prefix("return").unwrap().trim();
                    self.scopes.push(HashMap::new());
                    let mut stores = String::new();
                    let mut identifiers = Vec::new();
                    for identifier in statement.iter_identifiers() {
                        let name = identifier.get_name();
                        let register = self.bind(name);
                        stores.push_str(&format!(
                            "{}[{register}]={{{name}}};",
                            self.bridge_parameter
                        ));
                        identifiers.push(name.as_str());
                    }
                    let bridge_source = format!("function({},{},...)for {} in {iterators} do {stores}local signal,values={}();if signal==1 then break elseif signal==2 then return signal,values end end;return 0 end", self.bridge_parameter, self.callback_parameter, identifiers.join(","), self.callback_parameter);
                    let external = self
                        .external_code(format!("iterator:{}", self.external.len()), bridge_source);
                    let enter = self.emit(Op::GenericFor, 0, external, 0);
                    let body_start = self.code.len();
                    self.code[enter].a = body_start as u32;
                    self.loops.push(Loop::default());
                    self.block(statement.get_block(), true)?;
                    let continuation = self.code.len();
                    self.emit(Op::Signal, 0, 0, 0);
                    let exit = self.code.len();
                    self.emit(Op::Signal, 1, 0, 0);
                    let frame = self.loops.pop().unwrap();
                    for index in frame.breaks {
                        self.patch(index, exit);
                    }
                    for index in frame.continues {
                        self.patch(index, continuation);
                    }
                    self.code[enter].c = self.code.len() as u32;
                    self.scopes.pop();
                    self.iteration_bridges += 1;
                }
                Statement::LocalFunction(function) => {
                    let register = self.bind(function.get_name());
                    let nil = self.constant(Expression::nil());
                    self.emit(Op::Bind, register, nil, 0);
                    let mut expression = FunctionExpression::default();
                    expression.set_variadic(function.is_variadic());
                    *expression.mutate_parameters() = function.get_parameters().clone();
                    *expression.mutate_block() = function.get_block().clone();
                    let value = self.closure(&expression)?;
                    self.emit(Op::CellSet, register, value, 0);
                }
                Statement::Function(function) => {
                    let mut converted = Block::from(Statement::Function(function.clone()));
                    let resources = Resources::from_memory();
                    let context = ContextBuilder::new("function.luau", &resources, "").build();
                    ConvertFunctionToAssign::default().flawless_process(&mut converted, &context);
                    self.statements -= 1;
                    self.block(&converted, false)?;
                }
            }
        }
        if let Some(last) = block.get_last_statement() {
            match last {
                LastStatement::Return(ret) => {
                    let values =
                        self.values(&ret.iter_expressions().cloned().collect::<Vec<_>>())?;
                    self.emit(Op::Return, values, 0, 0);
                }
                LastStatement::Break(_) => {
                    let index = self.jump();
                    self.loops
                        .last_mut()
                        .ok_or("break outside loop")?
                        .breaks
                        .push(index);
                }
                LastStatement::Continue(_) => {
                    let index = self.jump();
                    self.loops
                        .last_mut()
                        .ok_or("continue outside loop")?
                        .continues
                        .push(index);
                }
            }
        }
        if scope {
            self.scopes.pop();
        }
        if self.code.len() > 20000 || self.registers > 65535 {
            return Err("function size limit");
        }
        Ok(())
    }
    fn finish_loop(&mut self, continuation: usize) {
        let frame = self.loops.pop().unwrap();
        for index in frame.breaks {
            self.patch(index, self.code.len());
        }
        for index in frame.continues {
            self.patch(index, continuation);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_registers_are_used_only_when_no_bridge_can_observe_cells() {
        let body = syntax::parse("local n=1;for i=1,5 do n+=i end;return n").unwrap();
        let pure = compile(&body, &[], "bridge".into(), "callback".into()).unwrap();
        assert!(!pure
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::Bind | Op::CellGet | Op::CellSet)));
        for source in [
            "local n=1;return function()n+=1;return n end",
            "local n=1;receiver:save(function()return n end)",
            "local n=1;for _,value in {2,3} do n+=value end;return n",
        ] {
            let body = syntax::parse(source).unwrap();
            let bridged = compile(&body, &[], "bridge".into(), "callback".into()).unwrap();
            assert!(
                bridged
                    .code
                    .iter()
                    .any(|instruction| instruction.op == Op::Bind),
                "{source}"
            );
        }
    }

    #[test]
    fn literal_index_preserves_signed_zero_nil_false_and_binary_strings() {
        let mut compiler = Compiler::new(&[], "bridge".into(), "callback".into());
        let zero = compiler.constant_id(0.0.into());
        let negative_zero = compiler.constant_id((-0.0).into());
        assert_ne!(zero, negative_zero);
        assert_eq!(zero, compiler.constant_id(0.0.into()));
        assert_eq!(negative_zero, compiler.constant_id((-0.0).into()));
        let nil = compiler.constant_id(Expression::nil());
        let false_value = compiler.constant_id(Expression::False(None));
        assert_ne!(nil, false_value);
        assert_eq!(nil, compiler.constant_id(Expression::nil()));
        assert_eq!(false_value, compiler.constant_id(Expression::False(None)));
        let binary: Expression = StringExpression::from_value(vec![0, 255, b'1']).into();
        let id = compiler.constant_id(binary.clone());
        assert_eq!(id, compiler.constant_id(binary));
        for number in 1..=4096 {
            let id = compiler.constant_id((number as f64).into());
            assert_eq!(id, compiler.constant_id((number as f64).into()));
        }
        assert_eq!(compiler.constants.len(), 4096 + 5);
    }
}
