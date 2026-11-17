//! Public, removable provenance markers, not authenticated signatures.
use crate::{
    lexer::{lex, Kind},
    syntax, ObfuscationError,
};
use darklua_core::nodes::{Block, Expression, Statement};

const MAGIC: &[u8] = b"ROBFWM\x01";
const PACKET_BYTES: usize = MAGIC.len() + 8 + 4;
pub const MAX_DETECTION_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatermarkDetection {
    pub visible_header: bool,
    pub embedded_watermark: bool,
    pub content_matches: bool,
}

impl WatermarkDetection {
    pub fn status(&self) -> &'static str {
        if self.content_matches {
            "matching_watermark"
        } else if self.embedded_watermark {
            "modified_watermark"
        } else if self.visible_header {
            "header_only"
        } else {
            "not_recognized"
        }
    }
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |state, byte| {
        (state ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
fn checksum(bytes: &[u8]) -> u32 {
    hash(bytes) as u32
}
fn fingerprint(block: &Block) -> u64 {
    hash(syntax::generate(block, true).as_bytes())
}

pub(crate) fn stamp(body: &Block) -> String {
    let mut packet = MAGIC.to_vec();
    packet.extend_from_slice(&fingerprint(body).to_le_bytes());
    packet.extend_from_slice(&checksum(&packet).to_le_bytes());
    let mut hidden = String::with_capacity(packet.len() * 8);
    for byte in packet {
        for bit in (0..8).rev() {
            hidden.push(if byte & (1 << bit) == 0 { ' ' } else { '\t' });
        }
    }
    // Actual spaces/tabs inside a literal; never invalid zero-width identifiers.
    // A temporary block-local binding adds no globals, calls or function frames.
    format!("do local _=[=[{hidden}]=]end;\n")
}

fn packet(bytes: &[u8]) -> Option<u64> {
    if bytes.len() != PACKET_BYTES * 8 {
        return None;
    }
    let mut decoded = Vec::with_capacity(PACKET_BYTES);
    for chunk in bytes.as_chunks::<8>().0 {
        let mut byte = 0;
        for &bit in chunk {
            byte = (byte << 1)
                | match bit {
                    b' ' => 0,
                    b'\t' => 1,
                    _ => return None,
                };
        }
        decoded.push(byte);
    }
    if !decoded.starts_with(MAGIC) {
        return None;
    }
    let end = PACKET_BYTES - 4;
    if checksum(&decoded[..end]) != u32::from_le_bytes(decoded[end..].try_into().ok()?) {
        return None;
    }
    Some(u64::from_le_bytes(
        decoded[MAGIC.len()..end].try_into().ok()?,
    ))
}

fn marker(statement: &Statement) -> Option<u64> {
    let Statement::Do(block) = statement else {
        return None;
    };
    let block = block.get_block();
    if block.get_last_statement().is_some() {
        return None;
    }
    let mut statements = block.iter_statements();
    let Statement::LocalAssign(assign) = statements.next()? else {
        return None;
    };
    if statements.next().is_some() || assign.iter_variables().count() != 1 {
        return None;
    }
    let mut values = assign.iter_values();
    let Expression::String(value) = values.next()? else {
        return None;
    };
    if values.next().is_some() {
        return None;
    }
    packet(value.get_value())
}

/// Inspect Luau data/AST only. A public marker is forgeable and removable.
pub fn detect_watermark(source: &str) -> Result<WatermarkDetection, ObfuscationError> {
    if source.len() > MAX_DETECTION_BYTES {
        return Err(ObfuscationError(
            "Detector limit: 8 MiB of UTF-8 source".into(),
        ));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        stacker::maybe_grow(16 * 1024 * 1024, 32 * 1024 * 1024, || detect_inner(source))
    }
    #[cfg(target_arch = "wasm32")]
    {
        detect_inner(source)
    }
}

fn detect_inner(source: &str) -> Result<WatermarkDetection, ObfuscationError> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let tokens = lex(source).map_err(|e| ObfuscationError(e.to_string()))?;
    let visible_header = tokens
        .iter()
        .take_while(|token| matches!(token.kind, Kind::Comment | Kind::Directive))
        .any(|token| {
            token.kind == Kind::Comment
                && token.text.starts_with("-- RealObfuscator [")
                && token
                    .text
                    .ends_with("] | https://github.com/linkoro57/RealObfuscator")
        });
    let mut block = syntax::parse(source)?;
    let mut marks = Vec::new();
    let statements = block
        .take_statements()
        .into_iter()
        .filter(|statement| {
            if let Some(value) = marker(statement) {
                marks.push(value);
                false
            } else {
                true
            }
        })
        .collect();
    block.set_statements(statements);
    let content_matches = if marks.is_empty() {
        false
    } else {
        let actual = fingerprint(&block);
        marks.iter().all(|expected| *expected == actual)
    };
    Ok(WatermarkDetection {
        visible_header,
        embedded_watermark: !marks.is_empty(),
        content_matches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ObfuscationConfig, Obfuscator};

    #[test]
    fn marker_survives_header_removal_and_formatting_across_profiles() {
        stacker::maybe_grow(16 * 1024 * 1024, 32 * 1024 * 1024, check_profiles);
    }

    fn check_profiles() {
        for mut config in [
            ObfuscationConfig::lightweight(),
            ObfuscationConfig::default(),
            ObfuscationConfig::hardened(),
            ObfuscationConfig::compatible(),
        ] {
            for pretty in [false, true] {
                config.minify = !pretty;
                let code = Obfuscator::new(config.clone().with_seed(42))
                    .obfuscate("local n=17;print(n,'hello');return n,nil")
                    .unwrap();
                assert_eq!(
                    detect_watermark(&code).unwrap().status(),
                    "matching_watermark"
                );
                let stripped = crate::lexer::emit(&lex(&code).unwrap());
                let detection = detect_watermark(&stripped).unwrap();
                assert!(detection.content_matches && !detection.visible_header);
                let reformatted = syntax::generate(&syntax::parse(&stripped).unwrap(), true);
                assert!(detect_watermark(&reformatted).unwrap().content_matches);
            }
        }
    }

    #[test]
    fn detector_separates_legacy_headers_tampering_and_unmarked_input() {
        let body = syntax::parse("return 17").unwrap();
        let mark = stamp(&body);
        assert!(
            detect_watermark(&format!("{mark}return 17"))
                .unwrap()
                .content_matches
        );
        assert_eq!(
            detect_watermark(&format!("{mark}return 18"))
                .unwrap()
                .status(),
            "modified_watermark"
        );
        assert_eq!(
            detect_watermark(
                "-- RealObfuscator [old] | https://github.com/linkoro57/RealObfuscator\nreturn 1"
            )
            .unwrap()
            .status(),
            "header_only"
        );
        assert_eq!(
            detect_watermark(
                "return '-- RealObfuscator [x] | https://github.com/linkoro57/RealObfuscator'"
            )
            .unwrap()
            .status(),
            "not_recognized"
        );
        assert!(detect_watermark("local =").is_err());
        let mut corrupt = mark.clone().into_bytes();
        let first = mark.find("[=[").unwrap() + 3;
        corrupt[first] = if corrupt[first] == b' ' { b'\t' } else { b' ' };
        let corrupt = String::from_utf8(corrupt).unwrap();
        assert!(
            !detect_watermark(&format!("{corrupt}return 17"))
                .unwrap()
                .embedded_watermark
        );
        assert!(detect_watermark(&" ".repeat(MAX_DETECTION_BYTES + 1)).is_err());
    }
}
