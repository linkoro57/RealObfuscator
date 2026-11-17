//! Bind native bridge arguments to the same prototype as their compiled caller.
use super::{
    compiler::Program,
    schema::{Field, Schema},
};
use crate::syntax;
use darklua_core::{
    nodes::*,
    process::{NodePostProcessor, NodePostVisitor, NodeProcessor},
};
use std::collections::HashMap;

struct NativeStrings<'a> {
    constants: &'a mut Vec<Expression>,
    ids: HashMap<Vec<u8>, u32>,
    prototype: IndexExpression,
    constant_slot: usize,
    decoder: &'a str,
    formatter: Option<&'a str>,
}

impl NativeStrings<'_> {
    fn bind(&mut self, string: &StringExpression) -> Expression {
        let bytes = string.get_value();
        let index = if let Some(index) = self.ids.get(bytes) {
            *index
        } else {
            self.constants
                .push(StringExpression::from_value(bytes.to_vec()).into());
            let index = self.constants.len() as u32;
            self.ids.insert(bytes.to_vec(), index);
            index
        };
        let factories = IndexExpression::new(self.prototype.clone(), self.constant_slot as f64);
        let factory = IndexExpression::new(factories, index as f64);
        FunctionCall::from_prefix(factory)
            .with_argument(Expression::identifier(self.decoder))
            .with_argument(self.prototype.clone())
            .with_argument(index as f64)
            .into()
    }
}

impl NodeProcessor for NativeStrings<'_> {
    fn process_function_call(&mut self, call: &mut FunctionCall) {
        if let Arguments::String(string) = call.mutate_arguments() {
            *call.mutate_arguments() = TupleArguments::new(vec![string.clone().into()]).into();
        }
    }
}
impl NodePostProcessor for NativeStrings<'_> {
    fn process_after_expression(&mut self, expression: &mut Expression) {
        match expression {
            Expression::String(string) => *expression = self.bind(string),
            Expression::InterpolatedString(string) if self.formatter.is_some() => {
                // Evaluate all values before __tostring conversions, as in Luau.
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
                            // Interpolation consumes exactly one value, including
                            // nil when the last call returns no values.
                            values.push(
                                ParentheseExpression::new(segment.get_expression().clone()).into(),
                            );
                        }
                    }
                }
                values.insert(0, self.bind(&StringExpression::from_value(format)));
                *expression = FunctionCall::from_name(self.formatter.unwrap())
                    .with_arguments(TupleArguments::new(values))
                    .into();
            }
            _ => {}
        }
    }
}
struct RuntimeStrings;
impl NodePostVisitor<NativeStrings<'_>> for RuntimeStrings {
    fn visit_type_function_statement(_: &mut TypeFunctionStatement, _: &mut NativeStrings<'_>) {}
    fn visit_type(_: &mut Type, _: &mut NativeStrings<'_>) {}
}

pub(super) fn bind_native_strings(
    program: &mut Program,
    bank: &str,
    id: u32,
    decoder: &str,
    schema: &Schema,
) {
    let ids = program
        .constants
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            if let Expression::String(string) = value {
                Some((string.get_value().to_vec(), index as u32 + 1))
            } else {
                None
            }
        })
        .collect();
    let mut bindings = NativeStrings {
        constants: &mut program.constants,
        ids,
        prototype: IndexExpression::new(Prefix::from_name(bank), id as f64),
        constant_slot: schema.slot(Field::Constants),
        decoder,
        formatter: None,
    };
    for external in &mut program.external {
        let mut body = syntax::generated(&format!("return {external}"));
        RuntimeStrings::visit_block(&mut body, &mut bindings);
        *external = syntax::generate(&body, true)
            .strip_prefix("return")
            .expect("return expression wrapper")
            .trim()
            .to_owned();
    }
}

/// String calls add no persistent wrapper around native stack-sensitive code.
pub(super) fn bind_fallback_strings(
    program: &mut Program,
    block: &mut Block,
    bank: &str,
    id: u32,
    decoder: &str,
    schema: &Schema,
    formatter: &str,
) {
    let mut bindings = NativeStrings {
        constants: &mut program.constants,
        ids: HashMap::new(),
        prototype: IndexExpression::new(Prefix::from_name(bank), id as f64),
        constant_slot: schema.slot(Field::Constants),
        decoder,
        formatter: Some(formatter),
    };
    RuntimeStrings::visit_block(block, &mut bindings);
}
