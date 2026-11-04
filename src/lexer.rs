use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind { Name, Number, String, Keyword, Symbol, Comment, Directive }

#[derive(Clone, Debug)]
pub struct Token { pub kind: Kind, pub text: String }

#[derive(Clone, Debug)]
pub struct LexError { pub offset: usize, pub message: String }
impl fmt::Display for LexError { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{} at byte {}", self.message, self.offset) } }
impl std::error::Error for LexError {}

fn keyword(s: &str) -> bool {
    matches!(s, "and"|"break"|"continue"|"do"|"else"|"elseif"|"end"|"export"|"false"|"for"|
        "function"|"if"|"in"|"local"|"nil"|"not"|"or"|"repeat"|"return"|"then"|"true"|"type"|"until"|"while")
}

fn long_bracket(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    if bytes.get(at) != Some(&b'[') { return None; }
    let mut i = at + 1;
    while bytes.get(i) == Some(&b'=') { i += 1; }
    (bytes.get(i) == Some(&b'[')).then_some((i - at - 1, i + 1))
}

pub fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    let b = source.as_bytes(); let mut out = Vec::new(); let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() { i += 1; continue; }
        if b[i] == b'-' && b.get(i+1) == Some(&b'-') {
            let start = i; i += 2;
            if let Some((eq, body)) = long_bracket(b, i) {
                i = body; let mut found = false;
                while i < b.len() { if b[i] == b']' && b.get(i+eq+1) == Some(&b']') && b[i+1..i+eq+1].iter().all(|x| *x==b'=') { i += eq+2; found=true; break; } i+=1; }
                if !found { return Err(LexError{offset:start,message:"unterminated block comment".into()}); }
            } else { while i < b.len() && b[i] != b'\n' { i += 1; } }
            let text = source[start..i].to_owned();
            let kind = if text.starts_with("--!") { Kind::Directive } else { Kind::Comment };
            out.push(Token{kind,text}); continue;
        }
        if b[i] == b'`' { return Err(LexError{offset:i,message:"interpolated strings are not supported".into()}); }
        if b[i] == b'\'' || b[i] == b'"' {
            let start=i; let quote=b[i]; i+=1; let mut closed=false;
            while i<b.len() { if b[i]==b'\\' { i+=1; if i<b.len(){i+=1;} } else if b[i]==quote {i+=1;closed=true;break;} else {i+=1;} }
            if !closed{return Err(LexError{offset:start,message:"unterminated string".into()});}
            out.push(Token{kind:Kind::String,text:source[start..i].into()}); continue;
        }
        if let Some((eq, body))=long_bracket(b,i) {
            let start=i; i=body; let mut found=false;
            while i<b.len(){if b[i]==b']'&&b.get(i+eq+1)==Some(&b']')&&b[i+1..i+eq+1].iter().all(|x|*x==b'='){i+=eq+2;found=true;break;}i+=1;}
            if !found{return Err(LexError{offset:start,message:"unterminated long string".into()});}
            out.push(Token{kind:Kind::String,text:source[start..i].into()});continue;
        }
        if b[i].is_ascii_alphabetic() || b[i]==b'_' {
            let start=i;i+=1;while i<b.len()&&(b[i].is_ascii_alphanumeric()||b[i]==b'_'){i+=1;}
            let text=&source[start..i];out.push(Token{kind:if keyword(text){Kind::Keyword}else{Kind::Name},text:text.into()});continue;
        }
        if b[i].is_ascii_digit() || (b[i]==b'.'&&b.get(i+1).is_some_and(u8::is_ascii_digit)) {
            let start=i;
            if b[i]==b'0'&&matches!(b.get(i+1),Some(b'x'|b'X')){i+=2;while i<b.len()&&(b[i].is_ascii_hexdigit()||b[i]==b'_'){i+=1;}}
            else if b[i]==b'0'&&matches!(b.get(i+1),Some(b'b'|b'B')){i+=2;while i<b.len()&&matches!(b[i],b'0'|b'1'|b'_'){i+=1;}}
            else { while i<b.len()&&(b[i].is_ascii_digit()||b[i]==b'_'||b[i]==b'.'){if b[i]==b'.'&&b.get(i+1)==Some(&b'.'){break;}i+=1;} if matches!(b.get(i),Some(b'e'|b'E')){i+=1;if matches!(b.get(i),Some(b'+'|b'-')){i+=1;}while i<b.len()&&(b[i].is_ascii_digit()||b[i]==b'_'){i+=1;}} }
            out.push(Token{kind:Kind::Number,text:source[start..i].into()});continue;
        }
        const OPS:[&str;12]=["...","..=","==","~=","<=",">=","+=","-=","*=","/=","%=","^="];
        if let Some(op)=OPS.iter().find(|op|source[i..].starts_with(**op)){out.push(Token{kind:Kind::Symbol,text:(*op).into()});i+=op.len();continue;}
        if source[i..].starts_with("..") {out.push(Token{kind:Kind::Symbol,text:"..".into()});i+=2;continue;}
        let ch=source[i..].chars().next().unwrap();
        if "+-*/%^#=<>();:{},.[]".contains(ch){out.push(Token{kind:Kind::Symbol,text:ch.to_string()});i+=ch.len_utf8();continue;}
        return Err(LexError{offset:i,message:format!("unsupported character {ch:?}")});
    }
    Ok(out)
}

pub fn needs_space(a:&Token,b:&Token)->bool{
    let word=|k:&Kind|matches!(k,Kind::Name|Kind::Number|Kind::Keyword);
    word(&a.kind)&&word(&b.kind) || (a.text=="-"&&b.text=="-") || (a.text=="."&&b.text.starts_with('.'))
}

pub fn emit(tokens:&[Token])->String{
    let mut out=String::new();let mut prev:Option<&Token>=None;
    for token in tokens { if matches!(token.kind,Kind::Comment|Kind::Directive){continue;} if prev.is_some_and(|p|needs_space(p,token)){out.push(' ');}out.push_str(&token.text);prev=Some(token); }
    out
}

#[cfg(test)] mod tests{
    use super::*;
    #[test]fn strings_keep_comments(){let t=lex("local x='-- ok' -- gone\nreturn x").unwrap();assert_eq!(emit(&t),"local x='-- ok'return x");}
    #[test]fn rejects_templates(){assert!(lex("return `x`").is_err());}
}
