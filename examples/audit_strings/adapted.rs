//! Source-informed reconstruction of the public data formats, without running Lua.
use super::{
    extract, integer,
    static_values::{self as data, args, index_of, name, Context, Schema, Value},
};
use darklua_core::{
    nodes::*,
    process::{DefaultVisitor, NodePostProcessor, NodePostVisitor, NodeProcessor, NodeVisitor},
    Parser,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
};

#[derive(Default)]
pub struct ResultSet {
    pub pool: super::Audit,
    pub values: BTreeSet<Vec<u8>>,
    pub graphs: usize,
    pub graph_rejected: usize,
    pub prototypes: usize,
    pub instructions: usize,
    pub vm_rejected: usize,
    pub bound_strings: usize,
    pub bound_rejected: usize,
    pub recipe_strings: usize,
    pub recipe_steps: usize,
    pub concatenations: usize,
}

#[derive(Default)]
struct Functions(Vec<(String, Vec<String>, Block)>);
impl NodeProcessor for Functions {
    fn process_local_function_statement(&mut self, f: &mut FunctionAssignment) {
        self.0.push((
            f.get_identifier().get_name().to_owned(),
            f.iter_parameters()
                .map(|p| p.get_name().to_owned())
                .collect(),
            f.get_block().clone(),
        ));
    }
}

fn locals(block: &Block) -> Vec<(&str, &Expression)> {
    block
        .iter_statements()
        .filter_map(|s| {
            if let Statement::LocalAssign(a) = s {
                Some(
                    a.iter_variables()
                        .zip(a.iter_values())
                        .map(|(v, e)| (v.get_name().as_str(), e))
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            }
        })
        .flatten()
        .collect()
}

fn schema(functions: &Functions) -> Option<(Schema, Block)> {
    let mut schema = None;
    for (_, params, body) in &functions.0 {
        if params.len() != 1 {
            continue;
        }
        let fields: Vec<_> = locals(body)
            .iter()
            .filter_map(|(_, e)| index_of(e, &params[0]))
            .collect();
        if fields.len() != 5 {
            continue;
        }
        let keys = locals(body).into_iter().find_map(|(_, e)| {
            let Expression::Call(call) = e else {
                return None;
            };
            let arguments = args(call)?;
            if arguments.len() != 2 {
                return None;
            }
            Some((
                index_of(arguments[0], &params[0])?,
                index_of(arguments[1], &params[0])?,
            ))
        });
        if let Some((seed, salt)) = keys {
            schema = Some(Schema {
                payload: fields[0],
                offsets: fields[1],
                layout: fields[2],
                count: fields[3],
                step: fields[4],
                seed,
                salt,
                ..Default::default()
            });
            break;
        }
    }
    let mut schema = schema?;
    for (_, params, body) in &functions.0 {
        let mut candidates = Vec::new();
        if params.len() == 3 {
            candidates.push(body.clone());
        }
        if params.len() == 4 || params.len() == 5 {
            struct Invocations<'a> {
                prototype: &'a str,
                blocks: Vec<Block>,
            }
            impl NodeProcessor for Invocations<'_> {
                fn process_function_expression(&mut self, f: &mut FunctionExpression) {
                    if matches!(
                        locals(f.get_block())
                            .iter()
                            .filter(|(_, e)| index_of(e, self.prototype).is_some())
                            .count(),
                        4 | 5
                    ) {
                        self.blocks.push(f.get_block().clone());
                    }
                }
            }
            let mut search = Invocations {
                prototype: &params[0],
                blocks: Vec::new(),
            };
            DefaultVisitor::visit_block(&mut body.clone(), &mut search);
            candidates.extend(search.blocks);
        }
        for body in candidates {
            let fields: Vec<_> = locals(&body)
                .iter()
                .filter_map(|(_, e)| index_of(e, &params[0]))
                .collect();
            if fields.len() == 4 || fields.len() == 5 {
                let extra = usize::from(fields.len() == 5);
                schema.constants = fields[2 + extra];
                schema.entry = fields[3 + extra];
                schema.recipe_mask = if extra == 1 { fields[1] } else { 0 };
                return Some((schema, body));
            }
        }
    }
    None
}

