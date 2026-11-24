//! armasm-compatible expression evaluator.
//!
//! Precedence follows armasm (not C): unary > multiplicative > shift > additive/logical
//! (`+ - :AND: :OR: :EOR: & | ^` share one level, left to right) > relational > boolean.
//! Values track how many relocatable (address) terms they contain so the assembler can
//! tell constants from addresses, and whether a single Thumb code label is involved so
//! data references get the Thumb bit like armlink applies it.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Val {
    pub v: i64,
    /// Net count of address terms (+label adds 1, -label subtracts 1). 0 = constant.
    pub reloc: i32,
    /// The expression is `code_label (+/- constant)` with a Thumb code label.
    pub thumb: bool,
}

impl Val {
    pub fn konst(v: i64) -> Self {
        Self { v, reloc: 0, thumb: false }
    }
    pub fn is_const(&self) -> bool {
        self.reloc == 0
    }
    /// Value as stored in data (DCD / literal pool): Thumb code addresses get bit 0.
    pub fn data_value(&self) -> u32 {
        let v = self.v as u32;
        if self.thumb && self.reloc == 1 {
            v | 1
        } else {
            v
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprError {
    Undefined(String),
    Syntax(String),
}

impl std::fmt::Display for ExprError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExprError::Undefined(s) => write!(f, "Undefined symbol '{}'", s),
            ExprError::Syntax(s) => write!(f, "{}", s),
        }
    }
}

pub type ExprResult = Result<Val, ExprError>;

/// Symbol resolution for the evaluator.
pub trait Resolver {
    /// Look up a symbol. `None` = undefined.
    fn symbol(&self, name: &str) -> Option<Result<Val, ExprError>>;
    /// Address of the current statement (`{PC}` / `.`).
    fn here(&self) -> Option<i64>;
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(i64),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
}

const WORD_OPS: &[(&str, &str)] = &[
    (":AND:", "&"),
    (":OR:", "|"),
    (":EOR:", "^"),
    (":SHL:", "<<"),
    (":SHR:", ">>"),
    (":ROL:", "rol"),
    (":ROR:", "ror"),
    (":MOD:", "%"),
    (":NOT:", "~"),
    (":LNOT:", "!"),
    (":LAND:", "&&"),
    (":LOR:", "||"),
    (":LEOR:", "^^"),
];

const SYM_OPS: &[&str] = &[
    "<<", ">>", "<=", ">=", "==", "!=", "<>", "/=", "&&", "||", "+", "-", "*", "/", "%", "&", "|", "^", "~",
    "!", "<", ">", "=",
];

fn tokenize(s: &str) -> Result<Vec<Tok>, ExprError> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'(' {
            out.push(Tok::LParen);
            i += 1;
            continue;
        }
        if c == b')' {
            out.push(Tok::RParen);
            i += 1;
            continue;
        }
        if c == b':' {
            let rest = s[i..].to_ascii_uppercase();
            if let Some((w, op)) = WORD_OPS.iter().find(|(w, _)| rest.starts_with(w)) {
                out.push(Tok::Op(op));
                i += w.len();
                continue;
            }
            return Err(ExprError::Syntax(format!("Unknown operator in '{}'", s.trim())));
        }
        if c == b'{' {
            let end = s[i..].find('}').ok_or_else(|| ExprError::Syntax(format!("Missing '}}' in '{}'", s)))?;
            let name = s[i + 1..i + end].trim().to_ascii_uppercase();
            match name.as_str() {
                "PC" => out.push(Tok::Ident(".".into())),
                "TRUE" => out.push(Tok::Num(1)),
                "FALSE" => out.push(Tok::Num(0)),
                "ARCHITECTURE" | "CPU" | "CONFIG" | "ENDIAN" | "CODESIZE" => out.push(Tok::Num(0)),
                _ => return Err(ExprError::Syntax(format!("Unsupported built-in variable {{{}}}", name))),
            }
            i += end + 1;
            continue;
        }
        if c == b'\'' {
            // character constant
            if i + 2 < b.len() && b[i + 2] == b'\'' {
                out.push(Tok::Num(b[i + 1] as i64));
                i += 3;
                continue;
            }
            return Err(ExprError::Syntax(format!("Bad character constant in '{}'", s.trim())));
        }
        if c == b'|' && i + 1 < b.len() && b[i + 1] != b'|' && out_expects_operand(&out) {
            // |bar quoted symbol| (only when it closes on the same token and contains no spaces)
            if let Some(end) = s[i + 1..].find('|') {
                let inner = &s[i + 1..i + 1 + end];
                if !inner.is_empty() && !inner.contains(char::is_whitespace) && !inner.contains(['(', ')']) {
                    out.push(Tok::Ident(inner.to_string()));
                    i += end + 2;
                    continue;
                }
            }
        }
        if c == b'.' && (i + 1 >= b.len() || !(b[i + 1].is_ascii_alphanumeric() || b[i + 1] == b'_')) {
            out.push(Tok::Ident(".".into()));
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || c == b'&' && i + 1 < b.len() && b[i + 1].is_ascii_hexdigit() && out_expects_operand(&out) {
            let start = i;
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Tok::Num(parse_number(&s[start..i])?));
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' || c == b'.' || c == b'$' {
            let start = i;
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.' || b[i] == b'$') {
                i += 1;
            }
            out.push(Tok::Ident(s[start..i].to_string()));
            continue;
        }
        if let Some(op) = SYM_OPS.iter().find(|op| s[i..].starts_with(*op)) {
            out.push(Tok::Op(op));
            i += op.len();
            continue;
        }
        return Err(ExprError::Syntax(format!("Unexpected character '{}' in '{}'", c as char, s.trim())));
    }
    Ok(out)
}

