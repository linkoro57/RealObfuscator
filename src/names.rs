use crate::{
    lexer::{lex, Kind},
    rng::Rng,
};
use darklua_core::{
    nodes::*,
    process::{NodeProcessor, NodeVisitor, Scope},
};
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) struct Names {
    pub rng: Rng,
    used: HashSet<String>,
    min: usize,
    max: usize,
    pub error: Option<String>,
}

impl Names {
    pub fn new(source: &str, rng: Rng, min: usize, max: usize) -> Self {
        let used = lex(source).unwrap_or_default().into_iter()
            .filter(|t| matches!(t.kind, Kind::Name | Kind::Keyword))
            .map(|t| t.text)
            .chain("and break continue do else elseif end export false for function if in local nil not or repeat return then true type until while self".split_whitespace().map(str::to_owned))
            // Reserve the helper template's temporary names and built-ins before
            // allocating its public interface, including for short-name settings.
            .chain("id cached entry data state out hash i c v result string table bit32 error".split_whitespace().map(str::to_owned)).collect();
        Self {
            rng,
            used,
            min,
            max,
            error: None,
        }
    }

    pub fn fresh(&mut self) -> String {
        for _ in 0..4096 {
            let name = self.rng.name(self.min, self.max);
            if self.used.insert(name.clone()) {
                return name;
            }
        }
        self.error = Some("identifier namespace exhausted; increase name_max_length".into());
        // Continue the visitor without hanging; the caller rejects the result.
        loop {
            let name = format!("_ro_{}", self.rng.next_u64());
            if self.used.insert(name.clone()) {
                return name;
            }
        }
    }
}

pub(crate) struct Renamer<'a> {
    pub names: &'a mut Names,
    pub count: usize,
    scopes: Vec<HashMap<String, String>>,
    preserve: HashSet<String>,
    type_functions: VecDeque<TypeFunctionStatement>,
}

impl<'a> Renamer<'a> {
    pub fn preserve_name(&mut self, name: &str) {
        self.preserve.insert(name.to_owned());
    }
    pub fn new(names: &'a mut Names) -> Self {
        Self {
            names,
            count: 0,
            scopes: Vec::new(),
            preserve: HashSet::new(),
            type_functions: VecDeque::new(),
        }
    }
    pub fn restore_type_functions(&mut self, block: &mut Block) {
        struct Restore<'a>(&'a mut VecDeque<TypeFunctionStatement>);
        impl NodeProcessor for Restore<'_> {}
        struct RestoreVisitor;
        impl NodeVisitor<Restore<'_>> for RestoreVisitor {
            fn visit_type_function(
                function: &mut TypeFunctionStatement,
                restore: &mut Restore<'_>,
            ) {
                *function = restore
                    .0
                    .pop_front()
                    .expect("one saved type function per placeholder");
            }
        }
        RestoreVisitor::visit_block(block, &mut Restore(&mut self.type_functions));
    }
    /// Generated helper names are already random and are referenced by the source
    /// block. Keep their interface while diversifying decoder internals as well.
    pub fn preserve_root_bindings(&mut self, block: &Block) {
        for statement in block.iter_statements() {
            match statement {
                Statement::LocalAssign(assign) => self
                    .preserve
                    .extend(assign.iter_variables().map(|v| v.get_name().to_owned())),
                Statement::LocalFunction(function) => {
                    self.preserve
                        .insert(function.get_identifier().get_name().to_owned());
                }
                _ => {}
            }
        }
    }
    fn resolve(&self, name: &str) -> Option<&String> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }
    fn bind(&mut self, name: &mut String) {
        if self.preserve.contains(name) {
            self.scopes
                .last_mut()
                .unwrap()
                .insert(name.clone(), name.clone());
            return;
        }
        let replacement = self.names.fresh();
        self.scopes
            .last_mut()
            .expect("scope visitor enters a scope")
            .insert(std::mem::replace(name, replacement.clone()), replacement);
        self.count += 1;
    }
}

impl Scope for Renamer<'_> {
    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn pop(&mut self) {
        self.scopes.pop();
    }
    fn insert(&mut self, name: &mut String) {
        self.bind(name);
    }
    fn insert_self(&mut self) {
        self.scopes
            .last_mut()
            .unwrap()
            .insert("self".into(), "self".into());
    }
    fn insert_local(&mut self, name: &mut String, _: Option<&mut Expression>) {
        self.bind(name);
    }
    fn insert_local_function(&mut self, function: &mut FunctionAssignment) {
        self.bind(function.mutate_identifier().mutate_name());
    }
}

impl NodeProcessor for Renamer<'_> {
    fn process_type_function(&mut self, function: &mut TypeFunctionStatement) {
        // Type functions have an independent compile-time environment. Hide their
        // bodies from the runtime scope visitor and restore them after renaming.
        let placeholder = TypeFunctionStatement::from_name(
            function.get_identifier().get_name().as_str(),
            Block::default(),
        );
        self.type_functions
            .push_back(std::mem::replace(function, placeholder));
    }
    fn process_variable_expression(&mut self, identifier: &mut Identifier) {
        if let Some(name) = self.resolve(identifier.get_name()) {
            identifier.set_name(name);
        }
    }
    fn process_type_field(&mut self, field: &mut TypeField) {
        if let Some(name) = self.resolve(field.get_namespace().get_name()).cloned() {
            field.mutate_namespace().set_name(name);
        }
    }
}
