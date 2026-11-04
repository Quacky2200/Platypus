// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Values and expressions for the debugger (`debug.rs`): what a spilled local is, how it is
//! shown and expanded, and a small evaluator for watch expressions, breakpoint conditions and
//! log messages. Everything reads the game's memory directly, so it runs only while the game
//! is stopped (or on its own thread, in the hook).
//!
//! Expressions: identifiers (the stopped function's named locals, `this` included), numbers,
//! strings, `true` `false` `null` `undefined`, `.field`, `[index]`, `.length` of strings and
//! arrays, `! -`, `* / %`, `+ -`, `< > <= >=`, `== != === !==`, `&&` `||` and parentheses.
//! No calls, assignments or function values.

use std::sync::{Arc, OnceLock};

/// How to show and read a value, from its static type.
#[derive(Clone, Debug)]
pub enum ShowTy {
    Num,
    Bool,
    /// A member of a string-literal union: an index into the module's tag texts.
    Lit(Arc<Vec<String>>),
    Str,
    /// A class instance or shape: the module's struct table and its id.
    Obj(Arc<Structs>, usize),
    /// An array of this element type (8-byte slots).
    Arr(Box<ShowTy>),
    /// A captured variable: a cell holding one value of the inner type.
    Cell(Box<ShowTy>),
    Nullable(Box<ShowTy>),
    Other(String),
}

/// The module's structs, filled once every field type is known (a field can name its own struct).
#[derive(Default)]
pub struct Structs(pub OnceLock<Vec<StructInfo>>);

impl std::fmt::Debug for Structs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Structs({})", self.0.get().map_or(0, Vec::len))
    }
}

#[derive(Clone, Debug)]
pub struct StructInfo {
    pub name: String,
    /// (name, byte offset in the object, type)
    pub fields: Vec<(String, usize, ShowTy)>,
}

/// A value read from the game.
#[derive(Clone, Debug)]
pub enum Val {
    Num(f64),
    Bool(bool),
    Str(String),
    Null,
    Undefined,
    /// A reference (object, array) or a value shown only as its type: the raw word and its type.
    Mem(u64, ShowTy),
}

/// The named locals of a stopped function: (name, raw word, type), in declaration order.
#[derive(Clone, Debug, Default)]
pub struct Scope {
    pub locals: Vec<(String, u64, ShowTy)>,
}

const NULL_NUM_BITS: u64 = crate::ir::NULL_NUM_BITS;

/// A pointer worth reading: aligned, not null, in user space.
fn plausible(p: u64) -> bool {
    p != 0 && p.is_multiple_of(8) && p < 0x0000_8000_0000_0000
}

/// The value of a raw 64-bit word of this type.
pub fn from_raw(raw: u64, ty: &ShowTy) -> Val {
    match ty {
        ShowTy::Num => Val::Num(f64::from_bits(raw)),
        ShowTy::Bool => Val::Bool(raw & 1 != 0),
        ShowTy::Lit(tags) => match tags.get(raw as u32 as usize) {
            Some(t) if raw as u32 != 0 => Val::Str(t.clone()),
            _ => Val::Undefined,
        },
        ShowTy::Str => {
            if raw == 0 {
                Val::Null
            } else {
                Val::Str(read_string(raw))
            }
        }
        ShowTy::Obj(..) | ShowTy::Arr(_) => {
            if raw == 0 {
                Val::Null
            } else {
                Val::Mem(raw, ty.clone())
            }
        }
        ShowTy::Cell(inner) => {
            if !plausible(raw) {
                Val::Null
            } else {
                // SAFETY: a cell local holds a live cell: a header word, then the value.
                from_raw(unsafe { *((raw + 8) as *const u64) }, inner)
            }
        }
        ShowTy::Nullable(inner) => {
            let null = match **inner {
                ShowTy::Num => raw == NULL_NUM_BITS,
                ShowTy::Lit(_) => raw as u32 == 0,
                ShowTy::Bool => false,
                _ => raw == 0,
            };
            if null {
                Val::Null
            } else {
                from_raw(raw, inner)
            }
        }
        ShowTy::Other(_) => Val::Mem(raw, ty.clone()),
    }
}

