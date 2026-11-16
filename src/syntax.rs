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
    let mut output = String::with_capacity(bytes.len().saturating_mul(4).saturating_add(2));
    output.push('"');
    for byte in bytes {
        output.push('\\');
        output.push(char::from(b'0' + byte / 100));
        output.push(char::from(b'0' + (byte / 10) % 10));
        output.push(char::from(b'0' + byte % 10));
    }
    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_byte_literals_round_trip_every_byte() {
        for bytes in [Vec::new(), (0..=255).collect()] {
            let literal = byte_literal(&bytes);
            assert_eq!(literal.len(), bytes.len() * 4 + 2);
            let block = parse(&format!("return {literal}")).unwrap();
            let Some(LastStatement::Return(ret)) = block.get_last_statement() else {
                panic!("expected return")
            };
            let Some(Expression::String(string)) = ret.iter_expressions().next() else {
                panic!("expected byte string")
            };
            assert_eq!(string.get_value(), bytes);
        }
    }
}
