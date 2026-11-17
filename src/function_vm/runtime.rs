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
    decoder: &str,
) -> String {
    let mut source = include_str!("runtime.luau").to_owned();
    let invocation = |nested_execution| {
        let mut handlers = Vec::new();
        for (op, ids) in codes {
            for id in ids {
                handlers.push((
                    *id,
                    alias_handler(op, *id, nested_execution, config.vm_bind_strings),
                ));
            }
        }
        handlers.sort_unstable_by_key(|(id, _)| *id);
        include_str!("invocation.luau")
            .replace("__EXECUTE_BEGIN__",if nested_execution {"local function execute(pc)"} else {"local pc=entry"})
            .replace("__EXECUTE_END__",if nested_execution {"return 0,nil end;local signal,result=execute(entry);if signal==2 then return unpackValues(result,1,result.n) end"} else {""})
            .replace("__HANDLERS__",&dispatch_tree(&handlers))
    };
    source = source.replace(
        "__INVOCATION__",
        &if codes.contains_key(&Op::GenericFor) {
            format!(
                "if bridgedIteration then {}else {}end",
                invocation(true),
                invocation(false)
            )
        } else {
            invocation(false)
        },
    );
    source = source.replace(
        "__STRING_DECODER__",
        if config.vm_literal_programs {
            include_str!("literal_programs.luau")
        } else if config.vm_bind_strings {
            include_str!("bound_strings.luau")
        } else {
            ""
        },
    );
    for (op, placeholder) in [
        (Op::LiteralStart, "__LITERAL_START__"),
        (Op::LiteralXor, "__LITERAL_XOR__"),
        (Op::LiteralAdd, "__LITERAL_ADD__"),
        (Op::LiteralBytes, "__LITERAL_BYTES__"),
        (Op::LiteralReturn, "__LITERAL_RETURN__"),
    ] {
        let test = codes
            .get(&op)
            .map(|ids| {
                ids.iter()
                    .map(|id| format!("op=={id}"))
                    .collect::<Vec<_>>()
                    .join(" or ")
            })
            .unwrap_or_else(|| "false".into());
        source = source.replace(placeholder, &test);
    }
    source = source.replace("__RUNNER__", runner);
    // Unroll without allocating another record table on the rolling path.
    let mut words = String::from("local state=xorValue(seed,index,recipeState or 0)\n");
    for i in 1..=5 {
        words.push_str(&format!(
            "local cipher{i}=word(slot{i});local value{i}=xorValue(rotateRight(cipher{i},state%31+1),state);state=(xorValue(rotateLeft(state,7),cipher{i})+step+{i})%4294967296;\n"
        ));
    }
    words.push_str("return value1,value2,value3,value4,value5");
    source = source.replace("__WORD_DECODER__", &words);
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
    if config.vm_bind_strings {
        source.replace("decodeBound", decoder)
    } else {
        source
    }
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

fn constant(index: &str, bind_strings: bool) -> String {
    let arguments = if bind_strings {
        format!("decodeBound,prototype,{index}")
    } else {
        String::new()
    };
    format!("local value=values[{index}];if value==nil then value=factories[{index}]({arguments});values[{index}]=if value==nil then nilValue else value elseif value==nilValue then value=nil end;")
}

fn alias_handler(op: &Op, opcode: u32, nested: bool, bound: bool) -> String {
    let names = super::wire::operands(opcode).map(|slot| ["a", "b", "c"][slot]);
    // Replace complete identifiers only; temporaries such as `cells` stay intact.
    handler(op, nested, bound)
        .split_inclusive(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .map(|token| {
            let end = token
                .trim_end_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .len();
            let name = match &token[..end] {
                "a" => names[0],
                "b" => names[1],
                "c" => names[2],
                _ => return token.to_owned(),
            };
            format!("{name}{}", &token[end..])
        })
        .collect()
}

fn handler(op: &Op, nested_execution: bool, bind_strings: bool) -> String {
    match op {
        Op::Constant => return format!("{}r[a]=value", constant("b", bind_strings)),
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
        Op::ForPrepare => "local state=r[a];for _=state[1],state[2],state[3] do break end;state[1]*=1;state[2]*=1;state[3]*=1",
        Op::ForNext => "r[a][1]+=r[a][3]",
        Op::Format => "r[a]=formatValues(unpackValues(r[b],1,r[b].n))",
        Op::Native => "if c==0 then r[a]=external[b](r,unpackValues(varargs,1,varargs.n))else r[a]=packValues(external[b](r,unpackValues(varargs,1,varargs.n)))end",
        Op::GenericFor if nested_execution => "local signal,result=external[b](r,function()return execute(a)end,unpackValues(varargs,1,varargs.n));if signal==2 then return signal,result end;pc=c",
        Op::GenericFor => "fail()",
        Op::Signal => "return a,nil",
        Op::Bind => "r[a]={r[b]}",
        Op::CellGet => "r[a]=r[b][1]",
        Op::CellSet => "r[a][1]=r[b]",
        Op::Closure => "local cells={};for i=1,#r[c] do local id=r[c][i];cells[id]=r[id]end;r[a]=external[b](cells)",
        Op::LiteralStart | Op::LiteralXor | Op::LiteralAdd | Op::LiteralBytes | Op::LiteralReturn => "fail()",
        Op::Binary(operator) => return format!("r[a]=r[b]{operator}r[c]"),
        Op::BinaryConstant(operator) => return format!("{}r[a]=r[b]{operator}value", constant("c", bind_strings)),
        Op::Unary(operator) => return format!("r[a]={operator} r[b]"),
    }.to_owned()
}
