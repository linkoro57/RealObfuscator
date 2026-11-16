//! Preserve native protocols while rewriting references to VM capture cells.
use darklua_core::{
    nodes::*,
    process::{NodeProcessor, Scope},
};
use std::collections::{BTreeSet, HashMap, HashSet};

pub(super) struct Bridge {
    pub(super) parameter: String,
    pub(super) bindings: HashMap<String, u32>,
    pub(super) nested_closure: bool,
    pub(super) introspection: bool,
}

/// Rewrite only free references of a closure. Each captured register contains a
/// mutable cell; factories retain the cells that existed at creation time, so
/// declarations in later loop iterations cannot retarget an earlier closure.
pub(super) struct Capture {
    pub(super) parameter: String,
    pub(super) bindings: HashMap<String, u32>,
    pub(super) masks: Vec<HashSet<String>>,
    pub(super) cells: BTreeSet<u32>,
    pub(super) unsupported: bool,
    pub(super) nested_closure: bool,
    pub(super) introspection: bool,
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
