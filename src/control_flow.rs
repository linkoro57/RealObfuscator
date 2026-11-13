//! Conservative dispatch: declarations and control transfers remain in their original scopes.
//! The experimental VM executes statement handlers, not arbitrary Luau bytecode.
use crate::{
    config::{DeadCodeDensity, ObfuscationConfig},
    names::Names,
    obfuscator::ObfuscationReport,
    syntax,
};
use darklua_core::{
    nodes::*,
    process::{NodePostProcessor, NodeProcessor},
};
use std::collections::HashSet;

pub(crate) struct ControlFlow<'a> {
    pub config: &'a ObfuscationConfig,
    pub names: &'a mut Names,
    pub report: &'a mut ObfuscationReport,
    pub byte: String,
    pub xor: String,
    pub fail: String,
}

impl ControlFlow<'_> {
    fn branch_tree(
        &mut self,
        branches: &[(u64, String)],
        state: &str,
        predicate: &str,
        invalid: &str,
    ) -> String {
        if branches.len() == 1 {
            let (id, body) = &branches[0];
            return format!("if {state}=={id}{predicate} then {body}else {invalid} end;");
        }
        let middle = branches.len() / 2;
        let split = branches[middle].0;
        let low = self.branch_tree(&branches[..middle], state, predicate, invalid);
        let high = self.branch_tree(&branches[middle..], state, predicate, invalid);
        // Bounded logarithmic dispatch, with independently diversified orientations.
        if self.names.rng.range(0, 2) == 0 {
            format!("if {state}<{split} then {low}else {high}end;")
        } else {
            format!("if {state}>={split} then {high}else {low}end;")
        }
    }

    fn dispatch(&mut self, statements: Vec<Statement>) -> Statement {
        let count = statements.len();
        let state = self.names.fresh();
        let guard = self.names.fresh();
        let mut used = HashSet::new();
        let mut ids = Vec::new();
        let dead_count = if self.config.inject_dead_code {
            let density = match self.config.dead_code_density {
                DeadCodeDensity::Low => 1,
                DeadCodeDensity::Medium => 2,
                DeadCodeDensity::High => 4,
            };
            (self.config.fake_function_count * density).min(16)
        } else {
            0
        };
        let upper = if self.config.virtualize { 256 } else { 1 << 24 };
        for _ in 0..count + dead_count + 1 {
            let id = loop {
                let candidate = self.names.rng.range(1, upper);
                if used.insert(candidate) {
                    break candidate;
                }
            };
            ids.push(id);
        }
        let mut branches = Vec::new();
        for (i, statement) in statements.into_iter().enumerate() {
            let body = syntax::generate(&Block::from(statement), true);
            let next = if self.config.virtualize {
                String::new()
            } else {
                format!("{state}={};", ids[i + 1])
            };
            branches.push((ids[i], format!("{body};{next}")));
        }
        // Unreachable handlers are mixed among real handlers, rather than appended
        // as a single obvious `if false` function.
        for &id in ids.iter().skip(count + 1) {
            branches.push((
                id,
                format!(
                    "{state}=({state}+{})%{};",
                    self.names.rng.range(17, 4096),
                    upper
                ),
            ));
        }
        self.report.decoy_blocks += dead_count;
        branches.sort_unstable_by_key(|branch| branch.0);
        let guard_value = self.names.rng.range(2, 65536);
        let predicate = if self.config.opaque_predicates {
            format!(" and ({guard}*({guard}+1))%2==0")
        } else {
            String::new()
        };
        let guard_init = if self.config.opaque_predicates {
            self.report.opaque_predicates += 1;
            format!("local {guard}={guard_value};")
        } else {
            String::new()
        };
        let mut source = format!("do {guard_init}");
        if self.config.virtualize {
            self.report.virtualized_statements += count;
            let tape = self.names.fresh();
            let pc = self.names.fresh();
            let key = self.names.fresh();
            let cipher_name = self.names.fresh();
            let iterator = self.names.fresh();
            let hash = self.names.fresh();
            let seed = self.names.rng.range(1, 256);
            let multiplier = match self.config.vm_hardening.as_str() {
                "max" => 97,
                "client-max" => 65,
                _ => 33,
            };
            let step = self.names.rng.range(1, 256);
            let mut rolling = seed;
            let mut encrypted = Vec::new();
            let mut checksum = 0u64;
            for (i, opcode) in ids[..count].iter().enumerate() {
                let cipher = opcode ^ rolling;
                encrypted.push(cipher as u8);
                rolling = (rolling * multiplier + cipher + ((i + 1) as u64 * step) % 256) % 256;
                checksum = (checksum * 257 + cipher) % 2147483647;
            }
            let payload = if self.config.vm_pack_bytecode {
                syntax::byte_literal(&encrypted)
            } else {
                format!(
                    "{{{}}}",
                    encrypted
                        .iter()
                        .map(u8::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            let read = |index: &str| {
                if self.config.vm_pack_bytecode {
                    format!("{}({tape},{index})", self.byte)
                } else {
                    format!("{tape}[{index}]")
                }
            };
            source.push_str(&format!("local {tape}={payload};"));
            if self.config.anti_tamper {
                source.push_str(&format!("local {hash}=0;for {iterator}=1,#{tape} do {hash}=({hash}*257+{})%2147483647 end;if {hash}~={checksum} then {}()end;", read(&iterator), self.fail));
                self.report.integrity_checks += 1;
            }
            if self.config.vm_predecode_bytecode {
                let decoded = self.names.fresh();
                source.push_str(&format!("local {decoded}={{}};local {key}={seed};for {iterator}=1,#{tape} do local {cipher_name}={};{decoded}[{iterator}]={}({cipher_name},{key});{key}=({key}*{multiplier}+{cipher_name}+({iterator}*{step})%256)%256 end;local {pc}=1;while {pc}<=#{decoded} do local {state}={decoded}[{pc}];{pc}={pc}+1;", read(&iterator), self.xor));
            } else {
                source.push_str(&format!("local {pc}=1;local {key}={seed};while {pc}<=#{tape} do local {cipher_name}={};local {state}={}({cipher_name},{key});{key}=({key}*{multiplier}+{cipher_name}+({pc}*{step})%256)%256;{pc}={pc}+1;", read(&pc), self.xor));
            }
        } else {
            source.push_str(&format!(
                "local {state}={};while {state}~={} do ",
                ids[0], ids[count]
            ));
        }
        let invalid = if self.config.anti_tamper || self.config.virtualize {
            format!("{}()", self.fail)
        } else {
            "break".into()
        };
        source.push_str(&self.branch_tree(&branches, &state, &predicate, &invalid));
        source.push_str("end;end");
        self.report.flattened_regions += 1;
        syntax::generated(&source).take_statements().pop().unwrap()
    }
}

impl NodeProcessor for ControlFlow<'_> {}
impl NodePostProcessor for ControlFlow<'_> {
    fn process_after_block(&mut self, block: &mut Block) {
        let threshold = if self.config.virtualize {
            1
        } else {
            self.config.min_blocks_for_cff
        };
        let mut output = Vec::new();
        let mut region = Vec::new();
        for statement in block.take_statements() {
            if matches!(
                statement,
                Statement::Assign(_) | Statement::CompoundAssign(_) | Statement::Call(_)
            ) {
                region.push(statement);
                // Bound interpreter size and local register pressure on very large scripts.
                if region.len() == 128 {
                    output.push(self.dispatch(std::mem::take(&mut region)));
                }
            } else {
                if region.len() >= threshold {
                    output.push(self.dispatch(std::mem::take(&mut region)));
                } else {
                    output.append(&mut region);
                }
                output.push(statement);
            }
        }
        if region.len() >= threshold {
            output.push(self.dispatch(region));
        } else {
            output.append(&mut region);
        }
        block.set_statements(output);
    }
}
