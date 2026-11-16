use crate::{
    config::{ObfuscationConfig, StringDecodeMode},
    names::Names,
    obfuscator::ObfuscationReport,
    syntax,
};
use darklua_core::{
    nodes::*,
    process::{NodePostProcessor, NodeProcessor},
};
use std::collections::HashMap;

/// Work on source bytes, before VM compilation and literal encoding. Concatenation
/// has no user operands, so it cannot invoke user metamethods or reorder effects.
pub(crate) struct SplitStrings<'a> {
    pub rng: &'a mut crate::rng::Rng,
    pub count: usize,
}

impl NodeProcessor for SplitStrings<'_> {
    fn process_function_call(&mut self, call: &mut FunctionCall) {
        if let Arguments::String(string) = call.mutate_arguments() {
            *call.mutate_arguments() = TupleArguments::new(vec![string.clone().into()]).into();
        }
    }
}

impl NodePostProcessor for SplitStrings<'_> {
    fn process_after_expression(&mut self, expression: &mut Expression) {
        let Expression::String(string) = expression else {
            return;
        };
        let bytes = string.get_value();
        if bytes.len() < 32 {
            return;
        }
        let pieces = self.rng.range(2, 9).min((bytes.len() / 8) as u64) as usize;
        let mut cuts: Vec<_> = (1..pieces)
            .map(|i| {
                let start = i * bytes.len() / pieces;
                let width = (bytes.len() / pieces / 2).max(1);
                start + self.rng.range(0, width as u64) as usize
            })
            .collect();
        cuts.push(bytes.len());
        let mut start = 0;
        let mut result: Option<Expression> = None;
        for end in cuts {
            let fragment: Expression =
                StringExpression::from_value(bytes[start..end].to_vec()).into();
            result = Some(match result {
                None => fragment,
                Some(previous) => {
                    BinaryExpression::new(BinaryOperator::Concat, previous, fragment).into()
                }
            });
            start = end;
        }
        *expression = result.expect("at least two fragments");
        self.count += 1;
    }
}

/// Convert property reads/writes and named table keys without touching method calls.
/// Keeping colon calls preserves Roblox's NAMECALL behavior and evaluates receivers once.
pub(crate) struct Indirection {
    pub count: usize,
}

fn indexed(field: &FieldExpression) -> IndexExpression {
    IndexExpression::new(
        field.get_prefix().clone(),
        StringExpression::from_value(field.get_field().get_name().as_str()),
    )
}

impl NodeProcessor for Indirection {
    fn process_expression(&mut self, expression: &mut Expression) {
        if let Expression::Field(field) = expression {
            *expression = indexed(field).into();
            self.count += 1;
        }
    }
    fn process_prefix_expression(&mut self, prefix: &mut Prefix) {
        if let Prefix::Field(field) = prefix {
            *prefix = indexed(field).into();
            self.count += 1;
        }
    }
    fn process_variable(&mut self, variable: &mut Variable) {
        if let Variable::Field(field) = variable {
            *variable = indexed(field).into();
            self.count += 1;
        }
    }
    fn process_table_expression(&mut self, table: &mut TableExpression) {
        for entry in table.iter_mut_entries() {
            if let TableEntry::Field(field) = entry {
                *entry = TableIndexEntry::new(
                    StringExpression::from_value(field.get_field().get_name().as_str()),
                    field.get_value().clone(),
                )
                .into();
                self.count += 1;
            }
        }
    }
}

struct Constant {
    id: u32,
    bytes: Vec<u8>,
    seed: u32,
    multiplier: u32,
    step: u32,
    checksum: u64,
}

/// A feedback stream per constant. The key and decoder ship with the client; this
/// conceals literals and diversifies builds, rather than providing cryptographic secrecy.
pub(crate) struct Constants<'a> {
    config: &'a ObfuscationConfig,
    decode_mode: StringDecodeMode,
    names: &'a mut Names,
    report: &'a mut ObfuscationReport,
    decoder: String,
    interface: String,
    format: String,
    xor: String,
    key: String,
    key_value: u32,
    pool: Vec<Constant>,
    ids: HashMap<Vec<u8>, u32>,
    used_ids: std::collections::HashSet<u32>,
    interpolation: bool,
}

