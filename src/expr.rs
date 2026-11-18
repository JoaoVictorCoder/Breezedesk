//! Tiny expression language used in `{...}` templates, `value`, `visible`,
//! `rotate` and dynamic colors.
//!
//! ```text
//! cpu * 2                      arithmetic: + - * / %
//! cpu > 80 ? "HOT" : "ok"      ternary, comparisons, && || ! (and/or/not)
//! if(battery < 20, "#F00", accent)
//! round(ram_used, 1)           functions
//! weather.current.temperature_2m   dotted paths (web data, Lua tables)
//! ```
//! Numbers remember how many decimals they should be shown with: `ram_used`
//! shows 1 decimal, `cpu` none, and `round(x, n)` changes it.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// Number + preferred number of decimals when displayed.
    Num(f64, u8),
    Str(String),
    Null,
}

impl Value {
    pub fn int(v: f64) -> Value {
        Value::Num(v, 0)
    }
    pub fn bool(b: bool) -> Value {
        Value::Num(if b { 1.0 } else { 0.0 }, 0)
    }
    pub fn num(&self) -> f64 {
        match self {
            Value::Num(v, _) => *v,
            Value::Str(s) => s.trim().parse().unwrap_or(0.0),
            Value::Null => 0.0,
        }
    }
    fn dec(&self) -> u8 {
        match self {
            Value::Num(_, d) => *d,
            _ => 0,
        }
    }
    pub fn truthy(&self) -> bool {
        match self {
            Value::Num(v, _) => *v != 0.0 && !v.is_nan(),
            Value::Str(s) => !s.is_empty(),
            Value::Null => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Num(v, d) if v.is_finite() => write!(f, "{:.*}", *d as usize, v),
            Value::Num(..) => write!(f, "-"),
            Value::Str(s) => f.write_str(s),
            Value::Null => Ok(()),
        }
    }
}

/// Where variables and host functions come from.
pub trait Env {
    fn var(&self, name: &str) -> Option<Value>;
    /// Host functions (time, disk, lua, ...). `None` = unknown function.
    fn call(&self, name: &str, args: &[Value]) -> Option<Value>;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Add, Sub, Mul, Div, Mod, Eq, Ne, Lt, Le, Gt, Ge, And, Or, Neg, Not,
}

#[derive(Clone, Debug)]
pub enum Ast {
    Lit(Value),
    Var(String),
    Unary(Op, Box<Ast>),
    Binary(Op, Box<Ast>, Box<Ast>),
    Ternary(Box<Ast>, Box<Ast>, Box<Ast>),
    Call(String, Vec<Ast>),
}

impl Ast {
    /// Visits every variable and function name (used to know what to measure).
    pub fn names(&self, f: &mut dyn FnMut(&str)) {
        match self {
            Ast::Lit(_) => {}
            Ast::Var(n) => f(n),
            Ast::Unary(_, a) => a.names(f),
            Ast::Binary(_, a, b) => {
                a.names(f);
                b.names(f);
            }
            Ast::Ternary(a, b, c) => {
                a.names(f);
                b.names(f);
                c.names(f);
            }
            Ast::Call(n, args) => {
                f(n);
                args.iter().for_each(|a| a.names(f));
            }
        }
    }

    pub fn eval(&self, env: &dyn Env) -> Value {
        match self {
            Ast::Lit(v) => v.clone(),
            Ast::Var(n) => match n.as_str() {
                "true" => Value::bool(true),
                "false" => Value::bool(false),
                "null" => Value::Null,
                _ => env.var(n).unwrap_or(Value::Null),
            },
            Ast::Unary(Op::Neg, a) => {
                let v = a.eval(env);
                Value::Num(-v.num(), v.dec())
            }
            Ast::Unary(_, a) => Value::bool(!a.eval(env).truthy()),
            Ast::Binary(Op::And, a, b) => {
                let l = a.eval(env);
                if l.truthy() { b.eval(env) } else { l }
            }
            Ast::Binary(Op::Or, a, b) => {
                let l = a.eval(env);
                if l.truthy() { l } else { b.eval(env) }
            }
            Ast::Binary(op, a, b) => binary(*op, a.eval(env), b.eval(env)),
            Ast::Ternary(c, a, b) => {
                if c.eval(env).truthy() { a.eval(env) } else { b.eval(env) }
            }
            Ast::Call(name, args) if name == "if" && args.len() >= 2 => {
                if args[0].eval(env).truthy() {
                    args[1].eval(env)
                } else {
                    args.get(2).map_or(Value::Null, |a| a.eval(env))
                }
            }
            Ast::Call(name, args) => {
                let vals: Vec<Value> = args.iter().map(|a| a.eval(env)).collect();
                builtin(name, &vals).or_else(|| env.call(name, &vals)).unwrap_or(Value::Null)
            }
        }
    }
}

