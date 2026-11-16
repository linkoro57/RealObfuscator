//! Columnar serialization of compiler-owned tables. Function bodies are opaque
//! leaves: their literals, lexical bindings and factory protocol stay intact.
use crate::{names::Names, obfuscator::ObfuscationReport, rng::Rng, syntax};
use darklua_core::nodes::*;

struct Entry {
    owner: usize,
    key: Expression,
    value: Expression,
    reference: Option<usize>,
}

#[derive(Default)]
struct Graph {
    tables: usize,
    entries: Vec<Entry>,
}

impl Graph {
    fn collect(&mut self, table: &TableExpression) -> usize {
        let owner = self.tables;
        self.tables += 1;
        let mut implicit = 0;
        for entry in table.iter_entries() {
            let (key, value) = match entry {
                TableEntry::Value(value) => {
                    implicit += 1;
                    (Expression::from(implicit as f64), value.as_ref())
                }
                TableEntry::Index(entry) => (entry.get_key().clone(), entry.get_value()),
                TableEntry::Field(entry) => (
                    StringExpression::from_value(entry.get_field().get_name().as_str()).into(),
                    entry.get_value(),
                ),
            };
            let reference = match value {
                Expression::Table(table) => Some(self.collect(table)),
                _ => None,
            };
            self.entries.push(Entry {
                owner,
                key,
                value: if reference.is_some() {
                    Expression::nil()
                } else {
                    value.clone()
                },
                reference,
            });
        }
        owner
    }
}

fn column(mut values: Vec<TableEntry>, rng: &mut Rng) -> Expression {
    rng.shuffle(&mut values);
    TableExpression::new(values).into()
}

/// Each column has its own physical order. Rebuild allocates all nodes before
/// linking them, including empty nodes. No per-instruction cost is introduced.
pub(super) fn emit(
    bank: &str,
    table: TableExpression,
    names: &mut Names,
    report: &mut ObfuscationReport,
) -> Block {
    let mut graph = Graph::default();
    let root = graph.collect(&table);
    report.vm_scattered_tables += graph.tables;
    report.vm_scattered_entries += graph.entries.len();
    let mut identities: Vec<_> = (1..=graph.tables).collect();
    names.rng.shuffle(&mut identities);
    names.rng.shuffle(&mut graph.entries);
    let mut owners = Vec::new();
    let mut keys = Vec::new();
    let mut values = Vec::new();
    let mut references = Vec::new();
    for (index, entry) in graph.entries.into_iter().enumerate() {
        let id = (index + 1) as f64;
        owners.push(TableIndexEntry::new(id, identities[entry.owner] as f64).into());
        keys.push(TableIndexEntry::new(id, entry.key).into());
        values.push(TableIndexEntry::new(id, entry.value).into());
        references.push(
            TableIndexEntry::new(
                id,
                entry.reference.map_or(0, |node| identities[node]) as f64,
            )
            .into(),
        );
    }
    let rebuild = names.fresh();
    let mut block = syntax::generated(&format!(
        "local function {rebuild}(destinations,keys,values,links,count,root)\
         local nodes=table.create(count);\
         for i=1,count do nodes[i]={{}} end;\
         for i=1,#destinations do \
         local target=links[i];\
         if target==0 then nodes[destinations[i]][keys[i]]=values[i] \
         else nodes[destinations[i]][keys[i]]=nodes[target] end end;\
         return nodes[root] end;"
    ));
    let arguments = vec![
        column(owners, &mut names.rng),
        column(keys, &mut names.rng),
        column(values, &mut names.rng),
        column(references, &mut names.rng),
        (graph.tables as f64).into(),
        (identities[root] as f64).into(),
    ];
    block.push_statement(VariableAssignment::from_variable(bank).with_value(
        FunctionCall::from_name(rebuild).with_arguments(TupleArguments::new(arguments)),
    ));
    block
}