impl<'a> Constants<'a> {
    pub fn new(
        config: &'a ObfuscationConfig,
        decode_mode: StringDecodeMode,
        names: &'a mut Names,
        report: &'a mut ObfuscationReport,
    ) -> Self {
        Self {
            config,
            decode_mode,
            decoder: names.fresh(),
            interface: names.fresh(),
            format: names.fresh(),
            xor: names.fresh(),
            key: names.fresh(),
            key_value: names.rng.next_u64() as u32,
            names,
            report,
            pool: Vec::new(),
            ids: HashMap::new(),
            used_ids: Default::default(),
            interpolation: false,
        }
    }

    fn string(&mut self, bytes: &[u8]) -> Expression {
        self.report.string_references += 1;
        let id = if let Some(id) = self.ids.get(bytes) {
            *id
        } else {
            let id = loop {
                let id = self.names.rng.range(1, 1 << 24) as u32;
                if self.used_ids.insert(id) {
                    break id;
                }
            };
            let seed = self.names.rng.range(1, 1u64 << 32) as u32;
            let multiplier = self.names.rng.next_u64() as u32;
            let step = self.names.rng.range(1, 1u64 << 32) as u32;
            let encrypted = crate::codec::encode(bytes, seed, multiplier, step);
            let mut checksum = 0u64;
            for &byte in bytes {
                checksum = (checksum * 65599 + u64::from(byte)) % 2147483647;
            }
            self.pool.push(Constant {
                id,
                bytes: encrypted,
                seed,
                multiplier,
                step,
                checksum,
            });
            self.ids.insert(bytes.to_vec(), id);
            self.report.encrypted_strings += 1;
            id
        };
        FunctionCall::from_prefix(IndexExpression::new(
            Prefix::from_name(self.interface.as_str()),
            1.0,
        ))
        .with_argument(id as f64)
        .into()
    }

    fn number(&mut self, value: f64) -> Option<Expression> {
        // These operations are exact over uint32. Fractions, large doubles, infinities
        // and negative zero are deliberately left alone.
        if !value.is_finite()
            || value.fract() != 0.0
            || !(0.0..=u32::MAX as f64).contains(&value)
            || (value == 0.0 && value.is_sign_negative())
            || (self.config.skip_trivial_numbers && (value == 0.0 || value == 1.0))
        {
            return None;
        }
        self.report.obfuscated_numbers += 1;
        let encoded = value as u32 ^ self.key_value;
        let mut expression = FunctionCall::from_prefix(IndexExpression::new(
            Prefix::from_name(self.interface.as_str()),
            3.0,
        ))
        .with_argument(encoded as f64)
        .into();
        for _ in 1..self.config.mba_depth {
            let delta = self.names.rng.range(1, 65536);
            // The runtime key prevents folding the entire replacement into a literal.
            expression = BinaryExpression::new(
                BinaryOperator::Minus,
                BinaryExpression::new(BinaryOperator::Plus, expression, delta as f64),
                delta as f64,
            )
            .into();
        }
        Some(expression)
    }