struct PoolRead<'a> {
    pools: &'a BTreeMap<String, BTreeMap<u32, Vec<u8>>>,
    parameter: &'a str,
    found: BTreeSet<String>,
}
impl NodeProcessor for PoolRead<'_> {
    fn process_index_expression(&mut self, i: &mut IndexExpression) {
        if matches!(i.get_index(),Expression::Identifier(id) if id.get_name()==self.parameter) {
            if let Some(name) = name(i.get_prefix()).filter(|n| self.pools.contains_key(*n)) {
                self.found.insert(name.to_owned());
            }
        }
    }
}

fn setup(block: &Block, functions: &Functions, result: &ResultSet) -> Context {
    let mut ctx = Context::default();
    let known: BTreeMap<_, _> = result.pool.recovered.iter().cloned().collect();
    let mut pools = BTreeMap::new();
    // The current helper bundle is an initialization-only IIFE returning three
    // interfaces. Inspect its declarations and export data; never invoke it.
    let interfaces: Vec<_> = locals(block)
        .into_iter()
        .filter_map(|(name, expression)| {
            let Expression::Call(call) = expression else {
                return None;
            };
            if !args(call)?.is_empty() {
                return None;
            }
            let Prefix::Parenthese(p) = call.get_prefix() else {
                return None;
            };
            let Expression::Function(f) = p.inner_expression() else {
                return None;
            };
            if f.iter_parameters().next().is_some() {
                return None;
            }
            let Some(LastStatement::Return(ret)) = f.get_block().get_last_statement() else {
                return None;
            };
            let Expression::Table(exports) = ret.iter_expressions().next()? else {
                return None;
            };
            (exports.iter_entries().count() == 3).then_some((name, f.get_block(), exports))
        })
        .collect();
    let declarations = locals(block)
        .into_iter()
        .chain(interfaces.iter().flat_map(|(_, body, _)| locals(body)));
    for (var, e) in declarations {
        if let Some(value) = ctx.eval(e) {
            if let Value::Table(t) = &value {
                let pool: BTreeMap<_, _> = t
                    .keys()
                    .filter_map(|id| known.get(id).map(|s| (*id, s.clone())))
                    .collect();
                if !pool.is_empty() {
                    pools.insert(var.to_owned(), pool);
                }
            }
            if matches!(value, Value::Number(_) | Value::Xor) || pools.contains_key(var) {
                ctx.bindings.insert(var.to_owned(), value);
            }
        }
    }
    for (var, params, body) in &functions.0 {
        if params.len() == 1 {
            let mut visitor = PoolRead {
                pools: &pools,
                parameter: &params[0],
                found: BTreeSet::new(),
            };
            DefaultVisitor::visit_block(&mut body.clone(), &mut visitor);
            if visitor.found.len() == 1 {
                let values = pools[visitor.found.first().unwrap()].clone();
                ctx.bindings.insert(
                    var.clone(),
                    Value::Decoder(std::rc::Rc::new(values.clone())),
                );
                ctx.decoders.insert(var.clone(), values);
            }
        }
        if params.len() == 6 {
            if let Some(LastStatement::Return(ret)) = body.get_last_statement() {
                if matches!(ret.iter_expressions().next(),Some(Expression::Index(i)) if matches!(i.get_index(),Expression::Identifier(id) if id.get_name()==&params[5]))
                {
                    ctx.rebuilders.insert(var.clone());
                }
            }
        }
    }
    for (name, _, exports) in interfaces {
        let mut table = BTreeMap::new();
        let mut known = false;
        for (i, entry) in exports.iter_entries().enumerate() {
            let TableEntry::Value(expression) = entry else {
                continue;
            };
            let mut value = ctx.eval(expression).unwrap_or(Value::Unknown);
            if let Expression::Function(factory) = expression.as_ref() {
                let params: Vec<_> = factory.iter_parameters().collect();
                if params.len() == 1 && factory.get_block().iter_statements().next().is_none() {
                    if let Some(LastStatement::Return(ret)) =
                        factory.get_block().get_last_statement()
                    {
                        if let Some(Expression::Call(call)) = ret.iter_expressions().next() {
                            if ctx.is_xor(call.get_prefix()) {
                                if let Some(args) = args(call).filter(|a| a.len() == 2) {
                                    if matches!(args[0],Expression::Identifier(id) if id.get_name()==params[0].get_name())
                                    {
                                        if let Some(key) = ctx.eval(args[1]).and_then(|v| v.uint())
                                        {
                                            value = Value::NumberDecoder(key);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            known |= matches!(value, Value::Decoder(_) | Value::NumberDecoder(_));
            table.insert(i as u32 + 1, value);
        }
        if known {
            ctx.bindings
                .insert(name.into(), Value::Table(std::rc::Rc::new(table)));
        }
    }
    ctx
}

struct Normalize<'a> {
    ctx: &'a Context,
    result: &'a mut ResultSet,
}
struct Post;
impl NodePostVisitor<Normalize<'_>> for Post {}
impl NodeProcessor for Normalize<'_> {}
impl NodePostProcessor for Normalize<'_> {
    fn process_after_expression(&mut self, e: &mut Expression) {
        if !matches!(
            e,
            Expression::Call(_)
                | Expression::Binary(_)
                | Expression::Index(_)
                | Expression::Identifier(_)
                | Expression::Parenthese(_)
        ) {
            return;
        }
        if let Some(value) = self.ctx.eval(e) {
            match value {
                Value::String(bytes) => {
                    if matches!(e,Expression::Binary(b) if b.operator()==BinaryOperator::Concat) {
                        self.result.concatenations += 1;
                    }
                    self.result.values.insert(bytes.clone());
                    *e = StringExpression::from_value(bytes).into();
                }
                Value::Number(number) if number.is_finite() => *e = number.into(),
                _ => {}
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Op {
    Constant,
    Move,
    Concat,
    ConcatConstant,
}
#[derive(Default)]
struct Handlers(BTreeMap<u32, Op>);
impl NodeProcessor for Handlers {
    fn process_if_statement(&mut self, s: &mut IfStatement) {
        for branch in s.iter_branches() {
            let Expression::Binary(test) = branch.get_condition() else {
                continue;
            };
            if test.operator() != BinaryOperator::Equal {
                continue;
            }
            let Some(opcode) = integer(test.right()) else {
                continue;
            };
            let Some(Statement::Assign(assign)) = branch.get_block().iter_statements().last()
            else {
                continue;
            };
            let Some(Variable::Index(destination)) = assign.iter_variables().next() else {
                continue;
            };
            let Some(regs) = name(destination.get_prefix()) else {
                continue;
            };
            let Some(value) = assign.iter_values().next() else {
                continue;
            };
            let register = |e: &Expression| matches!(e,Expression::Index(i) if name(i.get_prefix())==Some(regs) && matches!(i.get_index(),Expression::Identifier(_)));
            let op = match value {
                Expression::Identifier(_)
                    if matches!(
                        branch.get_block().iter_statements().next(),
                        Some(Statement::LocalAssign(_))
                    ) =>
                {
                    Op::Constant
                }
                Expression::Index(_) if register(value) => Op::Move,
                Expression::Binary(b)
                    if b.operator() == BinaryOperator::Concat && register(b.left()) =>
                {
                    if register(b.right()) {
                        Op::Concat
                    } else if matches!(b.right(), Expression::Identifier(_)) {
                        Op::ConcatConstant
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };
            self.0.insert(opcode, op);
        }
    }
}

fn prototypes(value: &Value, schema: Schema, out: &mut Vec<Value>, seen: &mut BTreeSet<usize>) {
    let Value::Table(table) = value else { return };
    if !seen.insert(std::rc::Rc::as_ptr(table) as usize) {
        return;
    }
    if table.contains_key(&schema.payload)
        && table.contains_key(&schema.count)
        && table.contains_key(&schema.layout)
    {
        out.push(value.clone());
        return;
    }
    for v in table.values() {
        prototypes(v, schema, out, seen);
    }
}

pub(super) struct RecordReader<'a> {
    count: usize,
    slots: [usize; 5],
    seed: u32,
    step: u32,
    payload: &'a Value,
    offsets: &'a BTreeMap<u32, Value>,
    mask: Option<Vec<u8>>,
}
impl<'a> RecordReader<'a> {
    pub(super) fn new(prototype: &'a Value, schema: Schema) -> Option<Self> {
        let table = prototype.table()?;
        let count = table.get(&schema.count)?.uint()? as usize;
        if count == 0 || count > 200_000 {
            return None;
        }
        let layout = table.get(&schema.layout)?.table()?;
        let slots: Vec<_> = (1..=5)
            .map(|i| layout.get(&i)?.uint())
            .collect::<Option<_>>()?;
        if slots.iter().copied().collect::<BTreeSet<_>>() != BTreeSet::from([1, 5, 9, 13, 17]) {
            return None;
        }
        let mask = match table.get(&schema.recipe_mask) {
            Some(Value::Nil) | None => None,
            Some(value) => {
                let bytes = value.bytes()?;
                if bytes.len() != count.div_ceil(8) {
                    return None;
                }
                Some(bytes)
            }
        };
        Some(Self {
            count,
            slots: slots
                .into_iter()
                .map(|i| i as usize - 1)
                .collect::<Vec<_>>()
                .try_into()
                .ok()?,
            seed: table.get(&schema.seed)?.uint()? ^ table.get(&schema.salt)?.uint()?,
            step: table.get(&schema.step)?.uint()?,
            payload: table.get(&schema.payload)?,
            offsets: table.get(&schema.offsets)?.table()?,
            mask,
        })
    }
    pub(super) fn keyed(&self, index: usize) -> bool {
        index > 0
            && self.mask.as_ref().is_some_and(|m| {
                m.get((index - 1) / 8)
                    .is_some_and(|b| b & (1 << ((index - 1) % 8)) != 0)
            })
    }
    pub(super) fn read(&self, index: usize, carry: u32) -> Option<[u32; 5]> {
        if index == 0 || index > self.count {
            return None;
        }
        let cipher = match self.payload {
            Value::String(all) => {
                let first = self.offsets.get(&(index as u32))?.uint()?.checked_sub(1)? as usize;
                let last = self
                    .offsets
                    .get(&(index as u32 + 1))?
                    .uint()?
                    .checked_sub(1)? as usize;
                if last < first || last - first > 25 {
                    return None;
                }
                all.get(first..last)?.to_vec()
            }
            other => other.table()?.get(&(index as u32))?.bytes()?,
        };
        if cipher.is_empty() || cipher.len() > 25 {
            return None;
        }
        let raw = data::invert(&cipher, self.seed ^ index as u32 ^ carry, self.step);
        let mut bytes = Vec::new();
        match raw[0] {
            0 => bytes.extend_from_slice(&raw[1..]),
            1 => {
                let mut i = 1;
                while i < raw.len() {
                    if raw[i] == 255 {
                        let n = usize::from(*raw.get(i + 1)?);
                        let byte = *raw.get(i + 2)?;
                        if n == 0 || bytes.len() + n > 24 {
                            return None;
                        }
                        bytes.extend(std::iter::repeat_n(byte, n));
                        i += 3;
                    } else {
                        bytes.push(raw[i]);
                        i += 1;
                    }
                    if bytes.len() > 24 {
                        return None;
                    }
                }
            }
            _ => return None,
        }
        if bytes.len() != 24 {
            return None;
        }
        let word =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let hash = bytes[..20]
            .iter()
            .fold(index as u64, |h, b| (h * 257 + u64::from(*b)) % 2147483647)
            as u32;
        if hash != word(20) {
            return None;
        }
        // Independent inverse of contextual word feedback. No encoder calls.
        let mut state = self.seed ^ index as u32 ^ carry;
        let mut record = self.slots.map(word);
        for (i, value) in record.iter_mut().enumerate() {
            let cipher = *value;
            *value = cipher.rotate_right(state % 31 + 1) ^ state;
            state = (state.rotate_left(7) ^ cipher)
                .wrapping_add(self.step)
                .wrapping_add(i as u32 + 1);
        }
        // Return canonical operands for the bounded interpreter, while handlers
        // in the artifact consume their physical alias-specific operand roles.
        let [a, b, c] = [record[1], record[2], record[3]];
        let operands = match record[0] % 6 {
            0 => [a, b, c],
            1 => [a, c, b],
            2 => [b, a, c],
            3 => [b, c, a],
            4 => [c, a, b],
            _ => [c, b, a],
        };
        record[1..4].copy_from_slice(&operands);
        if record[4] > self.count as u32 {
            return None;
        }
        Some(record)
    }
}
pub(super) fn records(prototype: &Value, schema: Schema) -> Option<Vec<[u32; 5]>> {
    let reader = RecordReader::new(prototype, schema)?;
    let mut result = vec![[0; 5]; reader.count + 1];
    for (index, record) in result.iter_mut().enumerate().skip(1) {
        if !reader.keyed(index) {
            *record = reader.read(index, 0)?;
        }
    }
    Some(result)
}

fn inspect(prototype: &Value, ctx: &Context, handlers: &Handlers, result: &mut ResultSet) {
    let schema = ctx.schema.unwrap();
    let Some(table) = prototype.table() else {
        return;
    };
    result.prototypes += 1;
    let Some(code) = records(prototype, schema) else {
        result.vm_rejected += 1;
        return;
    };
    result.instructions += code.len() - 1;
    let mut constants = BTreeMap::new();
    if let Some(factories) = table.get(&schema.constants).and_then(Value::table) {
        for (&id, factory) in factories {
            let Value::Factory(factory) = factory else {
                continue;
            };
            let bound = factory.iter_parameters().count() == 3;
            if let Some(Value::String(bytes)) = ctx.factory(factory, prototype, id) {
                if bound {
                    result.bound_strings += 1;
                    if ctx.is_recipe_factory(factory) {
                        result.recipe_strings += 1;
                    }
                }
                result.values.insert(bytes.clone());
                constants.insert(id, bytes);
            } else if bound {
                result.bound_rejected += 1;
            }
        }
    }
    let mut registers: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    let Some(mut pc) = table.get(&schema.entry).and_then(Value::uint) else {
        result.vm_rejected += 1;
        return;
    };
    let mut seen = BTreeSet::new();
    while pc != 0 {
        if !seen.insert(pc) {
            result.vm_rejected += 1;
            break;
        }
        let Some(&[op, a, b, c, next]) = code.get(pc as usize) else {
            result.vm_rejected += 1;
            break;
        };
        pc = next;
        let value = match handlers.0.get(&op) {
            Some(Op::Constant) => constants.get(&b).cloned(),
            Some(Op::Move) => registers.get(&b).cloned(),
            Some(Op::Concat | Op::ConcatConstant) => {
                let right = if matches!(handlers.0[&op], Op::Concat) {
                    registers.get(&c)
                } else {
                    constants.get(&c)
                };
                registers.get(&b).zip(right).and_then(|(left, right)| {
                    if left.len().checked_add(right.len())? > data::MAX_BYTES {
                        return None;
                    }
                    let mut value = left.clone();
                    value.extend_from_slice(right);
                    Some(value)
                })
            }
            // Do not carry values across branches, calls or unknown effects.
            None => {
                registers.clear();
                continue;
            }
        };
        if let Some(value) = value {
            if matches!(handlers.0[&op], Op::Concat | Op::ConcatConstant) {
                result.concatenations += 1;
                result.values.insert(value.clone());
            }
            registers.insert(a, value);
        } else {
            registers.remove(&a);
        }
    }
}

pub fn extract_adapted(source: &str) -> Result<ResultSet, Box<dyn Error>> {
    stacker::maybe_grow(16 * 1024 * 1024, 32 * 1024 * 1024, || {
        let mut block = Parser::default().parse(source).map_err(|e| e.to_string())?;
        let mut result = ResultSet {
            pool: extract(source)?,
            ..Default::default()
        };
        result
            .values
            .extend(result.pool.recovered.iter().map(|(_, s)| s.clone()));
        let mut functions = Functions::default();
        DefaultVisitor::visit_block(&mut block, &mut functions);
        let mut ctx = setup(&block, &functions, &result);
        let mut recipe_handlers = super::recipes::Handlers::default();
        DefaultVisitor::visit_block(&mut block, &mut recipe_handlers);
        ctx.recipe_handlers = recipe_handlers.0;
        let runtime = schema(&functions);
        ctx.schema = runtime.as_ref().map(|(s, _)| *s);
        Post::visit_block(
            &mut block,
            &mut Normalize {
                ctx: &ctx,
                result: &mut result,
            },
        );
        for (name, e) in locals(&block) {
            let graph = matches!(e,Expression::Call(c) if data::name(c.get_prefix()).is_some_and(|n|ctx.rebuilders.contains(n)));
            if let Some(value) = ctx.eval(e) {
                if graph {
                    result.graphs += 1;
                }
                let mut found = Vec::new();
                if let Some(schema) = ctx.schema {
                    prototypes(&value, schema, &mut found, &mut BTreeSet::new());
                }
                if graph || !found.is_empty() {
                    ctx.bindings.insert(name.to_owned(), value);
                }
            } else if graph {
                result.graph_rejected += 1;
            }
        }
        Post::visit_block(
            &mut block,
            &mut Normalize {
                ctx: &ctx,
                result: &mut result,
            },
        );
        if let Some((schema, mut runtime)) = runtime {
            let mut handlers = Handlers::default();
            DefaultVisitor::visit_block(&mut runtime, &mut handlers);
            let mut found = Vec::new();
            let mut seen = BTreeSet::new();
            for value in ctx.bindings.values() {
                prototypes(value, schema, &mut found, &mut seen);
            }
            for prototype in found {
                inspect(&prototype, &ctx, &handlers, &mut result);
            }
        }
        result.recipe_steps = ctx.recipe_steps.get();
        Ok(result)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use realobfuscator::{ObfuscationConfig, Obfuscator, ProtectionLevel};

    #[test]
    fn native_fallback_literals_remain_recoverable_without_executing_source() {
        let source = "local function f()return getfenv(),'native fallback literal with enough bytes to be split into fragments' end;print(f())";
        let expected = super::super::source_literals(source).unwrap();
        for recipes in [false, true] {
            let mut config = ObfuscationConfig::compatible().with_seed(42);
            config.vm_literal_programs = recipes;
            let output = Obfuscator::new(config)
                .obfuscate_with_report(source)
                .unwrap();
            assert_eq!(output.report.vm_functions, 0);
            assert_eq!(output.report.vm_native_literal_prototypes, 1);
            let result = extract_adapted(&output.code).unwrap();
            assert!(expected.is_subset(&result.values));
            assert_eq!(
                result.vm_rejected + result.bound_rejected + result.graph_rejected,
                0
            );
        }
    }

    #[test]
    fn recovers_whole_fragments_and_bound_literals_across_formats() {
        let source = r#"
            local function f()
                return '', '\000\255\128', 'short',
                    'a long binary-safe string that must be reconstructed from multiple encoded fragments',
                    'Unicode 日本語 😀 inside a sufficiently long byte-preserving string'
            end
            local receiver={}
            function receiver:send(s)return s end
            print(f())
            receiver:send('another long literal inside native method bridge arguments for reconstruction')
        "#;
        let expected = super::super::source_literals(source).unwrap();
        for seed in [0, 42] {
            for packed in [false, true] {
                for mode in [0, 1, 2] {
                    let mut config = if mode == 2 {
                        ObfuscationConfig::hardened()
                    } else {
                        ObfuscationConfig::default().with_level(if mode == 0 {
                            ProtectionLevel::Maximum
                        } else {
                            ProtectionLevel::Paranoid
                        })
                    }
                    .with_seed(seed);
                    config.vm_pack_bytecode = packed;
                    config.vm_pack_constants = packed;
                    let artifact = Obfuscator::new(config).obfuscate(source).unwrap();
                    let result = extract_adapted(&artifact).unwrap();
                    let missing: Vec<_> = expected.difference(&result.values).collect();
                    assert!(
                        missing.is_empty(),
                        "seed={seed} packed={packed} mode={mode}, missing={missing:?}"
                    );
                    assert_eq!(
                        result.pool.rejected
                            + result.graph_rejected
                            + result.vm_rejected
                            + result.bound_rejected,
                        0
                    );
                    assert!(result.concatenations > 0);
                    assert_eq!(result.graphs > 0, mode > 0);
                    assert_eq!(result.bound_strings > 0, mode == 2);
                }
            }
        }
    }

    #[test]
    fn supports_unscattered_and_unencoded_outer_pool_variants() {
        for bound in [false, true] {
            for outer in [false, true] {
                let mut config = ObfuscationConfig::hardened().with_seed(7);
                config.vm_scatter_data = false;
                config.vm_predecode_bytecode = true;
                config.encrypt_strings = outer;
                config.vm_bind_strings = bound;
                config.vm_literal_programs = bound;
                config.require_bound_strings = bound;
                let source="return 'a sufficiently long literal whose fragments must be assembled in register order'";
                let artifact = Obfuscator::new(config).obfuscate(source).unwrap();
                let result = extract_adapted(&artifact).unwrap();
                assert!(super::super::source_literals(source)
                    .unwrap()
                    .is_subset(&result.values));
                assert_eq!(result.graphs, 0);
                assert!(result.instructions > 0);
            }
        }
    }

    #[test]
    fn malformed_graph_links_and_bound_checksums_are_rejected() {
        stacker::maybe_grow(16 * 1024 * 1024, 32 * 1024 * 1024, corrupt_data);
    }

    fn corrupt_data() {
        use darklua_core::generator::{DenseLuaGenerator, LuaGenerator};
        struct Mutate {
            graph: bool,
            changed: bool,
        }
        impl NodeProcessor for Mutate {
            fn process_function_call(&mut self, call: &mut FunctionCall) {
                if self.changed {
                    return;
                }
                if let Arguments::Tuple(args) = call.mutate_arguments() {
                    let count = args.iter_values().count();
                    if self.graph && count == 6 {
                        *args.iter_mut_values().nth(5).unwrap() = 0.0.into();
                        self.changed = true;
                    }
                }
            }
            fn process_function_expression(&mut self, f: &mut FunctionExpression) {
                if self.graph || self.changed || f.iter_parameters().count() != 3 {
                    return;
                }
                if let Some(LastStatement::Return(ret)) = f.mutate_block().mutate_last_statement() {
                    if let Some(Expression::Call(call)) = ret.iter_mut_expressions().next() {
                        if let Arguments::Tuple(args) = call.mutate_arguments() {
                            if args.iter_values().count() == 4 {
                                *args.iter_mut_values().nth(1).unwrap() = 0.0.into();
                                self.changed = true;
                            }
                        }
                    }
                }
            }
        }
        let mut legacy = ObfuscationConfig::hardened().with_seed(42);
        legacy.vm_literal_programs = false;
        let artifact = Obfuscator::new(legacy)
            .obfuscate(
                "return 'a long literal to check independent reconstruction checksum rejection'",
            )
            .unwrap();
        for graph in [false, true] {
            let mut block = Parser::default().parse(&artifact).unwrap();
            let mut mutation = Mutate {
                graph,
                changed: false,
            };
            DefaultVisitor::visit_block(&mut block, &mut mutation);
            assert!(mutation.changed);
            let mut generator = DenseLuaGenerator::default();
            generator.write_block(&block);
            let result = extract_adapted(&generator.into_string()).unwrap();
            if graph {
                assert_eq!(result.graph_rejected, 1);
                assert_eq!(result.prototypes, 0);
            } else {
                assert!(result.bound_rejected > 0);
            }
        }
        struct CorruptInstruction(bool);
        impl NodeProcessor for CorruptInstruction {
            fn process_string_expression(&mut self, string: &mut StringExpression) {
                if !self.0
                    && string.get_value().len() > 100
                    && string
                        .get_value()
                        .iter()
                        .any(|byte| !matches!(byte, b' ' | b'\t'))
                {
                    let mut bytes = string.get_value().to_vec();
                    bytes[1] ^= 1;
                    *string = StringExpression::from_value(bytes);
                    self.0 = true;
                }
            }
        }
        let mut config = ObfuscationConfig::hardened().with_seed(42);
        config.encrypt_strings = false;
        let artifact = Obfuscator::new(config)
            .obfuscate(
                "return 'an independently checked string literal inside a bytecode prototype'",
            )
            .unwrap();
        let mut block = Parser::default().parse(&artifact).unwrap();
        let mut corrupt = CorruptInstruction(false);
        DefaultVisitor::visit_block(&mut block, &mut corrupt);
        assert!(corrupt.0);
        let mut generator = DenseLuaGenerator::default();
        generator.write_block(&block);
        let result = extract_adapted(&generator.into_string()).unwrap();
        // A keyed recipe record is validated only while that recipe is followed;
        // its corruption belongs to bound rejection rather than the keyless pass.
        assert!(result.vm_rejected + result.bound_rejected > 0);
    }

    #[test]
    fn unknown_calls_and_shadowed_source_values_are_not_evaluated() {
        let mut ctx = Context::default();
        ctx.bindings.insert("xor".into(), Value::Xor);
        let block = Parser::default()
            .parse("return os.execute('never executed'),loadstring('never compiled')(),xor(1,2)")
            .unwrap();
        let Some(LastStatement::Return(ret)) = block.get_last_statement() else {
            panic!()
        };
        let values: Vec<_> = ret.iter_expressions().map(|e| ctx.eval(e)).collect();
        assert!(values[0].is_none() && values[1].is_none());
        assert_eq!(values[2].as_ref().and_then(Value::uint), Some(3));
        let result =
            extract_adapted("local x='outer';do local x='inner';print(x..'literal')end").unwrap();
        assert!(result.values.is_empty());
    }
}
