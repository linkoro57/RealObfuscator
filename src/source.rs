//! Preserve Luau source bytes while adapting frontend edge cases.
use crate::obfuscator::ObfuscationError;
use full_moon::{
    tokenizer::{Lexer, LexerResult, TokenType},
    LuaVersion,
};
use std::borrow::Cow;

fn line_endings(source: &str) -> Cow<'_, str> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    if !source.contains('\r') {
        return Cow::Borrowed(source);
    }
    // Normalize Windows newlines without changing bare CR bytes in long strings.
    Cow::Owned(source.replace("\r\n", "\n"))
}

pub(crate) fn normalize(source: &str) -> Result<Cow<'_, str>, ObfuscationError> {
    let source = line_endings(source);
    if !source.contains("\\u{") && !source.contains("\\\r") {
        return Ok(source);
    }
    let tokens = match Lexer::new(&source, LuaVersion::luau()).collect() {
        LexerResult::Ok(tokens) => tokens,
        LexerResult::Fatal(errors) | LexerResult::Recovered(_, errors) => {
            return Err(ObfuscationError(errors[0].to_string()))
        }
    };
    let mut result = String::with_capacity(source.len());
    for token in tokens {
        let text = token.to_string();
        if matches!(
            token.token_type(),
            TokenType::StringLiteral { .. } | TokenType::InterpolatedString { .. }
        ) && !text.starts_with('[')
        {
            let bytes = text.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'\\'
                    && bytes.get(i + 1) == Some(&b'u')
                    && bytes.get(i + 2) == Some(&b'{')
                {
                    if let Some(end) = bytes[i + 3..]
                        .iter()
                        .position(|&b| b == b'}')
                        .map(|n| i + 3 + n)
                    {
                        if let Ok(value) = u32::from_str_radix(&text[i + 3..end], 16) {
                            // Luau permits UTF-8 encodings of surrogate codepoints.
                            // Rust char does not; feed equivalent byte escapes to the AST.
                            if (0xd800..=0xdfff).contains(&value) {
                                for byte in [
                                    0xe0 | (value >> 12) as u8,
                                    0x80 | ((value >> 6) & 63) as u8,
                                    0x80 | (value & 63) as u8,
                                ] {
                                    result.push_str(&format!("\\{byte:03}"));
                                }
                                i = end + 1;
                                continue;
                            }
                        }
                    }
                }
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    let ch = text[i + 1..].chars().next().unwrap();
                    result.push('\\');
                    result.push(if ch == '\r' { 'n' } else { ch });
                    i += 1 + ch.len_utf8();
                } else {
                    let ch = text[i..].chars().next().unwrap();
                    result.push(ch);
                    i += ch.len_utf8();
                }
            }
        } else {
            result.push_str(&text);
        }
    }
    Ok(Cow::Owned(result))
}

fn long_open(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    if bytes.get(at) != Some(&b'[') {
        return None;
    }
    let mut end = at + 1;
    while bytes.get(end) == Some(&b'=') {
        end += 1;
    }
    (bytes.get(end) == Some(&b'[')).then_some((end - at - 1, end + 1))
}
fn long_end(bytes: &[u8], start: usize, equals: usize) -> Option<(usize, usize)> {
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == b']'
            && bytes
                .get(i + 1..i + equals + 1)
                .is_some_and(|s| s.iter().all(|&b| b == b'='))
            && bytes.get(i + equals + 1) == Some(&b']')
        {
            return Some((i, i + equals + 2));
        }
        i += 1;
    }
    None
}

