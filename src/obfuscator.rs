use crate::config::ObfuscationConfig;
use crate::lexer::{emit, lex, Kind, Token};
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Debug)] pub struct ObfuscationError(pub String);
impl fmt::Display for ObfuscationError{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{write!(f,"Obfuscation failed: {}",self.0)}}
impl std::error::Error for ObfuscationError{}

pub struct Obfuscator { pub config: ObfuscationConfig }

impl Obfuscator {
    pub fn new(config: ObfuscationConfig) -> Self { Self{config} }
    pub fn obfuscate(&self, source:&str)->Result<String,ObfuscationError>{
        self.config.validate().map_err(ObfuscationError)?;
        let directives:Vec<&str>=source.lines().map(str::trim).take_while(|l|l.is_empty()||l.starts_with("--"))
            .filter(|l|matches!(*l,"--!native"|"--!optimize 0"|"--!optimize 1"|"--!optimize 2")).collect();
        let cleaned=if self.config.strip_types{strip_type_aliases(source)}else{source.to_owned()};
        let mut tokens=lex(&cleaned).map_err(|e|ObfuscationError(e.to_string()))?;
        tokens.retain(|t|!matches!(t.kind,Kind::Comment|Kind::Directive));
        let mut rng=self.config.rng.clone();
        if self.config.rename_identifiers { rename_locals(&mut tokens,&mut rng,self.config.name_min_length,self.config.name_max_length); }
        let mut prefix=String::new();
        if self.config.encrypt_strings {
            let decoder=rng.name(8,12); let key=rng.range(1,256) as u8;
            let changed=encrypt_strings(&mut tokens,&decoder,key);
            if changed { prefix=format!("local {decoder}=function(s)local t={{}};for i=1,#s do t[i]=string.char(bit32.bxor(string.byte(s,i),{key}))end;return table.concat(t)end;"); }
        }
        if self.config.obfuscate_numbers { obfuscate_numbers(&mut tokens,&mut rng,self.config.skip_trivial_numbers); }
        if self.config.inject_dead_code && !self.config.virtualize {
            let n=rng.name(8,12); let a=rng.range(100,999); prefix.push_str(&format!("local function {n}()if false then return {a} end end;"));
        }
        let body=format!("{prefix}{}",emit(&tokens));
        let signature=if self.config.virtualize{"RealObfuscator VM"}else{"RealObfuscator"};
        let mut output=format!("--!nocheck\n--!nolint\n-- {signature} [{}]\n{body}",self.config.build_id);
        if !self.config.virtualize&&!directives.is_empty(){output=format!("{}\n{}",directives.join("\n"),output);}
        Ok(output)
    }
}

impl Default for Obfuscator{fn default()->Self{Self::new(ObfuscationConfig::default())}}
pub fn obfuscate(source:&str,config:Option<ObfuscationConfig>)->Result<String,ObfuscationError>{Obfuscator::new(config.unwrap_or_default()).obfuscate(source)}

fn strip_type_aliases(source:&str)->String{
    source.lines().map(|line|{let t=line.trim_start();if (t.starts_with("type ")||t.starts_with("export type "))&&t.contains('='){""}else{line}}).collect::<Vec<_>>().join("\n")
}