    pub fn prefix(mut self) -> Block {
        let mut source = String::new();
        if self.interpolation {
            source.push_str(&format!("local {}=string.format;", self.format));
        }
        if self.report.obfuscated_numbers != 0 {
            source.push_str(&format!(
                "local {}=bit32.bxor;local {}={};",
                self.xor, self.key, self.key_value
            ));
        }
        if self.pool.is_empty() {
            return self.export(syntax::generated(&source));
        }
        self.names.rng.shuffle(&mut self.pool);
        let pool = self.names.fresh();
        let cache = self.names.fresh();
        let byte = self.names.fresh();
        let char_fn = self.names.fresh();
        let concat = self.names.fresh();
        let bxor = self.names.fresh();
        let shl = self.names.fresh();
        let shr = self.names.fresh();
        let band = self.names.fresh();
        let fail = self.names.fresh();
        source.push_str(&format!("local {byte},{char_fn},{concat},{bxor},{fail},{shl},{shr},{band}=string.byte,string.char,table.concat,bit32.bxor,error,bit32.lshift,bit32.rshift,bit32.band;"));
        let mut prefix = syntax::generated(&source);
        // Keep binary payloads as data throughout the pipeline. Formatting a large
        // escaped string only to parse it back into bytes adds avoidable work.
        let packed = !self.config.virtualize || self.config.vm_pack_constants;
        let entries = self
            .pool
            .iter()
            .map(|constant| {
                let payload: Expression = if packed {
                    StringExpression::from_value(constant.bytes.clone()).into()
                } else {
                    TableExpression::new(
                        constant
                            .bytes
                            .iter()
                            .map(|byte| TableEntry::from_value(*byte as f64))
                            .collect(),
                    )
                    .into()
                };
                let entry = TableExpression::new(vec![
                    TableEntry::from_value(payload),
                    TableEntry::from_value(constant.seed as f64),
                    TableEntry::from_value(constant.multiplier as f64),
                    TableEntry::from_value(constant.step as f64),
                    TableEntry::from_value(constant.checksum as f64),
                ]);
                TableIndexEntry::new(constant.id as f64, entry).into()
            })
            .collect();
        prefix.push_statement(
            VariableAssignment::from_variable(pool.as_str())
                .with_value(TableExpression::new(entries)),
        );
        source.clear();
        source.push_str(&format!("local {cache}={{}};local function {}(id){}local entry={pool}[id];if entry==nil then {fail}()end;local data=entry[1];local state={bxor}(entry[2],entry[4]);local out={{}};local hash=0;for i=1,#data do local c={};state={bxor}(state,{shl}(state,13));state={bxor}(state,{shr}(state,17));state={bxor}(state,{shl}(state,5));local v={bxor}(c,{band}(state,255));out[i]={char_fn}(v);state=(state+entry[3]+c+i)%4294967296;{}end;{}local result={concat}(out);{}return result end;",
            self.decoder,
            if self.config.cache_strings { format!("local cached={cache}[id];if cached~=nil then return cached end;") } else { String::new() },
            if packed { format!("{byte}(data,i)") } else { "data[i]".into() },
            if self.config.anti_tamper { "hash=(hash*65599+v)%2147483647;" } else { "" },
            if self.config.anti_tamper { format!("if hash~=entry[5] then {fail}()end;") } else { String::new() },
            if self.config.cache_strings { format!("{cache}[id]=result;") } else { String::new() },
        ));
        if self.config.anti_tamper {
            self.report.integrity_checks += self.pool.len();
        }
        if self.decode_mode == StringDecodeMode::Eager {
            source.push_str(&format!("for id in {pool} do {}(id)end;", self.decoder));
        }
        for statement in syntax::generated(&source).take_statements() {
            prefix.push_statement(statement);
        }
        self.export(prefix)
    }

    // Keep implementation locals out of the application's register budget. Only
    // one interface binding remains live in its scope; helpers close over their
    // builtins before application bindings or environments can change them.
    fn export(&self, mut block: Block) -> Block {
        if self.pool.is_empty() && !self.interpolation && self.report.obfuscated_numbers == 0 {
            return Block::default();
        }
        let number: Expression = if self.report.obfuscated_numbers != 0 {
            FunctionExpression::new(
                syntax::generated(&format!("return {}(value,{})", self.xor, self.key)),
                vec![TypedIdentifier::new("value")],
                false,
            )
            .into()
        } else {
            Expression::nil()
        };
        let decoder = if self.pool.is_empty() {
            Expression::nil()
        } else {
            Expression::identifier(self.decoder.as_str())
        };
        let formatter = if self.interpolation {
            Expression::identifier(self.format.as_str())
        } else {
            Expression::nil()
        };
        block.set_last_statement(ReturnStatement::one(TableExpression::new(vec![
            TableEntry::from_value(decoder),
            TableEntry::from_value(formatter),
            TableEntry::from_value(number),
        ])));
        Block::from(
            VariableAssignment::from_variable(self.interface.as_str()).with_value(
                FunctionCall::from_prefix(ParentheseExpression::new(
                    FunctionExpression::from_block(block),
                )),
            ),
        )
    }
}