/// Non-UTF-8 bytes are accepted inside strings/comments, never silently replaced
/// in executable source. The returned editor representation is valid UTF-8.
pub fn decode_source_bytes(bytes: &[u8]) -> Result<String, ObfuscationError> {
    if let Ok(source) = std::str::from_utf8(bytes) {
        return Ok(source.to_owned());
    }
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    #[derive(Clone, Copy)]
    enum Mode {
        Code(usize),
        Quoted(u8),
        Interpolation,
    }
    let mut modes = vec![Mode::Code(0)];
    let mut result = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match *modes.last().unwrap() {
            Mode::Quoted(quote) => {
                let b = bytes[i];
                if b == quote {
                    result.push(b as char);
                    modes.pop();
                    i += 1;
                } else if b == b'\\' && i + 1 < bytes.len() {
                    if bytes[i + 1] >= 128 {
                        result.push_str(&format!("\\{:03}", bytes[i + 1]));
                    } else {
                        result.push('\\');
                        result.push(bytes[i + 1] as char);
                    }
                    i += 2;
                } else if b >= 128 {
                    result.push_str(&format!("\\{b:03}"));
                    i += 1;
                } else {
                    result.push(b as char);
                    i += 1;
                }
            }
            Mode::Interpolation => {
                let b = bytes[i];
                if b == b'`' {
                    result.push('`');
                    modes.pop();
                    i += 1;
                } else if b == b'{' {
                    result.push('{');
                    modes.push(Mode::Code(1));
                    i += 1;
                } else if b == b'\\' && i + 1 < bytes.len() {
                    if bytes[i + 1] >= 128 {
                        result.push_str(&format!("\\{:03}", bytes[i + 1]));
                    } else {
                        result.push('\\');
                        result.push(bytes[i + 1] as char);
                    }
                    i += 2;
                } else if b >= 128 {
                    result.push_str(&format!("\\{b:03}"));
                    i += 1;
                } else {
                    result.push(b as char);
                    i += 1;
                }
            }
            Mode::Code(depth) => {
                if bytes.get(i..i + 2) == Some(b"--") {
                    let end = if let Some((equals, start)) = long_open(bytes, i + 2) {
                        long_end(bytes, start, equals)
                            .ok_or_else(|| ObfuscationError("unfinished long comment".into()))?
                            .1
                    } else {
                        bytes[i..]
                            .iter()
                            .position(|&b| b == b'\n' || b == b'\r')
                            .map_or(bytes.len(), |n| i + n)
                    };
                    result.extend(
                        bytes[i..end]
                            .iter()
                            .map(|&b| if b < 128 { b as char } else { '?' }),
                    );
                    i = end;
                } else if let Some((equals, start)) = long_open(bytes, i) {
                    let (end, next) = long_end(bytes, start, equals)
                        .ok_or_else(|| ObfuscationError("unfinished long string".into()))?;
                    let mut content = Vec::new();
                    let mut j = start;
                    if bytes.get(j..j + 2) == Some(b"\r\n") {
                        j += 2;
                    } else if bytes.get(j) == Some(&b'\n') {
                        j += 1;
                    }
                    while j < end {
                        if bytes.get(j..j + 2) == Some(b"\r\n") {
                            content.push(b'\n');
                            j += 2;
                        } else {
                            content.push(bytes[j]);
                            j += 1;
                        }
                    }
                    result.push_str(&crate::syntax::byte_literal(&content));
                    i = next;
                } else {
                    let b = bytes[i];
                    match b {
                        b'\'' | b'"' => modes.push(Mode::Quoted(b)),
                        b'`' => modes.push(Mode::Interpolation),
                        b'{' if depth > 0 => *modes.last_mut().unwrap() = Mode::Code(depth + 1),
                        b'}' if depth == 1 => {
                            modes.pop();
                        }
                        b'}' if depth > 1 => *modes.last_mut().unwrap() = Mode::Code(depth - 1),
                        128..=255 => {
                            return Err(ObfuscationError(format!(
                                "non-UTF-8 source byte outside a string/comment at byte {i}"
                            )))
                        }
                        _ => {}
                    }
                    result.push(b as char);
                    i += 1;
                }
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_newlines_and_surrogates_without_touching_comments_or_escaped_slashes() {
        let source = "-- \\u{D800}\r\nreturn '\\u{D800}', '\\\\u{D800}', `\\u{DFFF}`";
        let result = normalize(source).unwrap();
        assert!(result.starts_with("-- \\u{D800}\n"));
        assert!(result.contains("'\\237\\160\\128'"));
        assert!(result.contains("'\\\\u{D800}'"));
        assert!(result.contains("`\\237\\191\\191`"));
    }
    #[test]
    fn byte_input_preserves_raw_literals_and_rejects_invalid_identifiers() {
        let result = decode_source_bytes(
            b"--\xff\nreturn '\xff', [=[\r\n\xfe\r\nx]=], `\xfd { {x='\xfc'}.x }`",
        )
        .unwrap();
        assert!(result.contains("'\\255'"));
        assert!(result.contains("\"\\254\\010\\120\""));
        assert!(result.contains("`\\253"));
        assert!(decode_source_bytes(b"local \xff=1").is_err());
    }
}