fn rename_locals(tokens:&mut[Token],rng:&mut crate::rng::Rng,min:usize,max:usize){
    let mut declared=HashSet::new();let mut expect_local=false;let mut in_params=0usize;let mut for_names=false;
    for i in 0..tokens.len(){
        let text=tokens[i].text.as_str();
        if text=="local"{expect_local=true;continue;}
        if text=="function"{
            if expect_local { if let Some(next)=tokens.get(i+1){if next.kind==Kind::Name{declared.insert(next.text.clone());}} expect_local=false; }
            continue;
        }
        if text=="for"{for_names=true;continue;}
        if for_names { if text=="="||text=="in"{for_names=false;}else if tokens[i].kind==Kind::Name{declared.insert(tokens[i].text.clone());} continue; }
        if expect_local {
            if tokens[i].kind==Kind::Name{declared.insert(tokens[i].text.clone());continue;}
            if text==","{continue;} expect_local=false;
        }
        if text=="("&&i>0&&(tokens[i-1].text=="function"||tokens[..i].iter().rev().take(3).any(|t|t.text=="function")){in_params=1;continue;}
        if in_params>0 { if text=="("{in_params+=1;}else if text==")"{in_params-=1;}else if in_params==1&&tokens[i].kind==Kind::Name{declared.insert(tokens[i].text.clone());} }
    }
    declared.remove("self");
    let mut map=HashMap::new();
    for name in declared{let mut candidate=rng.name(min,max);while map.values().any(|x|x==&candidate){candidate=rng.name(min,max);}map.insert(name,candidate);}
    for i in 0..tokens.len(){if tokens[i].kind!=Kind::Name{continue;}let property=i>0&&matches!(tokens[i-1].text.as_str(),"."|":");let named_key=tokens.get(i+1).is_some_and(|n|n.text=="=")&&i>0&&tokens[i-1].text!="local";if !property&&!named_key{if let Some(new)=map.get(&tokens[i].text){tokens[i].text=new.clone();}}}
}

fn decode_quoted(raw:&str)->Option<Vec<u8>>{
    let q=raw.as_bytes();if q.len()<2||!matches!(q[0],b'\''|b'"'){return None;}let mut out=Vec::new();let mut i=1;
    while i+1<q.len(){if q[i]!=b'\\'{out.push(q[i]);i+=1;continue;}i+=1;if i+1>=q.len(){return None;}match q[i]{b'n'=>out.push(b'\n'),b'r'=>out.push(b'\r'),b't'=>out.push(b'\t'),b'a'=>out.push(7),b'b'=>out.push(8),b'f'=>out.push(12),b'v'=>out.push(11),b'\\'=>out.push(b'\\'),b'\''=>out.push(b'\''),b'"'=>out.push(b'"'),b'x'=>{if i+2>=q.len(){return None;}let s=std::str::from_utf8(&q[i+1..i+3]).ok()?;out.push(u8::from_str_radix(s,16).ok()?);i+=2;},d if d.is_ascii_digit()=>{let start=i;let mut end=i+1;while end<q.len()-1&&end<start+3&&q[end].is_ascii_digit(){end+=1;}out.push(std::str::from_utf8(&q[start..end]).ok()?.parse::<u8>().ok()?);i=end-1;},other=>out.push(other)}i+=1;}Some(out)
}

fn encrypt_strings(tokens:&mut[Token],decoder:&str,key:u8)->bool{
    let mut changed=false;for token in tokens.iter_mut(){if token.kind!=Kind::String{continue;}if let Some(bytes)=decode_quoted(&token.text){let mut literal=String::from("\"");for b in bytes{literal.push_str(&format!("\\{:03}",b^key));}literal.push('"');token.kind=Kind::Name;token.text=format!("{decoder}({literal})");changed=true;}}changed
}

fn obfuscate_numbers(tokens:&mut[Token],rng:&mut crate::rng::Rng,skip:bool){
    for token in tokens.iter_mut(){if token.kind!=Kind::Number{continue;}let clean=token.text.replace('_',"");let value=if clean.starts_with("0x")||clean.starts_with("0X"){i64::from_str_radix(&clean[2..],16).ok()}else if clean.starts_with("0b")||clean.starts_with("0B"){i64::from_str_radix(&clean[2..],2).ok()}else if clean.contains(['.','e','E']){None}else{clean.parse().ok()};if let Some(n)=value{if n.abs()>0xff_ffff||(skip&&matches!(n,-1|0|1)){continue;}let r=rng.range(10,501)as i64;token.kind=Kind::Name;token.text=format!("({}-{})",n+r,r);}}
}

#[cfg(test)]mod tests{use super::*;use crate::config::ProtectionLevel;
 #[test]fn deterministic(){let c=ObfuscationConfig::default().with_seed(42).with_level(ProtectionLevel::Minimal);assert_eq!(Obfuscator::new(c.clone()).obfuscate("local x=17;return x,'hi'").unwrap(),Obfuscator::new(c).obfuscate("local x=17;return x,'hi'").unwrap());}
 #[test]fn rejects_template(){assert!(Obfuscator::default().obfuscate("return `bad`").is_err());}
}