fn binary(op: Op, l: Value, r: Value) -> Value {
    let is_str = matches!(l, Value::Str(_)) || matches!(r, Value::Str(_));
    let dec = l.dec().max(r.dec());
    match op {
        Op::Add if is_str => Value::Str(format!("{l}{r}")),
        Op::Add => Value::Num(l.num() + r.num(), dec),
        Op::Sub => Value::Num(l.num() - r.num(), dec),
        Op::Mul => Value::Num(l.num() * r.num(), dec),
        Op::Div => Value::Num(l.num() / r.num(), dec),
        Op::Mod => Value::Num(l.num() % r.num(), dec),
        Op::Eq | Op::Ne => {
            let eq = if is_str { l.to_string() == r.to_string() } else { l.num() == r.num() };
            Value::bool(eq == (op == Op::Eq))
        }
        Op::Lt | Op::Le | Op::Gt | Op::Ge => {
            let ord = if is_str && !matches!((&l, &r), (Value::Num(..), _) | (_, Value::Num(..))) {
                l.to_string().partial_cmp(&r.to_string())
            } else {
                l.num().partial_cmp(&r.num())
            };
            use std::cmp::Ordering::*;
            Value::bool(matches!(
                (op, ord),
                (Op::Lt, Some(Less)) | (Op::Le, Some(Less | Equal)) | (Op::Gt, Some(Greater)) | (Op::Ge, Some(Greater | Equal))
            ))
        }
        _ => Value::Null,
    }
}

/// Pure functions available everywhere.
fn builtin(name: &str, a: &[Value]) -> Option<Value> {
    let n = |i: usize| a.get(i).map_or(0.0, |v| v.num());
    let s = |i: usize| a.get(i).map_or(String::new(), |v| v.to_string());
    let d0 = a.first().map_or(0, |v| v.dec());
    Some(match name {
        "round" => {
            let d = n(1).clamp(0.0, 10.0) as u8;
            let p = 10f64.powi(d as i32);
            Value::Num((n(0) * p).round() / p, d)
        }
        "floor" => Value::int(n(0).floor()),
        "ceil" => Value::int(n(0).ceil()),
        "abs" => Value::Num(n(0).abs(), d0),
        "min" => a.iter().min_by(|x, y| x.num().total_cmp(&y.num())).cloned().unwrap_or(Value::Null),
        "max" => a.iter().max_by(|x, y| x.num().total_cmp(&y.num())).cloned().unwrap_or(Value::Null),
        "clamp" => Value::Num(n(0).clamp(n(1), n(2).max(n(1))), d0),
        "num" => Value::Num(n(0), d0),
        "is_null" => Value::bool(matches!(a.first(), Some(Value::Null) | None)),
        "default" => match a.first() {
            Some(Value::Null) | None => a.get(1).cloned().unwrap_or(Value::Null),
            Some(v) => v.clone(),
        },
        "str" => Value::Str(s(0)),
        "upper" => Value::Str(s(0).to_uppercase()),
        "lower" => Value::Str(s(0).to_lowercase()),
        "len" => Value::int(s(0).chars().count() as f64),
        "trunc" => {
            let (text, max) = (s(0), n(1).max(1.0) as usize);
            if text.chars().count() > max {
                Value::Str(text.chars().take(max.saturating_sub(1)).collect::<String>() + "…")
            } else {
                Value::Str(text)
            }
        }
        "pad" => Value::Str(format!("{:0>width$}", s(0), width = n(1).max(0.0) as usize)),
        "fmt_bytes" => Value::Str(fmt_bytes(n(0))),
        "fmt_time" => {
            let t = n(0).max(0.0) as u64;
            if t >= 3600 {
                Value::Str(format!("{}:{:02}:{:02}", t / 3600, (t % 3600) / 60, t % 60))
            } else {
                Value::Str(format!("{}:{:02}", t / 60, t % 60))
            }
        }
        _ => return None,
    })
}

