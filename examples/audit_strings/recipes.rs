//! Independently interpret literal recipes after validating their VM records.
//! Tags are inferred from the emitted decoder's AST, never supplied by the writer.
use super::static_values::{self as data, Context, Schema, Value};
use darklua_core::{nodes::*, process::NodeProcessor};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub enum Kind {
    Start,
    Xor,
    Add,
    Mix,
    Bytes,
    Return,
}

#[derive(Default)]
pub struct Handlers(pub BTreeMap<u32, Kind>);

fn aliases(expression: &Expression, out: &mut Vec<(String, u32)>) -> Option<()> {
    let Expression::Binary(binary) = expression else {
        return None;
    };
    match binary.operator() {
        BinaryOperator::Or => {
            aliases(binary.left(), out)?;
            aliases(binary.right(), out)
        }
        BinaryOperator::Equal => {
            let Expression::Identifier(id) = binary.left() else {
                return None;
            };
            out.push((id.get_name().to_owned(), super::integer(binary.right())?));
            Some(())
        }
        _ => None,
    }
}

impl NodeProcessor for Handlers {
    fn process_if_statement(&mut self, statement: &mut IfStatement) {
        for branch in statement.iter_branches() {
            let mut tags = Vec::new();
            if aliases(branch.get_condition(), &mut tags).is_none()
                || tags.len() < 2
                || tags.iter().any(|(name, _)| name != &tags[0].0)
            {
                continue;
            }
            let block = branch.get_block();
            let kind = if matches!(block.get_last_statement(), Some(LastStatement::Return(_))) {
                Kind::Return
            } else if block
                .iter_statements()
                .any(|s| matches!(s, Statement::NumericFor(_)))
            {
                Kind::Bytes
            } else if block.iter_statements().any(|statement| {
                matches!(statement, Statement::Assign(assign) if assign.iter_variables().any(|variable| matches!(variable, Variable::Index(_))))
            }) {
                // A state-dependent memory read followed by an indexed write.
                Kind::Mix
            } else {
                let assignments: Vec<_> = block
                    .iter_statements()
                    .filter_map(|s| {
                        if let Statement::Assign(a) = s {
                            a.iter_values().next()
                        } else {
                            None
                        }
                    })
                    .collect();
                // Initialization uniquely derives state from five arguments,
                // including two descriptor fields, before allocating the output.
                if assignments.iter().any(|e| matches!(e,Expression::Call(c) if super::static_values::args(c).is_some_and(|a| a.len()==5 && a.iter().filter(|e| matches!(e,Expression::Index(_))).count()==2))) {
                    Kind::Start
                } else {
                    match assignments.last() {
                        Some(Expression::Binary(b)) if b.operator()==BinaryOperator::Percent => Kind::Xor,
                        Some(Expression::Call(_)) => Kind::Add,
                        _=>continue,
                    }
                }
            };
            for (_, tag) in tags {
                self.0.insert(tag, kind);
            }
        }
    }
}

pub fn decode(
    ctx: &Context,
    prototype: &Value,
    schema: Schema,
    index: u32,
    entry: u32,
) -> Option<Vec<u8>> {
    let table = prototype.table()?;
    let seed = table.get(&schema.seed)?.uint()? ^ table.get(&schema.salt)?.uint()?;
    let pointer = std::rc::Rc::as_ptr(match prototype {
        Value::Table(t) => t,
        _ => return None,
    }) as usize;
    let cached = ctx
        .recipe_records
        .borrow()
        .get(&pointer)
        .map(|(_, records)| records.clone());
    let code = if let Some(code) = cached {
        code
    } else {
        let code = std::rc::Rc::new(super::adapted::records(prototype, schema)?);
        ctx.recipe_records
            .borrow_mut()
            .insert(pointer, (prototype.clone(), code.clone()));
        code
    };
    let reader = super::adapted::RecordReader::new(prototype, schema)?;
    let mut pc = entry;
    let mut state = None;
    let mut memory = [0u32; 8];
    let mut out = Vec::new();
    let mut written = 0;
    let mut memory_steps = 0;
    let mut seen = BTreeSet::new();
    while pc != 0 && seen.len() < code.len() {
        if ctx.recipe_steps.get().saturating_add(seen.len()) > data::MAX_ITEMS * 4 {
            return None;
        }
        if !seen.insert(pc) {
            return None;
        }
        let [tag, a, b, c, next] = if reader.keyed(pc as usize) {
            reader.read(pc as usize, state?)?
        } else {
            *code.get(pc as usize)?
        };
        pc = next;
        match ctx.recipe_handlers.get(&tag)? {
            Kind::Start => {
                if state.is_some()
                    || b != index
                    || c as usize > data::MAX_BYTES
                    || c as usize > code.len().checked_mul(4)?
                {
                    return None;
                }
                state = Some(a ^ seed ^ index.wrapping_mul(65599) ^ entry);
                for (slot, word) in memory.iter_mut().enumerate() {
                    *word = (state? ^ (slot as u32 + 1).wrapping_mul(2654435769))
                        .rotate_left((slot as u32 + 1) * 3);
                }
                out.resize(c as usize, None);
            }
            Kind::Xor => {
                if !(1..=31).contains(&b) {
                    return None;
                }
                state = Some((state? ^ a).rotate_left(b).wrapping_add(c));
            }
            Kind::Add => {
                if !(1..=31).contains(&b) {
                    return None;
                }
                state = Some(state?.wrapping_add(a).rotate_left(b) ^ c);
            }
            Kind::Bytes => {
                let current = state?;
                let count = (b % 8) as usize;
                let offset = (b / 8) as usize;
                if !(1..=4).contains(&count) {
                    return None;
                }
                let bytes = (a ^ current).to_le_bytes();
                let destination = out.get_mut(offset..offset.checked_add(count)?)?;
                for (slot, &byte) in destination.iter_mut().zip(&bytes[..count]) {
                    if slot.replace(byte).is_some() {
                        return None;
                    }
                }
                written += count;
                state = Some((current.rotate_left(9) ^ a).wrapping_add(c));
            }
            Kind::Mix => {
                if !(1..=31).contains(&b) {
                    return None;
                }
                let current = state?;
                let slot = (current % 8) as usize;
                let previous = memory[slot];
                let value = current ^ previous;
                // Independent integer implementation of the emitted 16-bit round.
                let low = u64::from(value % 65536);
                let multiplier = u64::from(a % 65536) | 1;
                let product = ((low * multiplier + u64::from(a / 65536)) % 65536) as u32;
                let word = ((low as u32) * 65536) + ((value / 65536) ^ product);
                let next = word.rotate_left(b) ^ c;
                memory[slot] = previous.rotate_left(11).wrapping_add(next) ^ a;
                state = Some(next);
                memory_steps += 1;
            }
            Kind::Return => {
                if state.is_none() || c != index || a as usize != out.len() || written != out.len()
                {
                    return None;
                }
                let bytes: Vec<u8> = out.into_iter().collect::<Option<_>>()?;
                if data::checksum(&bytes) != b {
                    return None;
                }
                ctx.recipe_steps.set(ctx.recipe_steps.get() + seen.len());
                ctx.recipe_memory_steps
                    .set(ctx.recipe_memory_steps.get() + memory_steps);
                return Some(bytes);
            }
        }
    }
    None
}