fn read_string(p: u64) -> String {
    if !plausible(p) {
        return format!("<bad pointer {p:#x}>");
    }
    // SAFETY: a string slot holds a live string or zero; the pointer was checked above and the
    // length is checked before the bytes are read.
    let s = unsafe { platypus_ts_rt::Str::from_raw(p as *const platypus_ts_rt::Str) };
    if !(0..1 << 24).contains(&s.byte_len) {
        return format!("<string @{p:#x}>");
    }
    String::from_utf8_lossy(s.as_bytes()).into_owned()
}

/// An array's length and the address of its slots.
fn array_parts(p: u64) -> Option<(usize, u64)> {
    if !plausible(p) {
        return None;
    }
    // SAFETY: a live array: header word, length, capacity, data pointer.
    let (len, data) = unsafe { (*((p + 8) as *const i64), *((p + 24) as *const u64)) };
    (0..1 << 24).contains(&len).then_some((len as usize, data))
}

fn number(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v.fract() == 0.0 && v.abs() < 9e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// The type name and text of a value, as a debugger lists it.
pub fn describe(v: &Val) -> (String, String) {
    match v {
        Val::Num(n) => ("number".into(), number(*n)),
        Val::Bool(b) => ("boolean".into(), b.to_string()),
        Val::Str(s) => {
            let cut: String = s.chars().take(200).collect();
            let more = if cut.len() < s.len() { "…" } else { "" };
            ("string".into(), format!("{cut:?}{more}"))
        }
        Val::Null => ("null".into(), "null".into()),
        Val::Undefined => ("undefined".into(), "undefined".into()),
        Val::Mem(_, ShowTy::Obj(structs, id)) => {
            let name = structs
                .0
                .get()
                .and_then(|s| s.get(*id))
                .map_or("object", |s| s.name.as_str());
            (name.to_string(), name.to_string())
        }
        Val::Mem(raw, ShowTy::Arr(_)) => {
            let len = array_parts(*raw).map_or(0, |(l, _)| l);
            ("array".into(), format!("Array({len})"))
        }
        Val::Mem(raw, ShowTy::Other(t)) => (t.clone(), format!("{raw:#x}")),
        Val::Mem(raw, _) => ("value".into(), format!("{raw:#x}")),
    }
}

/// The parts of a value a debugger can open: an object's fields, an array's elements (the first
/// 100). Everything else has none.
pub fn children(v: &Val) -> Vec<(String, Val)> {
    match v {
        Val::Mem(raw, ShowTy::Obj(structs, id)) if plausible(*raw) => {
            let Some(info) = structs.0.get().and_then(|s| s.get(*id)) else {
                return Vec::new();
            };
            info.fields
                .iter()
                .map(|(name, off, ty)| {
                    // SAFETY: a live object of this struct: every field is one 8-byte slot.
                    let word = unsafe { *((*raw + *off as u64) as *const u64) };
                    (name.clone(), from_raw(word, ty))
                })
                .collect()
        }
        Val::Mem(raw, ShowTy::Arr(elem)) => {
            let Some((len, data)) = array_parts(*raw) else {
                return Vec::new();
            };
            if len > 0 && !plausible(data) {
                return Vec::new();
            }
            (0..len.min(100))
                .map(|i| {
                    // SAFETY: element `i` of a live array's slots.
                    let word = unsafe { *((data + 8 * i as u64) as *const u64) };
                    (format!("[{i}]"), from_raw(word, elem))
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

// ---- expressions ----

#[derive(Clone, Debug)]
pub enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Null,
    Undefined,
    Ident(String),
    Member(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Punct(&'static str),
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    const PUNCT: [&str; 22] = [
        "===", "!==", "==", "!=", "<=", ">=", "&&", "||", "+", "-", "*", "/", "%", "<", ">", "!",
        "(", ")", "[", "]", ".", ",",
    ];
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            out.push(Tok::Num(
                text.parse().map_err(|_| format!("bad number {text}"))?,
            ));
        } else if c == '"' || c == '\'' {
            let mut s = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err("unterminated string".into()),
                    Some('\\') => {
                        match chars.get(i + 1) {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(c) => s.push(*c),
                            None => return Err("unterminated string".into()),
                        }
                        i += 2;
                    }
                    Some(q) if *q == c => {
                        i += 1;
                        break;
                    }
                    Some(c) => {
                        s.push(*c);
                        i += 1;
                    }
                }
            }
            out.push(Tok::Str(s));
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$')
            {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
        } else {
            let rest: String = chars[i..].iter().take(3).collect();
            let Some(p) = PUNCT.iter().find(|p| rest.starts_with(**p)) else {
                return Err(format!("unexpected {c:?}"));
            };
            out.push(Tok::Punct(p));
            i += p.len();
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn eat(&mut self, p: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Punct(q)) if *q == p) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn binary(
        &mut self,
        ops: &[(&str, Op)],
        next: fn(&mut Parser) -> Result<Expr, String>,
    ) -> Result<Expr, String> {
        let mut left = next(self)?;
        'outer: loop {
            for (p, op) in ops {
                if self.eat(p) {
                    let right = next(self)?;
                    left = Expr::Bin(*op, Box::new(left), Box::new(right));
                    continue 'outer;
                }
            }
            return Ok(left);
        }
    }

    fn or(&mut self) -> Result<Expr, String> {
        self.binary(&[("||", Op::Or)], Parser::and)
    }
    fn and(&mut self) -> Result<Expr, String> {
        self.binary(&[("&&", Op::And)], Parser::equality)
    }
    fn equality(&mut self) -> Result<Expr, String> {
        self.binary(
            &[
                ("===", Op::Eq),
                ("!==", Op::Ne),
                ("==", Op::Eq),
                ("!=", Op::Ne),
            ],
            Parser::relational,
        )
    }
    fn relational(&mut self) -> Result<Expr, String> {
        self.binary(
            &[("<=", Op::Le), (">=", Op::Ge), ("<", Op::Lt), (">", Op::Gt)],
            Parser::additive,
        )
    }
    fn additive(&mut self) -> Result<Expr, String> {
        self.binary(&[("+", Op::Add), ("-", Op::Sub)], Parser::multiplicative)
    }
    fn multiplicative(&mut self) -> Result<Expr, String> {
        self.binary(
            &[("*", Op::Mul), ("/", Op::Div), ("%", Op::Rem)],
            Parser::unary,
        )
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat("!") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        if self.eat("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        loop {
            if self.eat(".") {
                match self.peek().cloned() {
                    Some(Tok::Ident(name)) => {
                        self.at += 1;
                        e = Expr::Member(Box::new(e), name);
                    }
                    _ => return Err("a name after '.'".into()),
                }
            } else if self.eat("[") {
                let i = self.or()?;
                if !self.eat("]") {
                    return Err("expected ']'".into());
                }
                e = Expr::Index(Box::new(e), Box::new(i));
            } else {
                return Ok(e);
            }
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let Some(t) = self.peek().cloned() else {
            return Err("the expression ends too soon".into());
        };
        self.at += 1;
        match t {
            Tok::Num(n) => Ok(Expr::Num(n)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::Ident(n) => Ok(match n.as_str() {
                "true" => Expr::Bool(true),
                "false" => Expr::Bool(false),
                "null" => Expr::Null,
                "undefined" => Expr::Undefined,
                _ => Expr::Ident(n),
            }),
            Tok::Punct("(") => {
                let e = self.or()?;
                if !self.eat(")") {
                    return Err("expected ')'".into());
                }
                Ok(e)
            }
            Tok::Punct(p) => Err(format!("unexpected '{p}'")),
        }
    }
}

/// Parses an expression.
pub fn parse(src: &str) -> Result<Expr, String> {
    let mut p = Parser {
        toks: lex(src)?,
        at: 0,
    };
    let e = p.or()?;
    if p.at < p.toks.len() {
        return Err("something after the end of the expression".into());
    }
    Ok(e)
}

pub fn truthy(v: &Val) -> bool {
    match v {
        Val::Num(n) => *n != 0.0 && !n.is_nan(),
        Val::Bool(b) => *b,
        Val::Str(s) => !s.is_empty(),
        Val::Null | Val::Undefined => false,
        Val::Mem(..) => true,
    }
}

fn as_text(v: &Val) -> String {
    match v {
        Val::Str(s) => s.clone(),
        other => describe(other).1,
    }
}

fn same(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => x == y,
        (Val::Bool(x), Val::Bool(y)) => x == y,
        (Val::Str(x), Val::Str(y)) => x == y,
        (Val::Null | Val::Undefined, Val::Null | Val::Undefined) => true,
        (Val::Mem(x, _), Val::Mem(y, _)) => x == y,
        _ => false,
    }
}

/// Evaluates an expression in the stopped function's scope.
pub fn eval(e: &Expr, scope: &Scope) -> Result<Val, String> {
    Ok(match e {
        Expr::Num(n) => Val::Num(*n),
        Expr::Str(s) => Val::Str(s.clone()),
        Expr::Bool(b) => Val::Bool(*b),
        Expr::Null => Val::Null,
        Expr::Undefined => Val::Undefined,
        Expr::Ident(name) => {
            let (_, raw, ty) = scope
                .locals
                .iter()
                .rev()
                .find(|(n, _, _)| n == name)
                .ok_or_else(|| format!("{name} is not defined"))?;
            from_raw(*raw, ty)
        }
        Expr::Member(obj, name) => {
            let v = eval(obj, scope)?;
            match (&v, name.as_str()) {
                (Val::Str(s), "length") => Val::Num(s.encode_utf16().count() as f64),
                (Val::Mem(raw, ShowTy::Arr(_)), "length") => {
                    Val::Num(array_parts(*raw).map_or(0, |(l, _)| l) as f64)
                }
                (Val::Null | Val::Undefined, _) => {
                    return Err(format!("cannot read '{name}' of null"));
                }
                _ => children(&v)
                    .into_iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| v)
                    .ok_or_else(|| format!("no field '{name}'"))?,
            }
        }
        Expr::Index(obj, idx) => {
            let v = eval(obj, scope)?;
            let Val::Num(i) = eval(idx, scope)? else {
                return Err("an index must be a number".into());
            };
            if i < 0.0 || i.fract() != 0.0 {
                return Ok(Val::Undefined);
            }
            let i = i as usize;
            match &v {
                Val::Str(s) => s
                    .chars()
                    .nth(i)
                    .map_or(Val::Undefined, |c| Val::Str(c.to_string())),
                Val::Mem(raw, ShowTy::Arr(elem)) => {
                    let Some((len, data)) = array_parts(*raw) else {
                        return Err("bad array".into());
                    };
                    if i >= len {
                        Val::Undefined
                    } else {
                        // SAFETY: element `i` of a live array's slots, in range.
                        from_raw(unsafe { *((data + 8 * i as u64) as *const u64) }, elem)
                    }
                }
                _ => return Err("only arrays and strings have an index".into()),
            }
        }
        Expr::Not(x) => Val::Bool(!truthy(&eval(x, scope)?)),
        Expr::Neg(x) => match eval(x, scope)? {
            Val::Num(n) => Val::Num(-n),
            _ => return Err("only a number can be negated".into()),
        },
        Expr::Bin(Op::And, a, b) => Val::Bool(truthy(&eval(a, scope)?) && truthy(&eval(b, scope)?)),
        Expr::Bin(Op::Or, a, b) => Val::Bool(truthy(&eval(a, scope)?) || truthy(&eval(b, scope)?)),
        Expr::Bin(op, a, b) => {
            let (x, y) = (eval(a, scope)?, eval(b, scope)?);
            match (op, &x, &y) {
                (Op::Eq, ..) => Val::Bool(same(&x, &y)),
                (Op::Ne, ..) => Val::Bool(!same(&x, &y)),
                (Op::Add, Val::Num(p), Val::Num(q)) => Val::Num(p + q),
                (Op::Add, Val::Str(_), _) | (Op::Add, _, Val::Str(_)) => {
                    Val::Str(format!("{}{}", as_text(&x), as_text(&y)))
                }
                (Op::Sub, Val::Num(p), Val::Num(q)) => Val::Num(p - q),
                (Op::Mul, Val::Num(p), Val::Num(q)) => Val::Num(p * q),
                (Op::Div, Val::Num(p), Val::Num(q)) => Val::Num(p / q),
                (Op::Rem, Val::Num(p), Val::Num(q)) => Val::Num(p % q),
                (Op::Lt, Val::Num(p), Val::Num(q)) => Val::Bool(p < q),
                (Op::Gt, Val::Num(p), Val::Num(q)) => Val::Bool(p > q),
                (Op::Le, Val::Num(p), Val::Num(q)) => Val::Bool(p <= q),
                (Op::Ge, Val::Num(p), Val::Num(q)) => Val::Bool(p >= q),
                (Op::Lt, Val::Str(p), Val::Str(q)) => Val::Bool(p < q),
                (Op::Gt, Val::Str(p), Val::Str(q)) => Val::Bool(p > q),
                (Op::Le, Val::Str(p), Val::Str(q)) => Val::Bool(p <= q),
                (Op::Ge, Val::Str(p), Val::Str(q)) => Val::Bool(p >= q),
                _ => {
                    return Err(format!(
                        "cannot apply {op:?} to {} and {}",
                        describe(&x).0,
                        describe(&y).0
                    ));
                }
            }
        }
    })
}

/// Expands `{expression}` in a log message with the expressions' values (an error shows as
/// `<error: ...>`).
pub fn interpolate(msg: &str, scope: &Scope) -> String {
    let mut out = String::new();
    let mut rest = msg;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let text = match parse(&after[..close]).and_then(|e| eval(&e, scope)) {
            Ok(v) => match v {
                Val::Str(s) => s,
                other => describe(&other).1,
            },
            Err(e) => format!("<error: {e}>"),
        };
        out.push_str(&text);
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// A breakpoint's hit condition (`5`, `== 5`, `>= 5`, `> 5`, `< 5`, `<= 5`, `% 5`): does the
/// `n`th hit stop? A bare number means exactly that hit.
#[derive(Clone, Copy, Debug)]
pub struct HitCondition {
    op: &'static str,
    n: u64,
}

impl HitCondition {
    pub fn parse(text: &str) -> Result<HitCondition, String> {
        let t = text.trim();
        let (op, num) = ["==", ">=", "<=", ">", "<", "%", "="]
            .iter()
            .find_map(|o| t.strip_prefix(o).map(|r| (*o, r)))
            .unwrap_or(("==", t));
        let n: u64 = num
            .trim()
            .parse()
            .map_err(|_| format!("hit condition {text:?}: a number after the operator"))?;
        let op = if op == "=" { "==" } else { op };
        // `% 0` would divide by zero.
        if op == "%" && n == 0 {
            return Err("hit condition: % 0".into());
        }
        Ok(HitCondition { op, n })
    }

    pub fn matches(&self, hits: u64) -> bool {
        match self.op {
            "==" => hits == self.n,
            ">=" => hits >= self.n,
            "<=" => hits <= self.n,
            ">" => hits > self.n,
            "<" => hits < self.n,
            _ => hits.is_multiple_of(self.n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Scope {
        Scope {
            locals: vec![
                ("n".into(), 5f64.to_bits(), ShowTy::Num),
                ("flag".into(), 1, ShowTy::Bool),
                ("n".into(), 7f64.to_bits(), ShowTy::Num),
            ],
        }
    }

    fn run(text: &str) -> Result<String, String> {
        let e = parse(text)?;
        eval(&e, &scope()).map(|v| describe(&v).1)
    }

    // M9-TS-3408
    #[test]
    fn expressions_follow_precedence_and_the_innermost_declaration_of_a_name() {
        assert_eq!(run("1 + 2 * 3").unwrap(), "7");
        assert_eq!(run("(1 + 2) * 3").unwrap(), "9");
        assert_eq!(run("10 % 4 - -1").unwrap(), "3");
        // The later `n` shadows the earlier one.
        assert_eq!(run("n").unwrap(), "7");
        assert_eq!(run("n >= 7 && flag").unwrap(), "true");
        assert_eq!(run("!flag || n == 6").unwrap(), "false");
        assert_eq!(run("'a' + 1 + \"b\"").unwrap(), "\"a1b\"");
        assert_eq!(run("'abc'.length").unwrap(), "3");
        assert_eq!(run("'abc'[1]").unwrap(), "\"b\"");
        assert_eq!(run("null == undefined").unwrap(), "true");
        assert_eq!(run("1 === 1 && 2 !== 3").unwrap(), "true");
    }

    // M9-TS-3409
    #[test]
    fn bad_expressions_say_what_is_wrong() {
        assert!(run("").unwrap_err().contains("too soon"));
        assert!(run("1 +").unwrap_err().contains("too soon"));
        assert!(run("(1").unwrap_err().contains("')'"));
        assert!(run("1 2").unwrap_err().contains("after the end"));
        assert!(run("a b").is_err());
        assert!(run("#").unwrap_err().contains("unexpected"));
        assert!(run("'open").unwrap_err().contains("unterminated"));
        assert!(run("missing").unwrap_err().contains("not defined"));
        assert!(run("n.x").is_err());
        assert!(run("flag - 1").unwrap_err().contains("cannot apply"));
        // No calls: a function cannot run in the stopped game.
        assert!(run("n(1)").is_err());
    }

    // M9-TS-3410
    #[test]
    fn hit_conditions_and_log_messages() {
        let h = |t: &str, hits: &[u64]| -> Vec<bool> {
            let c = HitCondition::parse(t).unwrap();
            hits.iter().map(|n| c.matches(*n)).collect()
        };
        assert_eq!(h("3", &[2, 3, 4]), [false, true, false]);
        assert_eq!(h("= 3", &[2, 3, 4]), [false, true, false]);
        assert_eq!(h(">= 3", &[2, 3, 4]), [false, true, true]);
        assert_eq!(h("> 3", &[3, 4]), [false, true]);
        assert_eq!(h("< 3", &[2, 3]), [true, false]);
        assert_eq!(h("% 2", &[1, 2, 3, 4]), [false, true, false, true]);
        assert!(HitCondition::parse("often").is_err());
        assert!(HitCondition::parse("% 0").is_err());
        assert_eq!(
            interpolate("n={n}, flag={flag}, x={nope}.", &scope()),
            "n=7, flag=true, x=<error: nope is not defined>."
        );
        assert_eq!(interpolate("open { brace", &scope()), "open { brace");
    }

    // M9-TS-3411
    #[test]
    fn objects_and_arrays_are_read_from_memory_by_their_layout() {
        // An object is a header word then one 8-byte slot per field; an array a header, a length,
        // a capacity and the address of its slots.
        let slots: Vec<u64> = vec![1.5f64.to_bits(), 2.5f64.to_bits(), 3.5f64.to_bits()];
        let array: Vec<u64> = vec![0, 3, 3, slots.as_ptr() as u64];
        let object: Vec<u64> = vec![0, 4f64.to_bits(), array.as_ptr() as u64];
        let structs = Arc::new(Structs::default());
        let arr_ty = ShowTy::Arr(Box::new(ShowTy::Num));
        let _ = structs.0.set(vec![StructInfo {
            name: "Thing".into(),
            fields: vec![
                ("size".into(), 8, ShowTy::Num),
                ("items".into(), 16, arr_ty),
            ],
        }]);
        let sc = Scope {
            locals: vec![("t".into(), object.as_ptr() as u64, ShowTy::Obj(structs, 0))],
        };
        let get = |text: &str| describe(&eval(&parse(text).unwrap(), &sc).unwrap()).1;
        assert_eq!(get("t"), "Thing");
        assert_eq!(get("t.size"), "4");
        assert_eq!(get("t.items"), "Array(3)");
        assert_eq!(get("t.items.length"), "3");
        assert_eq!(get("t.items[2]"), "3.5");
        assert_eq!(get("t.items[3]"), "undefined");
        assert_eq!(get("t.items[0] + t.items[1]"), "4");
        let t = eval(&parse("t").unwrap(), &sc).unwrap();
        let names: Vec<String> = children(&t).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["size", "items"]);
        let items = eval(&parse("t.items").unwrap(), &sc).unwrap();
        let shown: Vec<String> = children(&items)
            .into_iter()
            .map(|(n, v)| format!("{n}={}", describe(&v).1))
            .collect();
        assert_eq!(shown, ["[0]=1.5", "[1]=2.5", "[2]=3.5"]);
    }
}
