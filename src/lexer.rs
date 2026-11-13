//! Public lexical utilities. Transformations use the AST, not this token stream.
use full_moon::{
    tokenizer::{Lexer, LexerResult, TokenType},
    LuaVersion,
};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Name,
    Number,
    String,
    InterpolatedString,
    Keyword,
    Symbol,
    Comment,
    Directive,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: Kind,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct LexError {
    pub offset: usize,
    pub message: String,
}
impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}
impl std::error::Error for LexError {}

pub fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    let tokens = match Lexer::new(source, LuaVersion::luau()).collect() {
        LexerResult::Ok(tokens) => tokens,
        LexerResult::Fatal(errors) | LexerResult::Recovered(_, errors) => {
            let error = &errors[0];
            return Err(LexError {
                offset: error.position().bytes(),
                message: error.to_string(),
            });
        }
    };
    Ok(tokens
        .into_iter()
        .filter_map(|token| {
            let text = token.to_string();
            let kind = match token.token_type() {
                TokenType::Eof | TokenType::Whitespace { .. } => return None,
                TokenType::Identifier { .. } => Kind::Name,
                TokenType::Number { .. } => Kind::Number,
                TokenType::StringLiteral { .. } => Kind::String,
                TokenType::InterpolatedString { .. } => Kind::InterpolatedString,
                TokenType::SingleLineComment { .. } if text.starts_with("--!") => Kind::Directive,
                TokenType::SingleLineComment { .. }
                | TokenType::MultiLineComment { .. }
                | TokenType::Shebang { .. } => Kind::Comment,
                _ if text.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) => Kind::Keyword,
                _ => Kind::Symbol,
            };
            Some(Token { kind, text })
        })
        .collect())
}

pub fn needs_space(a: &Token, b: &Token) -> bool {
    let word = |k: &Kind| matches!(k, Kind::Name | Kind::Number | Kind::Keyword);
    if word(&a.kind) && word(&b.kind) {
        return true;
    }
    if (a.kind == Kind::Number && b.text.starts_with('.'))
        || (a.text.ends_with('.') && (b.kind == Kind::Number || b.text.starts_with('.')))
    {
        return true;
    }
    let joined = format!("{}{}", a.text, b.text);
    matches!(
        joined.as_str(),
        "--" | "//"
            | "//="
            | ".."
            | "..."
            | "..="
            | "=="
            | "~="
            | "<="
            | ">="
            | "+="
            | "-="
            | "*="
            | "/="
            | "%="
            | "^="
            | "::"
            | "->"
            | "[["
            | "<<"
            | ">>"
    ) || (a.text == "[" && b.kind == Kind::String && b.text.starts_with('['))
}

pub fn emit(tokens: &[Token]) -> String {
    let mut out = String::new();
    let mut previous = None;
    for token in tokens {
        if matches!(token.kind, Kind::Comment | Kind::Directive) {
            continue;
        }
        if previous.is_some_and(|p| needs_space(p, token)) {
            out.push(' ');
        }
        out.push_str(&token.text);
        previous = Some(token);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handles_luau_and_malformed_long_brackets_without_panicking() {
        assert!(lex("return `value={10 // 3}`").is_ok());
        assert!(lex("--[====[x]").is_err());
        assert!(lex("[====[x]").is_err());
    }
    #[test]
    fn keeps_numeric_concatenation_separate() {
        let tokens = lex("return 1 .. 'ok'").unwrap();
        assert_eq!(emit(&tokens), "return 1 ..'ok'");
    }
}