fn out_expects_operand(out: &[Tok]) -> bool {
    matches!(out.last(), None | Some(Tok::Op(_)) | Some(Tok::LParen))
}

/// Parse an armasm/C numeric literal: `123`, `0x1F`, `&1F`, `2_1010`, `0b1010`, `8_777`.
pub fn parse_number(t: &str) -> Result<i64, ExprError> {
    let bad = || ExprError::Syntax(format!("Invalid number '{}'", t));
    let t = t.trim();
    let (digits, radix) = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        (h, 16)
    } else if let Some(h) = t.strip_prefix('&') {
        (h, 16)
    } else if let Some(bn) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
        (bn, 2)
    } else if let Some((base, rest)) = t.split_once('_') {
        let r: u32 = base.parse().map_err(|_| bad())?;
        if !(2..=9).contains(&r) {
            return Err(bad());
        }
        (rest, r)
    } else {
        (t, 10)
    };
    let digits = digits.trim_end_matches(['u', 'U', 'l', 'L']);
    if digits.is_empty() {
        return Err(bad());
    }
    u64::from_str_radix(digits, radix).map(|v| v as i64).map_err(|_| bad())
}

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    res: &'a dyn Resolver,
    src: &'a str,
}

fn binop_prec(op: &str) -> Option<u8> {
    Some(match op {
        "||" | "&&" | "^^" => 1,
        "=" | "==" | "!=" | "<>" | "/=" | "<" | ">" | "<=" | ">=" => 2,
        "+" | "-" | "&" | "|" | "^" => 3,
        "<<" | ">>" | "rol" | "ror" => 4,
        "*" | "/" | "%" => 5,
        _ => return None,
    })
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn parse(&mut self, min_prec: u8) -> ExprResult {
        let mut lhs = self.unary()?;
        while let Some(Tok::Op(op)) = self.peek() {
            let op = *op;
            let Some(prec) = binop_prec(op) else { break };
            if prec < min_prec {
                break;
            }
            self.pos += 1;
            let rhs = self.parse(prec + 1)?;
            lhs = self.apply(op, lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> ExprResult {
        match self.peek().cloned() {
            Some(Tok::Op("-")) => {
                self.pos += 1;
                let v = self.unary()?;
                Ok(Val { v: v.v.wrapping_neg(), reloc: -v.reloc, thumb: false })
            }
            Some(Tok::Op("+")) => {
                self.pos += 1;
                self.unary()
            }
            Some(Tok::Op("~")) => {
                self.pos += 1;
                let v = self.unary()?;
                self.need_const(&v, "~")?;
                Ok(Val::konst(!(v.v as u32) as i64))
            }
            Some(Tok::Op("!")) => {
                self.pos += 1;
                let v = self.unary()?;
                Ok(Val::konst((v.v == 0) as i64))
            }
            Some(Tok::LParen) => {
                self.pos += 1;
                let v = self.parse(0)?;
                if self.peek() != Some(&Tok::RParen) {
                    return Err(ExprError::Syntax(format!("Missing ')' in '{}'", self.src.trim())));
                }
                self.pos += 1;
                Ok(v)
            }
            Some(Tok::Num(n)) => {
                self.pos += 1;
                Ok(Val::konst(n))
            }
            Some(Tok::Ident(name)) => {
                self.pos += 1;
                if name == "." {
                    return match self.res.here() {
                        Some(pc) => Ok(Val { v: pc, reloc: 1, thumb: false }),
                        None => Err(ExprError::Syntax("{PC} is not available here".into())),
                    };
                }
                match self.res.symbol(&name) {
                    Some(r) => r,
                    None => Err(ExprError::Undefined(name)),
                }
            }
            _ => Err(ExprError::Syntax(format!("Malformed expression '{}'", self.src.trim()))),
        }
    }

    fn need_const(&self, v: &Val, op: &str) -> Result<(), ExprError> {
        if v.reloc != 0 {
            return Err(ExprError::Syntax(format!(
                "Operator '{}' cannot be applied to an address in '{}'",
                op,
                self.src.trim()
            )));
        }
        Ok(())
    }

    fn apply(&self, op: &str, a: Val, b: Val) -> ExprResult {
        let (x, y) = (a.v, b.v);
        Ok(match op {
            "+" => {
                let reloc = a.reloc + b.reloc;
                let thumb = (a.reloc == 1 && a.thumb && b.reloc == 0) || (b.reloc == 1 && b.thumb && a.reloc == 0);
                Val { v: x.wrapping_add(y), reloc, thumb }
            }
            "-" => {
                let reloc = a.reloc - b.reloc;
                Val { v: x.wrapping_sub(y), reloc, thumb: a.thumb && b.reloc == 0 && reloc == 1 }
            }
            _ => {
                // All other operators need constants (address differences are constants)
                self.need_const(&a, op)?;
                self.need_const(&b, op)?;
                let (ux, uy) = (x as u32, y as u32);
                let v: i64 = match op {
                    "*" => x.wrapping_mul(y),
                    "/" => {
                        if y == 0 {
                            return Err(ExprError::Syntax(format!("Division by zero in '{}'", self.src.trim())));
                        }
                        (ux / uy) as i64
                    }
                    "%" => {
                        if y == 0 {
                            return Err(ExprError::Syntax(format!("Division by zero in '{}'", self.src.trim())));
                        }
                        (ux % uy) as i64
                    }
                    "<<" => {
                        if (0..64).contains(&y) {
                            ((x as u64) << y) as i64
                        } else {
                            0
                        }
                    }
                    ">>" => {
                        if (0..32).contains(&y) {
                            (ux >> y) as i64
                        } else {
                            0
                        }
                    }
                    "rol" => ux.rotate_left(uy % 32) as i64,
                    "ror" => ux.rotate_right(uy % 32) as i64,
                    "&" => x & y,
                    "|" => x | y,
                    "^" => x ^ y,
                    "=" | "==" => (ux == uy) as i64,
                    "!=" | "<>" | "/=" => (ux != uy) as i64,
                    "<" => (ux < uy) as i64,
                    ">" => (ux > uy) as i64,
                    "<=" => (ux <= uy) as i64,
                    ">=" => (ux >= uy) as i64,
                    "&&" => (x != 0 && y != 0) as i64,
                    "||" => (x != 0 || y != 0) as i64,
                    "^^" => ((x != 0) ^ (y != 0)) as i64,
                    _ => return Err(ExprError::Syntax(format!("Unknown operator '{}'", op))),
                };
                Val::konst(v)
            }
        })
    }
}

/// Evaluate an expression.
pub fn eval(src: &str, res: &dyn Resolver) -> ExprResult {
    let toks = tokenize(src)?;
    if toks.is_empty() {
        return Err(ExprError::Syntax("Missing expression".into()));
    }
    let mut p = Parser { toks, pos: 0, res, src };
    let v = p.parse(0)?;
    if p.pos != p.toks.len() {
        return Err(ExprError::Syntax(format!("Unexpected text in expression '{}'", src.trim())));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct M(HashMap<&'static str, Val>);
    impl Resolver for M {
        fn symbol(&self, name: &str) -> Option<Result<Val, ExprError>> {
            self.0.get(name).map(|v| Ok(*v))
        }
        fn here(&self) -> Option<i64> {
            Some(0x100)
        }
    }

    fn ev(s: &str) -> i64 {
        let mut m = HashMap::new();
        m.insert("lab", Val { v: 0x0800_0010, reloc: 1, thumb: true });
        m.insert("K", Val::konst(5));
        eval(s, &M(m)).unwrap().v
    }

    #[test]
    fn precedence_follows_armasm() {
        assert_eq!(ev("1 << 2"), 4);
        assert_eq!(ev("(15 << 20)"), 15 << 20);
        assert_eq!(ev("1 + 2 << 3"), 1 + (2 << 3)); // shift binds tighter than +
        assert_eq!(ev("2 * 3 + 1"), 7);
        assert_eq!(ev("1 :OR: 6 :AND: 3"), (1 | 6) & 3); // same level, left to right
        assert_eq!(ev("0x10 | 1"), 0x11);
        assert_eq!(ev("&FF"), 255);
        assert_eq!(ev("2_1010"), 10);
        assert_eq!(ev("'A'"), 65);
        assert_eq!(ev("-1"), -1);
        assert_eq!(ev("~0"), 0xFFFF_FFFF);
        assert_eq!(ev("K*2"), 10);
        assert_eq!(ev("{PC}"), 0x100);
        assert_eq!(ev("10 :MOD: 3"), 1);
        assert_eq!(ev("1 :SHL: 4"), 16);
    }

    #[test]
    fn thumb_bit_tracking() {
        let mut m = HashMap::new();
        m.insert("lab", Val { v: 0x0800_0010, reloc: 1, thumb: true });
        m.insert("lab2", Val { v: 0x0800_0020, reloc: 1, thumb: true });
        let r = M(m);
        assert_eq!(eval("lab", &r).unwrap().data_value(), 0x0800_0011);
        assert_eq!(eval("lab+4", &r).unwrap().data_value(), 0x0800_0015);
        let d = eval("lab2-lab", &r).unwrap();
        assert!(d.is_const());
        assert_eq!(d.data_value(), 0x10);
    }
}
