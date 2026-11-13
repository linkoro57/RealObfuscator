use crate::obfuscator::ObfuscationError;
use darklua_core::{
    generator::{DenseLuaGenerator, LuaGenerator, ReadableLuaGenerator},
    nodes::*,
    Parser,
};

pub(crate) fn parse(source: &str) -> Result<Block, ObfuscationError> {
    Parser::default()
        .parse(source)
        .map_err(|e| ObfuscationError(e.to_string()))
}

pub(crate) fn generate(block: &Block, minify: bool) -> String {
    if minify {
        let mut generator = DenseLuaGenerator::new(usize::MAX);
        generator.write_block(block);
        generator.into_string()
    } else {
        let mut generator = ReadableLuaGenerator::default();
        generator.write_block(block);
        generator.into_string()
    }
}

/// Only generated snippets use this; input errors always propagate through `parse`.
pub(crate) fn generated(source: &str) -> Block {
    parse(source).unwrap_or_else(|e| panic!("invalid generated Luau: {e}\n{source}"))
}

pub(crate) fn byte_literal(bytes: &[u8]) -> String {
    let mut output = String::from("\"");
    for byte in bytes {
        output.push_str(&format!("\\{byte:03}"));
    }
    output.push('"');
    output
}
