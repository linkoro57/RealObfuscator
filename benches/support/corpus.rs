//! Deterministic workloads; every module returns a repeatable numeric checksum.
use std::fmt::Write;

pub const WORKLOADS: &[&str] = &[
    "arithmetic",
    "branches",
    "numeric_loop",
    "tables",
    "method_calls",
    "closures",
    "recursion",
    "varargs",
    "generic_iteration",
    "interpolation",
    "repeated_strings",
    "unique_strings",
    "large_block",
    "many_functions",
    "typed",
    "fallback",
    "while_loop",
    "repeat_loop",
    "properties",
    "custom_iteration",
    "metatables",
    "binary_strings",
    "shadowing",
    "comments",
];

pub fn source(workload: &str, scale: usize) -> String {
    let mut body = match workload {
        "arithmetic" => "local n=17;for i=1,SCALE do n=(n*13+i*7)%100003 end;total+=n".into(),
        "branches" => "local n=0;for i=1,SCALE do if i%3==0 then n+=i elseif i%2==0 then n-=i else n+=7 end end;total+=n".into(),
        "numeric_loop" => "local n=0;for i=1,SCALE do for j=1,8 do n=(n+i*j)%100003 end end;total+=n".into(),
        "while_loop" => "local i,n=0,0;while i<SCALE do i+=1;if i%3==0 then continue end;n+=i end;total+=n".into(),
        "repeat_loop" => "local i,n=0,0;repeat i+=1;local k=i*3;n+=k until i>=SCALE;total+=n".into(),
        "tables" => "local t={};for i=1,SCALE do t[i]={i,i*2,i*3} end;for i=1,#t do total+=t[i][2] end".into(),
        "properties" => "local t={value=0,step=3};for i=1,SCALE do t.value+=t.step;t.step=(t.step+1)%17 end;total+=t.value".into(),
        "method_calls" => "local t={value=0};function t:add(x) self.value+=x;return self.value end;for i=1,SCALE do total+=t:add(i) end".into(),
        "closures" => "local function make(n) return function(x) n+=x;return n end end;local f,g=make(3),make(7);for i=1,SCALE do total+=f(i)+g(2) end".into(),
        "recursion" => "local function fib(n) if n<2 then return n end;return fib(n-1)+fib(n-2) end;for i=1,SCALE do total+=fib(7) end".into(),
        "varargs" => "local function f(...) local t=table.pack(...);local n=0;for i=1,t.n do n+=t[i] or 0 end;return n,nil,t.n end;for i=1,SCALE do local n,_,count=f(i,nil,3,4);total+=n+count end".into(),
        "generic_iteration" => "local t={};for i=1,SCALE do t[i]=i*3 end;for k,v in t do total+=k+v end".into(),
        "custom_iteration" => "local t=setmetatable({},{__iter=function() local i=0;return function() i+=1;if i<=SCALE then return i,i*3 end end end});for k,v in t do total+=k+v end".into(),
        "metatables" => "local t=setmetatable({value=7},{__index=function(self,key) if key=='missing' then return self.value*2 end;return 0 end});for i=1,SCALE do total+=t.missing end".into(),
        "interpolation" => "for i=1,SCALE do local value=`item={i};round={round}`;total+=#value end".into(),
        "binary_strings" => r#"for i=1,SCALE do local value='\000\255\128\n\r\t\u{1F600}';total+=#value+string.byte(value,2) end"#.into(),
        "repeated_strings" => generated(scale, |body, _| { body.push_str("total+=#'shared-literal';\n"); }),
        "unique_strings" => generated(scale, |body, i| { writeln!(body, "total+=#'unique-literal-{i:06}';").unwrap(); }),
        "large_block" => generated(scale, |body, i| { writeln!(body, "total=(total+{}*round)%100003;", i+17).unwrap(); }),
        "many_functions" => generated(scale, |body, i| { writeln!(body, "do local function f{i}(x)return x*3+{i} end;total+=f{i}(round);end;").unwrap(); }),
        "shadowing" => "local value=5;for i=1,SCALE do local value=value+i;do local value=value*3;total+=value end;total+=value end;total+=value".into(),
        "typed" => "type Entry={value:number};local function f(x:number):number local t:Entry={value=x*3};return t.value end;for i=1,SCALE do total+=f(i) end".into(),
        "comments" => generated(scale, |body, i| { writeln!(body, "-- comment {i}: runtime passes must ignore this text\ntotal+={i};").unwrap(); }),
        "fallback" => "local function f(x) local environment=getfenv(0);return x+(if environment~=nil then 7 else 0) end;for i=1,SCALE do total+=f(i) end".into(),
        _ => unreachable!("workloads are selected from WORKLOADS"),
    };
    body = body.replace("SCALE", &scale.to_string());
    format!("--!strict\nreturn function(iterations)\nlocal total=0\nfor round=1,iterations do\n{body}\nend\nreturn total\nend\n")
}

fn generated(scale: usize, mut statement: impl FnMut(&mut String, usize)) -> String {
    let mut body = String::new();
    for i in 0..scale {
        statement(&mut body, i);
    }
    body
}
