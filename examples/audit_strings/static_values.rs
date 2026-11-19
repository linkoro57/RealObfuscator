//! A deliberately small, non-executing evaluator for generated data expressions.
use darklua_core::nodes::*;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

pub const MAX_ITEMS: usize = 2_000_000;
pub const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub enum Value {
    Nil,
    Number(f64),
    String(Vec<u8>),
    Table(Rc<BTreeMap<u32, Value>>),
    Factory(Rc<FunctionExpression>),
    Xor,
    Decoder(Rc<BTreeMap<u32, Vec<u8>>>),
    NumberDecoder(u32),
    Unknown,
}
impl Value {
    pub fn uint(&self) -> Option<u32> {
        match self {
            Self::Number(n)
                if n.is_finite() && n.fract() == 0.0 && (0.0..=u32::MAX as f64).contains(n) =>
            {
                Some(*n as u32)
            }
            _ => None,
        }
    }
    pub fn table(&self) -> Option<&BTreeMap<u32, Value>> {
        match self {
            Self::Table(t) => Some(t),
            _ => None,
        }
    }
    pub fn bytes(&self) -> Option<Vec<u8>> {
        match self {
            Self::String(s) => Some(s.clone()),
            Self::Table(t) if t.len() <= MAX_BYTES => (1..=t.len() as u32)
                .map(|i| u8::try_from(t.get(&i)?.uint()?).ok())
                .collect(),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Schema {
    pub payload: u32,
    pub offsets: u32,
    pub layout: u32,
    pub count: u32,
    pub seed: u32,
    pub salt: u32,
    pub step: u32,
    pub entry: u32,
    pub constants: u32,
    pub recipe_mask: u32,
}

#[derive(Default)]
pub struct Context {
    pub bindings: BTreeMap<String, Value>,
    pub decoders: BTreeMap<String, BTreeMap<u32, Vec<u8>>>,
    pub rebuilders: BTreeSet<String>,
    pub schema: Option<Schema>,
    pub recipe_handlers: BTreeMap<u32, super::recipes::Kind>,
    pub recipe_records: RefCell<RecordCache>,
    pub recipe_steps: Cell<usize>,
    pub recipe_memory_steps: Cell<usize>,
}

// Keep the owning table alive so its pointer cannot be reused by another graph.
type RecordCache = BTreeMap<usize, (Value, Rc<Vec<[u32; 5]>>)>;

pub fn args(call: &FunctionCall) -> Option<Vec<&Expression>> {
    if call.get_method().is_some() {
        return None;
    }
    match call.get_arguments() {
        Arguments::Tuple(t) => Some(t.iter_values().collect()),
        _ => None,
    }
}
pub fn name(prefix: &Prefix) -> Option<&str> {
    match prefix {
        Prefix::Identifier(id) => Some(id.get_name()),
        _ => None,
    }
}
pub fn index_of(expression: &Expression, object: &str) -> Option<u32> {
    let Expression::Index(i) = expression else {
        return None;
    };
    if name(i.get_prefix())? != object {
        return None;
    }
    super::integer(i.get_index())
}

impl Context {
    pub fn is_xor(&self, prefix: &Prefix) -> bool {
        matches!(self.prefix(prefix, 0), Some(Value::Xor))
    }
    pub fn eval(&self, expression: &Expression) -> Option<Value> {
        self.inner(expression, 0)
    }
    fn prefix(&self, prefix: &Prefix, depth: usize) -> Option<Value> {
        if depth > 128 {
            return None;
        }
        match prefix {
            Prefix::Identifier(id) => self.bindings.get(id.get_name()).cloned(),
            Prefix::Index(i) => self.index(i, depth),
            Prefix::Parenthese(p) => self.inner(p.inner_expression(), depth + 1),
            Prefix::Call(call) => self.call(call, depth + 1),
            Prefix::Field(f)
                if name(f.get_prefix()) == Some("bit32") && f.get_field().get_name() == "bxor" =>
            {
                Some(Value::Xor)
            }
            _ => None,
        }
    }
    fn index(&self, i: &IndexExpression, depth: usize) -> Option<Value> {
        let table = self.prefix(i.get_prefix(), depth + 1)?;
        let key = self.inner(i.get_index(), depth + 1)?.uint()?;
        Some(table.table()?.get(&key).cloned().unwrap_or(Value::Nil))
    }
    fn inner(&self, expression: &Expression, depth: usize) -> Option<Value> {
        if depth > 128 {
            return None;
        }
        match expression {
            Expression::Nil(_) => Some(Value::Nil),
            Expression::Number(n) => Some(Value::Number(n.compute_value())),
            Expression::String(s) => Some(Value::String(s.get_value().to_vec())),
            Expression::Identifier(id) => self.bindings.get(id.get_name()).cloned(),
            Expression::Parenthese(p) => self.inner(p.inner_expression(), depth + 1),
            Expression::Index(i) => self.index(i, depth),
            Expression::Field(f)
                if name(f.get_prefix()) == Some("bit32") && f.get_field().get_name() == "bxor" =>
            {
                Some(Value::Xor)
            }
            Expression::Call(call) => self.call(call, depth + 1),
            Expression::Function(f) => Some(Value::Factory(Rc::new(f.as_ref().clone()))),
            Expression::Binary(b) => {
                let left = self.inner(b.left(), depth + 1)?;
                let right = self.inner(b.right(), depth + 1)?;
                match (left, right) {
                    (Value::Number(a), Value::Number(c)) => {
                        Some(Value::Number(match b.operator() {
                            BinaryOperator::Plus => a + c,
                            BinaryOperator::Minus => a - c,
                            BinaryOperator::Asterisk => a * c,
                            BinaryOperator::Slash => a / c,
                            BinaryOperator::Percent => a - c * (a / c).floor(),
                            _ => return None,
                        }))
                    }
                    (Value::String(mut a), Value::String(c))
                        if b.operator() == BinaryOperator::Concat
                            && a.len().checked_add(c.len())? <= MAX_BYTES =>
                    {
                        a.extend(c);
                        Some(Value::String(a))
                    }
                    _ => None,
                }
            }
            Expression::Table(table) => {
                let mut values = BTreeMap::new();
                let mut implicit = 0u32;
                for entry in table.iter_entries() {
                    if values.len() >= MAX_ITEMS {
                        return None;
                    }
                    let (key, value) = match entry {
                        TableEntry::Value(value) => {
                            implicit = implicit.checked_add(1)?;
                            (implicit, value.as_ref())
                        }
                        TableEntry::Index(entry) => (
                            self.inner(entry.get_key(), depth + 1)?.uint()?,
                            entry.get_value(),
                        ),
                        _ => return None,
                    };
                    let value = self.inner(value, depth + 1)?;
                    // Data initializers have unique keys. Reject ambiguous tables.
                    if values.insert(key, value).is_some() {
                        return None;
                    }
                }
                Some(Value::Table(Rc::new(values)))
            }
            _ => None,
        }
    }
    fn call(&self, call: &FunctionCall, depth: usize) -> Option<Value> {
        if depth > 128 {
            return None;
        }
        let args = args(call)?;
        if let Some(name) = name(call.get_prefix()) {
            if let Some(pool) = self.decoders.get(name) {
                if args.len() != 1 {
                    return None;
                }
                let id = self.inner(args[0], depth + 1)?.uint()?;
                return Some(Value::String(pool.get(&id)?.clone()));
            }
            if self.rebuilders.contains(name) {
                return self.rebuild(&args, depth + 1);
            }
        }
        match self.prefix(call.get_prefix(), depth + 1)? {
            Value::Decoder(pool) if args.len() == 1 => Some(Value::String(
                pool.get(&self.inner(args[0], depth + 1)?.uint()?)?.clone(),
            )),
            Value::NumberDecoder(key) if args.len() == 1 => Some(Value::Number(
                (self.inner(args[0], depth + 1)?.uint()? ^ key) as f64,
            )),
            Value::Xor if (2..=5).contains(&args.len()) => {
                let mut result = 0;
                for arg in args {
                    result ^= self.inner(arg, depth + 1)?.uint()?;
                }
                Some(Value::Number(result as f64))
            }
            Value::Factory(factory) if args.len() == 3 => {
                let context = self.inner(args[1], depth + 1)?;
                let index = self.inner(args[2], depth + 1)?.uint()?;
                self.factory(&factory, &context, index)
            }
            _ => None,
        }
    }
    pub fn is_recipe_factory(&self, factory: &FunctionExpression) -> bool {
        if let Some(LastStatement::Return(ret)) = factory.get_block().get_last_statement() {
            if let Some(Expression::Call(call)) = ret.iter_expressions().next() {
                return args(call).is_some_and(|a| a.len() == 3)
                    && factory.iter_parameters().count() == 3;
            }
        }
        false
    }
    pub fn factory(
        &self,
        factory: &FunctionExpression,
        prototype: &Value,
        index: u32,
    ) -> Option<Value> {
        if factory.get_block().iter_statements().next().is_some() {
            return None;
        }
        let Some(LastStatement::Return(ret)) = factory.get_block().get_last_statement() else {
            return None;
        };
        let values: Vec<_> = ret.iter_expressions().collect();
        if values.len() != 1 {
            return None;
        }
        let params: Vec<_> = factory
            .iter_parameters()
            .map(|id| id.get_name().as_str())
            .collect();
        if params.is_empty() {
            return self.eval(values[0]);
        }
        let Expression::Call(call) = values[0] else {
            return None;
        };
        let args = args(call)?;
        if params.len() == 3
            && args.len() == 3
            && name(call.get_prefix()) == Some(params[0])
            && matches!(args[0],Expression::Identifier(id) if id.get_name()==params[1])
            && matches!(args[1],Expression::Identifier(id) if id.get_name()==params[2])
        {
            let entry = self.eval(args[2])?.uint()?;
            return super::recipes::decode(self, prototype, self.schema?, index, entry)
                .map(Value::String);
        }
        if params.len() != 3 || args.len() != 4 || name(call.get_prefix()) != Some(params[0]) {
            return None;
        }
        if !matches!(args[2], Expression::Identifier(id) if id.get_name()==params[2])
            || !matches!(args[3], Expression::Identifier(id) if id.get_name()==params[1])
        {
            return None;
        }
        let cipher = self.eval(args[0])?.bytes()?;
        let expected = self.eval(args[1])?.uint()?;
        let schema = self.schema?;
        let table = prototype.table()?;
        let seed = table.get(&schema.seed)?.uint()?;
        let salt = table.get(&schema.salt)?.uint()?;
        let step = table.get(&schema.step)?.uint()?;
        let entry = table.get(&schema.entry)?.uint()?;
        let plain = invert(
            &cipher,
            seed ^ salt ^ entry ^ index.wrapping_mul(65599) ^ 0xa511e9b3,
            step,
        );
        if plain.first().copied() != Some(index as u8) {
            return None;
        }
        let bytes = plain.get(1..)?.to_vec();
        (checksum(&bytes) == expected).then_some(Value::String(bytes))
    }
    fn rebuild(&self, args: &[&Expression], depth: usize) -> Option<Value> {
        if args.len() != 6 {
            return None;
        }
        let columns: Vec<_> = args[..4]
            .iter()
            .map(|a| self.inner(a, depth + 1))
            .collect::<Option<_>>()?;
        let columns: Vec<_> = columns.iter().map(Value::table).collect::<Option<_>>()?;
        let count = self.inner(args[4], depth + 1)?.uint()? as usize;
        let root = self.inner(args[5], depth + 1)?.uint()? as usize;
        if count == 0
            || count > MAX_ITEMS
            || root == 0
            || root > count
            || columns[0].len() > MAX_ITEMS
        {
            return None;
        }
        let mut nodes = vec![Vec::new(); count + 1];
        for id in 1..=columns[0].len() as u32 {
            let owner = columns[0].get(&id)?.uint()? as usize;
            let key = columns[1].get(&id)?.uint()?;
            let target = columns[3].get(&id)?.uint()? as usize;
            if owner == 0 || owner > count || target > count {
                return None;
            }
            nodes[owner].push((
                key,
                columns[2].get(&id).cloned().unwrap_or(Value::Nil),
                target,
            ));
        }
        fn link(
            id: usize,
            nodes: &[Vec<(u32, Value, usize)>],
            active: &mut BTreeSet<usize>,
            cache: &mut BTreeMap<usize, Value>,
            depth: usize,
        ) -> Option<Value> {
            if depth > 128 || !active.insert(id) {
                return None;
            }
            let mut table = BTreeMap::new();
            for (key, value, target) in &nodes[id] {
                let value = if *target == 0 {
                    value.clone()
                } else if let Some(value) = cache.get(target) {
                    value.clone()
                } else {
                    link(*target, nodes, active, cache, depth + 1)?
                };
                if table.insert(*key, value).is_some() {
                    return None;
                }
            }
            active.remove(&id);
            let value = Value::Table(Rc::new(table));
            cache.insert(id, value.clone());
            Some(value)
        }
        link(root, &nodes, &mut BTreeSet::new(), &mut BTreeMap::new(), 0)
    }
}

pub fn invert(cipher: &[u8], mut state: u32, step: u32) -> Vec<u8> {
    cipher
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let plain = c ^ state as u8;
            state = state
                .wrapping_add(step)
                .wrapping_add(u32::from(c))
                .wrapping_add(i as u32 + 1);
            plain
        })
        .collect()
}
pub fn checksum(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0u64, |h, b| (h * 65599 + u64::from(*b)) % 2147483647) as u32
}
