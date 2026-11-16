//! Generate the interpreter and its opcode dispatch tree.
use super::{instruction::Op, schema::Schema};
use crate::config::{ObfuscationConfig, VmHardening};
use std::collections::BTreeMap;

pub(super) fn runtime(
    runner: &str,
    schema: &Schema,
    codes: &BTreeMap<Op, Vec<u32>>,
    config: &ObfuscationConfig,
    hardening: VmHardening,
) -> String {
    let nested_execution = codes.contains_key(&Op::GenericFor);
    let mut source = include_str!("runtime.luau").to_owned();
    source = source.replace("__RUNNER__", runner);
    source = source.replace(
        "__EXECUTE_BEGIN__",
        if nested_execution {
            "local function execute(pc)"
        } else {
            "local pc=entry"
        },
    );
    source = source.replace("__EXECUTE_END__", if nested_execution {
        "return 0,nil end;local signal,result=execute(entry);if signal==2 then return unpackValues(result,1,result.n) end"
    } else { "" });
    source = schema.apply(source);
    source = source.replace(
        "__PACKED__",
        if config.vm_pack_bytecode {
            "true"
        } else {
            "false"
        },
    );
    source = source.replace(
        "__CACHED__",
        if config.vm_predecode_bytecode {
            "true"
        } else {
            "false"
        },
    );
    source = source.replace(
        "__INTEGRITY__",
        if config.anti_tamper { "true" } else { "false" },
    );
    source = source.replace(
        "__EPHEMERAL__",
        if hardening == VmHardening::Max {
            "true"
        } else {
            "false"
        },
    );
    let mut handlers = Vec::new();
    for (op, ids) in codes {
        for id in ids {
            handlers.push((*id, handler(op, nested_execution)));
        }
    }
    handlers.sort_unstable_by_key(|(id, _)| *id);
    source.replace("__HANDLERS__", &dispatch_tree(&handlers))
}

fn dispatch_tree(handlers: &[(u32, String)]) -> String {
    if handlers.len() == 1 {
        let (id, code) = &handlers[0];
        return format!("if op=={id} then {code} else fail()end;");
    }
    let middle = handlers.len() / 2;
    format!(
        "if op<{} then {}else {}end;",
        handlers[middle].0,
        dispatch_tree(&handlers[..middle]),
        dispatch_tree(&handlers[middle..])
    )
}

fn constant(index: &str) -> String {
    format!("local value=values[{index}];if value==nil then value=factories[{index}]();values[{index}]=if value==nil then nilValue else value elseif value==nilValue then value=nil end;")
}

fn handler(op: &Op, nested_execution: bool) -> String {
    match op {
        Op::Constant => return format!("{}r[a]=value", constant("b")),
        Op::Move => "r[a]=r[b]",
        Op::Read => "r[a]=external[b]()",
        Op::Write => "external[a](r[b],true)",
        Op::Index => "r[a]=r[b][r[c]]",
        Op::SetIndex => "r[a][r[b]]=r[c]",
        Op::Table => "r[a]=createTable(b)",
        Op::TableExtend => "local values=r[b];local expanded=createTable(c-1+values.n);for key,value in nextValue,r[a] do expanded[key]=value end;for i=1,values.n do expanded[c+i-1]=values[i]end;r[a]=expanded",
        Op::List => "r[a]={n=0}",
        Op::Push => "local list=r[a];list.n+=1;list[list.n]=r[b]",
        Op::Extend => "local list=r[a];local values=r[b];for i=1,values.n do list[list.n+i]=values[i]end;list.n+=values.n",
        Op::Item => "r[a]=r[b][c]",
        Op::Call => "r[a]=r[b](unpackValues(r[c],1,r[c].n))",
        Op::CallMany => "r[a]=packValues(r[b](unpackValues(r[c],1,r[c].n)))",
        Op::Vararg => "r[a]=varargs[1]",
        Op::VarargMany => "r[a]=varargs",
        Op::Return if nested_execution => "return 2,r[a]",
        Op::Return => "return unpackValues(r[a],1,r[a].n)",
        Op::Jump => "pc=a",
        Op::JumpFalse => "if not r[a]then pc=b end",
        Op::JumpTrue => "if r[a]then pc=b end",
        Op::ForCheck => "local state=r[b];r[a]=(state[3]>0 and state[1]<=state[2])or(state[3]<=0 and state[1]>=state[2])",
        Op::ForNext => "r[a][1]+=r[a][3]",
        Op::Format => "r[a]=formatValues(unpackValues(r[b],1,r[b].n))",
        Op::Native => "if c==0 then r[a]=external[b](r,unpackValues(varargs,1,varargs.n))else r[a]=packValues(external[b](r,unpackValues(varargs,1,varargs.n)))end",
        Op::GenericFor => "local signal,result=external[b](r,function()return execute(a)end,unpackValues(varargs,1,varargs.n));if signal==2 then return signal,result end;pc=c",
        Op::Signal => "return a,nil",
        Op::Bind => "r[a]={r[b]}",
        Op::CellGet => "r[a]=r[b][1]",
        Op::CellSet => "r[a][1]=r[b]",
        Op::Closure => "local cells={};for i=1,#r[c] do local id=r[c][i];cells[id]=r[id]end;r[a]=external[b](cells)",
        Op::Binary(operator) => return format!("r[a]=r[b]{operator}r[c]"),
        Op::BinaryConstant(operator) => return format!("{}r[a]=r[b]{operator}value", constant("c")),
        Op::Unary(operator) => return format!("r[a]={operator} r[b]"),
    }.to_owned()
}
