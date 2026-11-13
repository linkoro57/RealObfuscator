//! Compile supported function bodies to a private register instruction set.
//! Compilation is transactional: unsupported constructs retain their native body.
use crate::{
    config::ObfuscationConfig,
    names::{Names, Renamer},
    obfuscator::ObfuscationReport,
    syntax,
};
use darklua_core::{
    nodes::*,
    process::{DefaultVisitor, NodePostProcessor, NodeProcessor, NodeVisitor, Scope, ScopeVisitor},
    rules::{ContextBuilder, ConvertFunctionToAssign, FlawlessRule},
    Resources,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Op {
    Constant,
    Move,
    Read,
    Write,
    Index,
    SetIndex,
    Table,
    TableExtend,
    List,
    Push,
    Extend,
    Item,
    Call,
    CallMany,
    Vararg,
    VarargMany,
    Return,
    Jump,
    JumpFalse,
    JumpTrue,
    Binary(&'static str),
    BinaryConstant(&'static str),
    Unary(&'static str),
    ForCheck,
    ForNext,
    Format,
    Native,
    GenericFor,
    Signal,
    Bind,
    CellGet,
    CellSet,
    Closure,
}

#[derive(Clone)]
struct Instruction {
    op: Op,
    a: u32,
    b: u32,
    c: u32,
}
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

struct Compiler {
    code: Vec<Instruction>,
    constants: Vec<Expression>,
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

impl Compiler {
    fn new(parameters: &[String], bridge_parameter: String, callback_parameter: String) -> Self {
        let mut compiler = Self {
            code: Vec::new(),
            constants: Vec::new(),
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
        if let Some(index) = self
            .constants
            .iter()
            .position(|existing| *existing == value)
        {
            return index as u32 + 1;
        }
        self.constants.push(value);
        self.constants.len() as u32
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
    fn values(&mut self, values: &[Expression]) -> CompileResult<u32> {
        let list = self.reg();
        self.emit(Op::List, list, 0, 0);
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
        let arguments = self.reg();
        self.emit(Op::List, arguments, 0, 0);
        for (i, value) in values.iter().enumerate() {
            if i + 1 == values.len() && Self::expands(value) {
                let value = self.many(value)?;
                self.emit(Op::Extend, arguments, value, 0);
            } else {
                let value = self.expr(value)?;
                self.emit(Op::Push, arguments, value, 0);
            }
        }
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

pub(crate) struct FunctionVm<'a> {
    config: &'a ObfuscationConfig,
    names: &'a mut Names,
    report: &'a mut ObfuscationReport,
    runner: String,
    bank: String,
    programs: Vec<(u32, Compiler)>,
    schema: Vec<usize>,
}

impl<'a> FunctionVm<'a> {
    pub fn runner(&self) -> &str {
        &self.runner
    }
    pub fn new(
        config: &'a ObfuscationConfig,
        names: &'a mut Names,
        report: &'a mut ObfuscationReport,
    ) -> Self {
        let runner = names.fresh();
        let bank = names.fresh();
        let mut schema: Vec<_> = (1..=12).collect();
        names.rng.shuffle(&mut schema);
        Self {
            config,
            names,
            report,
            runner,
            bank,
            programs: Vec::new(),
            schema,
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
        let mut compiler = Compiler::new(&parameters, self.names.fresh(), self.names.fresh());
        if let Err(reason) = compiler.block(block, true) {
            if chunk {
                self.report.vm_chunk_fallback = Some(reason.to_owned());
                return;
            }
            self.report.vm_native_functions += 1;
            *self
                .report
                .vm_fallback_reasons
                .entry(reason.to_owned())
                .or_default() += 1;
            return;
        }
        let empty = compiler.values(&[]).unwrap();
        compiler.emit(Op::Return, empty, 0, 0);
        let id = loop {
            let id = self.names.rng.range(1, 1 << 24) as u32;
            if self.programs.iter().all(|(existing, _)| *existing != id) {
                break id;
            }
        };
        let external = compiler.external.join(",");
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
        self.report.vm_instructions += compiler.code.len();
        self.report.vm_fused_instructions += compiler.fused;
        self.report.vm_native_bridges += compiler.native_bridges;
        self.report.vm_iteration_bridges += compiler.iteration_bridges;
        self.report.vm_closure_bridges += compiler.closure_bridges;
        self.report.virtualized_statements += compiler.statements;
        self.programs.push((id, compiler));
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
        let aliases = match self.config.vm_hardening.as_str() {
            "max" => 4,
            "client-max" => 3,
            _ => 2,
        };
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
        let mut bank = format!("local {}={{", self.bank);
        for (id, program) in &self.programs {
            let mut order: Vec<_> = (0..program.code.len()).collect();
            self.names.rng.shuffle(&mut order);
            let mut positions = vec![0u32; program.code.len() + 1];
            for (physical, &logical) in order.iter().enumerate() {
                positions[logical] = physical as u32 + 1;
            }
            let mut layout: Vec<_> = (0..5).collect();
            self.names.rng.shuffle(&mut layout);
            let seed = self.names.rng.range(1, 1u64 << 32) as u32;
            let step = self.names.rng.range(1, 1u64 << 32) as u32;
            let multiplier = self.names.rng.next_u64() as u32;
            let mut payload = Vec::new();
            let mut offsets = Vec::new();
            let mut records = Vec::new();
            for (physical, &logical) in order.iter().enumerate() {
                let instruction = &program.code[logical];
                let choices = &codes[&instruction.op];
                let opcode = choices[self.names.rng.range(0, choices.len() as u64) as usize];
                let mut words = [
                    opcode,
                    instruction.a,
                    instruction.b,
                    instruction.c,
                    positions[logical + 1],
                ];
                match instruction.op {
                    Op::Jump => words[1] = positions[instruction.a as usize],
                    Op::JumpFalse | Op::JumpTrue => words[2] = positions[instruction.b as usize],
                    Op::GenericFor => {
                        words[1] = positions[instruction.a as usize];
                        words[3] = positions[instruction.c as usize];
                    }
                    _ => {}
                }
                let mut bytes = [0u8; 20];
                for (index, word) in words.iter().enumerate() {
                    bytes[layout[index] * 4..layout[index] * 4 + 4]
                        .copy_from_slice(&word.to_le_bytes());
                }
                let checksum = bytes.iter().fold(physical as u64 + 1, |hash, byte| {
                    (hash * 257 + u64::from(*byte)) % 2147483647
                }) as u32;
                let mut record = bytes.to_vec();
                record.extend_from_slice(&checksum.to_le_bytes());
                let compressed = rle(&record);
                let serialized = if self.config.vm_pack_bytecode && compressed.len() < record.len()
                {
                    let mut data = vec![1];
                    data.extend(compressed);
                    data
                } else {
                    let mut data = vec![0];
                    data.extend(record);
                    data
                };
                let serialized = crate::codec::encode(
                    &serialized,
                    seed ^ (physical as u32 + 1),
                    step,
                    multiplier,
                );
                offsets.push(payload.len() + 1);
                payload.extend_from_slice(&serialized);
                records.push(format!(
                    "{{{}}}",
                    serialized
                        .iter()
                        .map(u8::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
            offsets.push(payload.len() + 1);
            let payload = if self.config.vm_pack_bytecode {
                syntax::byte_literal(&payload)
            } else {
                format!("{{{}}}", records.join(","))
            };
            let constants = program
                .constants
                .iter()
                .map(|value| {
                    let body = Block::from(ReturnStatement::one(value.clone()));
                    format!("function(){} end", syntax::generate(&body, true))
                })
                .collect::<Vec<_>>()
                .join(",");
            let layout = layout
                .iter()
                .map(|slot| (slot * 4 + 1).to_string())
                .collect::<Vec<_>>()
                .join(",");
            let values = [
                payload,
                seed.to_string(),
                step.to_string(),
                multiplier.to_string(),
                format!("{{{layout}}}"),
                positions[0].to_string(),
                program.code.len().to_string(),
                format!("{{{constants}}}"),
                "{}".into(),
                "{}".into(),
                "nil".into(),
                format!(
                    "{{{}}}",
                    offsets
                        .iter()
                        .map(usize::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ];
            bank.push_str(&format!("[{id}]={{"));
            for (logical, value) in values.iter().enumerate() {
                bank.push_str(&format!("[{}]={value},", self.schema[logical]));
            }
            bank.push_str("},");
            if self.config.anti_tamper {
                self.report.integrity_checks += program.code.len();
            }
        }
        bank.push_str("};");
        let runtime = runtime(&self.runner, &self.schema, &codes, self.config);
        let mut runtime = syntax::generated(&runtime);
        let mut renamer = Renamer::new(self.names);
        renamer.preserve_name(&self.runner);
        ScopeVisitor::visit_block(&mut runtime, &mut renamer);
        (syntax::generated(&bank), runtime)
    }
}

fn rle(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let mut end = i + 1;
        while end < bytes.len() && bytes[end] == bytes[i] && end - i < 255 {
            end += 1;
        }
        let count = end - i;
        if count >= 4 || bytes[i] == 255 {
            result.extend([255, count as u8, bytes[i]]);
        } else {
            result.extend_from_slice(&bytes[i..end]);
        }
        i = end;
    }
    result
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

fn runtime(
    runner: &str,
    schema: &[usize],
    codes: &BTreeMap<Op, Vec<u32>>,
    config: &ObfuscationConfig,
) -> String {
    let mut source = include_str!("function_vm_runtime.luau").to_owned();
    for (index, slot) in schema.iter().enumerate() {
        source = source.replace(&format!("__S{}__", index + 1), &slot.to_string());
    }
    source = source.replace("__RUNNER__", runner);
    source = source.replace(
        "__PACKED__",
        if config.vm_pack_bytecode {
            "true"
        } else {
            "false"
        },
    );
    source = source.replace(
        "__CACHED__",
        if config.vm_predecode_bytecode {
            "true"
        } else {
            "false"
        },
    );
    source = source.replace(
        "__INTEGRITY__",
        if config.anti_tamper { "true" } else { "false" },
    );
    source = source.replace(
        "__EPHEMERAL__",
        if config.vm_hardening == "max" {
            "true"
        } else {
            "false"
        },
    );
    let mut handlers = Vec::new();
    for (op, ids) in codes {
        for id in ids {
            handlers.push((*id, handler(op)));
        }
    }
    handlers.sort_unstable_by_key(|(id, _)| *id);
    source.replace("__HANDLERS__", &dispatch_tree(&handlers))
}

fn dispatch_tree(handlers: &[(u32, String)]) -> String {
    if handlers.len() == 1 {
        let (id, code) = &handlers[0];
        return format!("if op=={id} then {code} else fail()end;");
    }
    let middle = handlers.len() / 2;
    format!(
        "if op<{} then {}else {}end;",
        handlers[middle].0,
        dispatch_tree(&handlers[..middle]),
        dispatch_tree(&handlers[middle..])
    )
}

fn handler(op: &Op) -> String {
    match op {
        Op::Constant => "r[a]=constant(b)", Op::Move => "r[a]=r[b]", Op::Read => "r[a]=external[b]()", Op::Write => "external[a](r[b],true)",
        Op::Index => "r[a]=r[b][r[c]]", Op::SetIndex => "r[a][r[b]]=r[c]", Op::Table => "r[a]=createTable(b)",
        Op::TableExtend => "local values=r[b];local expanded=createTable(c-1+values.n);for key,value in nextValue,r[a] do expanded[key]=value end;for i=1,values.n do expanded[c+i-1]=values[i]end;r[a]=expanded",
        Op::List => "r[a]={n=0}", Op::Push => "local list=r[a];list.n+=1;list[list.n]=r[b]",
        Op::Extend => "local list=r[a];local values=r[b];for i=1,values.n do list[list.n+i]=values[i]end;list.n+=values.n",
        Op::Item => "r[a]=r[b][c]", Op::Call => "r[a]=r[b](unpackValues(r[c],1,r[c].n))", Op::CallMany => "r[a]=packValues(r[b](unpackValues(r[c],1,r[c].n)))",
        Op::Vararg => "r[a]=varargs[1]", Op::VarargMany => "r[a]=varargs", Op::Return => "return 2,r[a]",
        Op::Jump => "pc=a", Op::JumpFalse => "if not r[a]then pc=b end", Op::JumpTrue => "if r[a]then pc=b end",
        Op::ForCheck => "local state=r[b];r[a]=(state[3]>0 and state[1]<=state[2])or(state[3]<=0 and state[1]>=state[2])",
        Op::ForNext => "r[a][1]+=r[a][3]", Op::Format => "r[a]=formatValues(unpackValues(r[b],1,r[b].n))",
        Op::Native => "if c==0 then r[a]=external[b](r,unpackValues(varargs,1,varargs.n))else r[a]=packValues(external[b](r,unpackValues(varargs,1,varargs.n)))end",
        Op::GenericFor => "local signal,result=external[b](r,function()return execute(a)end,unpackValues(varargs,1,varargs.n));if signal==2 then return signal,result end;pc=c",
        Op::Signal => "return a,nil",
        Op::Bind => "r[a]={r[b]}", Op::CellGet => "r[a]=r[b][1]", Op::CellSet => "r[a][1]=r[b]",
        Op::Closure => "local cells={};for i=1,#r[c] do local id=r[c][i];cells[id]=r[id]end;r[a]=external[b](cells)",
        Op::Binary(operator) => return format!("r[a]=r[b]{operator}r[c]"),
        Op::BinaryConstant(operator) => return format!("r[a]=r[b]{operator}constant(c)"),
        Op::Unary(operator) => return format!("r[a]={operator} r[b]"),
    }.to_owned()
}

struct Bridge {
    parameter: String,
    bindings: HashMap<String, u32>,
    nested_closure: bool,
    introspection: bool,
}

/// Rewrite only free references of a closure. Each captured register contains a
/// mutable cell; factories retain the cells that existed at creation time, so
/// declarations in later loop iterations cannot retarget an earlier closure.
struct Capture {
    parameter: String,
    bindings: HashMap<String, u32>,
    masks: Vec<HashSet<String>>,
    cells: BTreeSet<u32>,
    unsupported: bool,
    nested_closure: bool,
    introspection: bool,
}
impl Capture {
    fn reference(&mut self, name: &Identifier) -> Option<IndexExpression> {
        if self
            .masks
            .iter()
            .rev()
            .any(|scope| scope.contains(name.get_name()))
        {
            return None;
        }
        let Some(&register) = self.bindings.get(name.get_name()) else {
            if matches!(name.get_name().as_str(), "getfenv" | "setfenv" | "debug") {
                self.introspection = true;
            }
            return None;
        };
        self.cells.insert(register);
        Some(IndexExpression::new(
            IndexExpression::new(Prefix::from_name(self.parameter.as_str()), register as f64),
            1.0,
        ))
    }
    fn mask(&mut self, name: &str) {
        self.masks.last_mut().unwrap().insert(name.to_owned());
    }
}
impl Scope for Capture {
    fn push(&mut self) {
        self.masks.push(HashSet::new());
    }
    fn pop(&mut self) {
        self.masks.pop();
    }
    fn insert(&mut self, name: &mut String) {
        self.mask(name);
    }
    fn insert_self(&mut self) {
        self.mask("self");
    }
    fn insert_local(&mut self, name: &mut String, _: Option<&mut Expression>) {
        self.mask(name);
    }
    fn insert_local_function(&mut self, function: &mut FunctionAssignment) {
        self.mask(function.get_name());
    }
}
impl NodeProcessor for Capture {
    fn process_function_expression(&mut self, _: &mut FunctionExpression) {
        self.nested_closure = true;
    }
    fn process_type_function(&mut self, _: &mut TypeFunctionStatement) {
        self.unsupported = true;
    }
    fn process_expression(&mut self, expression: &mut Expression) {
        if let Expression::Identifier(name) = expression {
            if let Some(reference) = self.reference(name) {
                *expression = reference.into();
            }
        }
    }
    fn process_prefix_expression(&mut self, prefix: &mut Prefix) {
        if let Prefix::Identifier(name) = prefix {
            if let Some(reference) = self.reference(name) {
                *prefix = reference.into();
            }
        }
    }
    fn process_variable(&mut self, variable: &mut Variable) {
        if let Variable::Identifier(name) = variable {
            if let Some(reference) = self.reference(name) {
                *variable = reference.into();
            }
        }
    }
}
impl Bridge {
    fn reference(&self, name: &Identifier) -> Option<IndexExpression> {
        self.bindings.get(name.get_name()).map(|register| {
            IndexExpression::new(
                IndexExpression::new(Prefix::from_name(self.parameter.as_str()), *register as f64),
                1.0,
            )
        })
    }
}
impl NodeProcessor for Bridge {
    fn process_variable_expression(&mut self, name: &mut Identifier) {
        if matches!(name.get_name().as_str(), "getfenv" | "setfenv" | "debug") {
            self.introspection = true;
        }
    }
    fn process_function_expression(&mut self, _: &mut FunctionExpression) {
        self.nested_closure = true;
    }
    fn process_expression(&mut self, expression: &mut Expression) {
        if let Expression::Identifier(name) = expression {
            if let Some(reference) = self.reference(name) {
                *expression = reference.into();
            }
        }
    }
    fn process_prefix_expression(&mut self, prefix: &mut Prefix) {
        if let Prefix::Identifier(name) = prefix {
            if let Some(reference) = self.reference(name) {
                *prefix = reference.into();
            }
        }
    }
}
