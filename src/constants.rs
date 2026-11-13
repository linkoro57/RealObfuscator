use crate::{config::ObfuscationConfig, names::Names, obfuscator::ObfuscationReport, syntax};
use darklua_core::{
    nodes::*,
    process::{NodePostProcessor, NodeProcessor},
};
use std::collections::HashMap;

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
    names: &'a mut Names,
    report: &'a mut ObfuscationReport,
    decoder: String,
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
        names: &'a mut Names,
        report: &'a mut ObfuscationReport,
    ) -> Self {
        Self {
            config,
            decoder: names.fresh(),
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
        FunctionCall::from_name(self.decoder.as_str())
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
        let mut expression = FunctionCall::from_name(self.xor.as_str())
            .with_argument(encoded as f64)
            .with_argument(Expression::identifier(self.key.as_str()))
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
            return syntax::generated(&source);
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
        source.push_str(&format!("local {byte},{char_fn},{concat},{bxor},{fail},{shl},{shr},{band}=string.byte,string.char,table.concat,bit32.bxor,error,bit32.lshift,bit32.rshift,bit32.band;local {pool}={{"));
        let packed = !self.config.virtualize || self.config.vm_pack_constants;
        for constant in &self.pool {
            let payload = if packed {
                syntax::byte_literal(&constant.bytes)
            } else {
                format!(
                    "{{{}}}",
                    constant
                        .bytes
                        .iter()
                        .map(u8::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            source.push_str(&format!(
                "[{}]={{{payload},{},{},{},{}}},",
                constant.id, constant.seed, constant.multiplier, constant.step, constant.checksum
            ));
        }
        source.push_str(&format!("}};local {cache}={{}};local function {}(id){}local entry={pool}[id];if entry==nil then {fail}()end;local data=entry[1];local state={bxor}(entry[2],entry[4]);local out={{}};local hash=0;for i=1,#data do local c={};state={bxor}(state,{shl}(state,13));state={bxor}(state,{shr}(state,17));state={bxor}(state,{shl}(state,5));local v={bxor}(c,{band}(state,255));out[i]={char_fn}(v);state=(state+entry[3]+c+i)%4294967296;{}end;{}local result={concat}(out);{}return result end;",
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
        if self.config.string_decode_mode == "eager" {
            source.push_str(&format!("for id in {pool} do {}(id)end;", self.decoder));
        }
        syntax::generated(&source)
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
                *expression = FunctionCall::from_name(self.format.as_str())
                    .with_arguments(TupleArguments::new(arguments))
                    .into();
                self.interpolation = true;
            }
            _ => {}
        }
    }
}