pub fn fmt_bytes(b: f64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b.max(0.0);
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 || v >= 100.0 { format!("{v:.0} {}", U[i]) } else { format!("{v:.1} {}", U[i]) }
}

// ------------------------------------------------------------------ parser

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64, u8),
    Str(String),
    Ident(String),
    Sym(&'static str),
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    const SYMS: [&str; 20] = [
        "==", "!=", "<=", ">=", "&&", "||", "+", "-", "*", "/", "%", "<", ">", "!", "?", ":", "(", ")", ",", "=",
    ];
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let start = i;
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                i += 1;
            }
            let s: String = c[start..i].iter().collect();
            let dec = s.split_once('.').map_or(0, |(_, f)| f.len().min(10) as u8);
            out.push(Tok::Num(s.parse().map_err(|_| format!("bad number {s:?}"))?, dec));
        } else if ch == '"' || ch == '\'' {
            let mut s = String::new();
            i += 1;
            while i < c.len() && c[i] != ch {
                if c[i] == '\\' && i + 1 < c.len() {
                    i += 1;
                }
                s.push(c[i]);
                i += 1;
            }
            if i >= c.len() {
                return Err("unterminated string".into());
            }
            i += 1;
            out.push(Tok::Str(s));
        } else if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_' || c[i] == '.') {
                i += 1;
            }
            out.push(Tok::Ident(c[start..i].iter().collect()));
        } else {
            let two: String = c[i..(i + 2).min(c.len())].iter().collect();
            let sym = SYMS
                .iter()
                .find(|s| two.starts_with(**s) && s.len() == 2)
                .or_else(|| SYMS.iter().find(|s| s.len() == 1 && s.starts_with(ch)))
                .ok_or_else(|| format!("unexpected character {ch:?}"))?;
            i += sym.len();
            out.push(Tok::Sym(sym));
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn eat(&mut self, sym: &str) -> bool {
        let hit = match self.peek() {
            Some(Tok::Sym(s)) => *s == sym,
            Some(Tok::Ident(s)) => s == sym, // and / or / not
            _ => false,
        };
        if hit {
            self.pos += 1;
        }
        hit
    }
    fn expect(&mut self, sym: &str) -> Result<(), String> {
        if self.eat(sym) { Ok(()) } else { Err(format!("expected '{sym}'")) }
    }

    fn ternary(&mut self) -> Result<Ast, String> {
        let c = self.or()?;
        if self.eat("?") {
            let a = self.ternary()?;
            self.expect(":")?;
            let b = self.ternary()?;
            return Ok(Ast::Ternary(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }
    fn or(&mut self) -> Result<Ast, String> {
        let mut l = self.and()?;
        while self.eat("||") || self.eat("or") {
            l = Ast::Binary(Op::Or, Box::new(l), Box::new(self.and()?));
        }
        Ok(l)
    }
    fn and(&mut self) -> Result<Ast, String> {
        let mut l = self.cmp()?;
        while self.eat("&&") || self.eat("and") {
            l = Ast::Binary(Op::And, Box::new(l), Box::new(self.cmp()?));
        }
        Ok(l)
    }
    fn cmp(&mut self) -> Result<Ast, String> {
        let mut l = self.add()?;
        loop {
            let op = if self.eat("==") || self.eat("=") {
                Op::Eq
            } else if self.eat("!=") {
                Op::Ne
            } else if self.eat("<=") {
                Op::Le
            } else if self.eat(">=") {
                Op::Ge
            } else if self.eat("<") {
                Op::Lt
            } else if self.eat(">") {
                Op::Gt
            } else {
                return Ok(l);
            };
            l = Ast::Binary(op, Box::new(l), Box::new(self.add()?));
        }
    }
    fn add(&mut self) -> Result<Ast, String> {
        let mut l = self.mul()?;
        loop {
            let op = if self.eat("+") { Op::Add } else if self.eat("-") { Op::Sub } else { return Ok(l) };
            l = Ast::Binary(op, Box::new(l), Box::new(self.mul()?));
        }
    }
    fn mul(&mut self) -> Result<Ast, String> {
        let mut l = self.unary()?;
        loop {
            let op = if self.eat("*") {
                Op::Mul
            } else if self.eat("/") {
                Op::Div
            } else if self.eat("%") {
                Op::Mod
            } else {
                return Ok(l);
            };
            l = Ast::Binary(op, Box::new(l), Box::new(self.unary()?));
        }
    }
    fn unary(&mut self) -> Result<Ast, String> {
        if self.eat("-") {
            return Ok(Ast::Unary(Op::Neg, Box::new(self.unary()?)));
        }
        if self.eat("!") || self.eat("not") {
            return Ok(Ast::Unary(Op::Not, Box::new(self.unary()?)));
        }
        self.primary()
    }
    fn primary(&mut self) -> Result<Ast, String> {
        let tok = self.peek().cloned().ok_or("unexpected end of expression")?;
        self.pos += 1;
        match tok {
            Tok::Num(v, d) => Ok(Ast::Lit(Value::Num(v, d))),
            Tok::Str(s) => Ok(Ast::Lit(Value::Str(s))),
            Tok::Sym("(") => {
                let e = self.ternary()?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::Ident(name) => {
                if self.eat("(") {
                    let mut args = Vec::new();
                    if !self.eat(")") {
                        loop {
                            args.push(self.ternary()?);
                            if self.eat(")") {
                                break;
                            }
                            self.expect(",")?;
                        }
                    }
                    Ok(Ast::Call(name, args))
                } else {
                    Ok(Ast::Var(name))
                }
            }
            Tok::Sym(s) => Err(format!("unexpected '{s}'")),
        }
    }
}

pub fn parse(src: &str) -> Result<Ast, String> {
    let mut p = Parser { toks: tokenize(src)?, pos: 0 };
    let ast = p.ternary().map_err(|e| format!("{e} in expression {src:?}"))?;
    if p.pos != p.toks.len() {
        return Err(format!("unexpected text after expression {src:?}"));
    }
    Ok(ast)
}

// --------------------------------------------------------------- templates

/// Text with `{expression}` parts. `{{` and `}}` are literal braces.
#[derive(Clone, Debug, Default)]
pub struct Template {
    parts: Vec<Part>,
}

#[derive(Clone, Debug)]
enum Part {
    Lit(String),
    Expr(Ast),
}

/// Old `{time:%H:%M}` / `{disk:C:}` syntax, kept for convenience.
const COLON_FUNCS: [&str; 5] = ["time", "disk", "disk_used", "disk_free", "disk_total"];

impl Template {
    pub fn parse(src: &str) -> Result<Template, String> {
        let mut parts = Vec::new();
        let mut lit = String::new();
        let c: Vec<char> = src.chars().collect();
        let mut i = 0;
        while i < c.len() {
            match c[i] {
                '{' if c.get(i + 1) == Some(&'{') => {
                    lit.push('{');
                    i += 2;
                }
                '}' if c.get(i + 1) == Some(&'}') => {
                    lit.push('}');
                    i += 2;
                }
                '{' => {
                    // find the closing brace, skipping quoted strings
                    let mut j = i + 1;
                    let mut quote = None;
                    while j < c.len() {
                        match (quote, c[j]) {
                            (None, '}') => break,
                            (None, q @ ('"' | '\'')) => quote = Some(q),
                            (Some(q), ch) if ch == q => quote = None,
                            _ => {}
                        }
                        j += 1;
                    }
                    if j >= c.len() {
                        lit.extend(&c[i..]);
                        break;
                    }
                    let inner: String = c[i + 1..j].iter().collect();
                    if !lit.is_empty() {
                        parts.push(Part::Lit(std::mem::take(&mut lit)));
                    }
                    parts.push(Part::Expr(parse_inner(&inner)?));
                    i = j + 1;
                }
                ch => {
                    lit.push(ch);
                    i += 1;
                }
            }
        }
        if !lit.is_empty() {
            parts.push(Part::Lit(lit));
        }
        Ok(Template { parts })
    }

    #[cfg(test)]
    pub fn is_static(&self) -> bool {
        self.parts.iter().all(|p| matches!(p, Part::Lit(_)))
    }

    pub fn render(&self, env: &dyn Env) -> String {
        match self.parts.as_slice() {
            [] => String::new(),
            [Part::Lit(s)] => s.clone(),
            parts => {
                let mut out = String::new();
                for p in parts {
                    match p {
                        Part::Lit(s) => out.push_str(s),
                        Part::Expr(e) => out.push_str(&e.eval(env).to_string()),
                    }
                }
                out
            }
        }
    }

    /// A template made of a single `{expr}` keeps the value's type.
    pub fn value(&self, env: &dyn Env) -> Value {
        match self.parts.as_slice() {
            [Part::Expr(e)] => e.eval(env),
            _ => Value::Str(self.render(env)),
        }
    }

    pub fn names(&self, f: &mut dyn FnMut(&str)) {
        for p in &self.parts {
            if let Part::Expr(e) = p {
                e.names(f);
            }
        }
    }
}

fn parse_inner(inner: &str) -> Result<Ast, String> {
    if let Some((name, arg)) = inner.split_once(':') {
        let name = name.trim();
        if COLON_FUNCS.contains(&name) {
            return Ok(Ast::Call(name.to_string(), vec![Ast::Lit(Value::Str(arg.to_string()))]));
        }
    }
    parse(inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct E(HashMap<&'static str, Value>);
    impl Env for E {
        fn var(&self, n: &str) -> Option<Value> {
            self.0.get(n).cloned()
        }
        fn call(&self, n: &str, a: &[Value]) -> Option<Value> {
            (n == "time").then(|| Value::Str(format!("T[{}]", a[0])))
        }
    }

    fn env() -> E {
        E(HashMap::from([
            ("cpu", Value::Num(42.4, 0)),
            ("ram_used", Value::Num(7.84, 1)),
            ("name", Value::Str("Breezedesk".into())),
            ("w.current.temp", Value::Num(21.5, 1)),
        ]))
    }

    fn ev(s: &str) -> String {
        parse(s).unwrap().eval(&env()).to_string()
    }

    #[test]
    fn arithmetic_and_decimals() {
        assert_eq!(ev("cpu"), "42");
        assert_eq!(ev("ram_used"), "7.8");
        assert_eq!(ev("cpu * 2"), "85");
        assert_eq!(ev("round(cpu, 2)"), "42.40");
        assert_eq!(ev("(1 + 2) * 3 - 4 / 2"), "7");
        assert_eq!(ev("-cpu"), "-42");
        assert_eq!(ev("10 % 3"), "1");
    }

    #[test]
    fn logic_and_strings() {
        assert_eq!(ev("cpu > 40 ? 'hot' : 'ok'"), "hot");
        assert_eq!(ev("if(cpu < 10, 'idle', 'busy')"), "busy");
        assert_eq!(ev("cpu > 40 and not (cpu > 50)"), "1");
        assert_eq!(ev("name + ' ' + 1"), "Breezedesk 1");
        assert_eq!(ev("name == 'Breezedesk'"), "1");
        assert_eq!(ev("upper(name)"), "BREEZEDESK");
        assert_eq!(ev("trunc('abcdefgh', 4)"), "abc…");
        assert_eq!(ev("w.current.temp"), "21.5");
        assert_eq!(ev("missing"), "");
        assert_eq!(ev("fmt_time(125)"), "2:05");
        assert_eq!(ev("max(1, cpu, 3)"), "42");
        assert_eq!(ev("default(missing, 'n/a')"), "n/a");
        assert_eq!(ev("default(cpu, 'n/a')"), "42");
    }

    #[test]
    fn templates() {
        let t = Template::parse("CPU {cpu}% {{x}} {time:%H:%M} {cpu > 40 ? \"}\" : 'n'}").unwrap();
        assert_eq!(t.render(&env()), "CPU 42% {x} T[%H:%M] }");
        assert!(Template::parse("plain").unwrap().is_static());
        assert!(Template::parse("{cpu +}").is_err());
        assert_eq!(Template::parse("open {").unwrap().render(&env()), "open {");
    }
}