impl NodeProcessor for Constants<'_> {
    fn process_function_call(&mut self, call: &mut FunctionCall) {
        if self.config.encrypt_strings {
            if let Arguments::String(string) = call.mutate_arguments() {
                *call.mutate_arguments() = TupleArguments::new(vec![string.clone().into()]).into();
            }
        }
    }
}

impl NodePostProcessor for Constants<'_> {
    fn process_after_expression(&mut self, expression: &mut Expression) {
        match expression {
            Expression::String(string) if self.config.encrypt_strings => {
                *expression = self.string(string.get_value());
            }
            Expression::Number(number) if self.config.obfuscate_numbers => {
                if let Some(new) = self.number(number.compute_value()) {
                    *expression = new;
                }
            }
            Expression::InterpolatedString(string) if self.config.encrypt_strings => {
                // Luau interpolation uses the %* format specifier. Keep all arguments
                // in one call so expression evaluation precedes __tostring conversion.
                let mut format = Vec::new();
                let mut arguments = Vec::new();
                for segment in string.iter_segments() {
                    match segment {
                        InterpolationSegment::String(segment) => {
                            for &byte in segment.get_value() {
                                format.push(byte);
                                if byte == b'%' {
                                    format.push(b'%');
                                }
                            }
                        }
                        InterpolationSegment::Value(segment) => {
                            format.extend_from_slice(b"%*");
                            arguments.push(segment.get_expression().clone());
                        }
                    }
                }
                arguments.insert(0, self.string(&format));
                *expression = FunctionCall::from_prefix(IndexExpression::new(
                    Prefix::from_name(self.interface.as_str()),
                    2.0,
                ))
                .with_arguments(TupleArguments::new(arguments))
                .into();
                self.interpolation = true;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod split_tests {
    use super::*;

    fn leaves(expression: &Expression, pieces: &mut Vec<Vec<u8>>) {
        match expression {
            Expression::String(string) => pieces.push(string.get_value().to_vec()),
            Expression::Binary(binary) => {
                assert_eq!(binary.operator(), BinaryOperator::Concat);
                leaves(binary.left(), pieces);
                leaves(binary.right(), pieces);
            }
            _ => panic!("unexpected fragment expression"),
        }
    }

    #[test]
    fn fragmentation_is_bounded_and_preserves_every_byte() {
        for length in [0, 1, 31, 32, 33, 255, 4096, 100_000] {
            let original: Vec<_> = (0..length).map(|i| i as u8).collect();
            for seed in 0..16 {
                let mut expression: Expression =
                    StringExpression::from_value(original.clone()).into();
                let mut rng = crate::rng::Rng::new(seed);
                let mut splitter = SplitStrings {
                    rng: &mut rng,
                    count: 0,
                };
                splitter.process_after_expression(&mut expression);
                let mut pieces = Vec::new();
                leaves(&expression, &mut pieces);
                assert_eq!(pieces.concat(), original);
                if length < 32 {
                    assert_eq!(pieces.len(), 1);
                    assert_eq!(splitter.count, 0);
                } else {
                    assert!((2..=8).contains(&pieces.len()));
                    assert!(pieces.iter().all(|piece| !piece.is_empty()));
                    assert_eq!(splitter.count, 1);
                }
            }
        }
    }
}
